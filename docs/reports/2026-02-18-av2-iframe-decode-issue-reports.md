# AV2 Iframe Decode Investigation — Issue Reports

Date: 2026-02-18
Project: `wav2c` (worktree `phase4-partition-tree`)
Reference decoder: `../avm_build/avmdec`

## Validation Snapshot

- `reconstruct_avmenc_minimal_frame`: **PSNR Y/U/V = 100 dB** (minimal stream OK)
- `decode_avmenc_noibc_psnr`: **Y=48.04 dB, U=11.61 dB, V=7.96 dB**
- `decode_avmenc_nofilter_psnr`: **Y=7.97 dB, U=11.42 dB, V=10.47 dB**
- `decode_avmenc_128x128_q30`: **Y=8.58 dB**

This shows simple/minimal bitstreams decode correctly, but real AV2 intra bitstreams diverge strongly.

---

## Issue 1 — IntraBC motion vector reconstruction is incomplete

### Severity
Critical

### Symptom
Large luma mismatch when IntraBC is enabled; catastrophic in `nofilter` and full q30 fixtures.

### Evidence
- `src/decode/tile.rs` in `read_intrabc_rest()`:
  - For `intrabc_mode != 0`, motion is forced to `(0,0)`.
  - For `intrabc_mode == 0`, only delta is decoded; selected reference BV is not added.
- In traces, wav2c decodes `ibc_mode=1` and DRL index, but still reconstructs using effectively zero/partial BV.
- Reference trace from `avmdec` shows non-zero BV for same blocks, e.g. `(0,-64)`.

### Root Cause
The decoder does not implement AV2’s full IntraBC candidate selection + final BV derivation path (DRL-selected ref BV plus optional delta depending on mode).

### Reference Behavior
- `../av2/av2/decoder/decodemv.c`
  - `read_intrabc_info()`
  - `assign_dv()`
- Mode 1 uses selected reference BV directly; mode 0 applies decoded delta on top of selected BV.

### Required Fix
1. Build and use IntraBC reference BV candidate stack.
2. Apply DRL-selected candidate.
3. For mode 0, add decoded delta at correct precision.
4. Keep precision/morph constraints aligned with AV2.

### Acceptance Criteria
- `decode_avmenc_nofilter_psnr` luma rises substantially (target: >30 dB after this fix alone).
- IntraBC-heavy blocks align with reference BV directions and magnitudes.

---

## Issue 2 — UV mode index is not mapped through AV2 uv mode list

### Severity
High

### Symptom
Non-CFL chroma prediction uses wrong intra modes in many blocks.

### Evidence
- `src/decode/tile.rs` (`read_uv_mode()`): decoded `uv_mode_idx` is used directly as final mode.
- AV2 requires mapping `uv_mode_idx` through `uv_intra_mode_list`, which is reordered based on luma mode.

### Root Cause
Decoder skips `uv_mode_idx -> uv_intra_mode_list[uv_mode_idx]` translation and treats index as absolute mode id.

### Reference Behavior
- `../av2/av2/decoder/decodemv.c` (`read_intra_uv_mode()`)
- `../av2/av2/common/reconintra.c` (`get_uv_intra_mode_set()`)

### Required Fix
1. Recreate `uv_intra_mode_list` per block from colocated Y mode.
2. Map decoded `uv_mode_idx` via the list.
3. Handle directional angle-delta coupling when UV mode equals Y mode.

### Acceptance Criteria
- Chroma non-CFL blocks match reference mode selections.
- U/V PSNR improves on noibc and nofilter fixtures.

---

## Issue 3 — CFL signaling is collapsed to explicit/zero-alpha behavior

### Severity
Critical

### Symptom
Very low chroma quality (U/V PSNR around 8–12 dB) even in noibc fixture.

### Evidence
- `src/decode/tile.rs` (`read_uv_mode()`):
  - If `cfl_mhccp_switch != 0`, returns CFL with `(alpha_u, alpha_v)=(0,0)`.
  - If `cfl_idx != 0`, also returns CFL with zeros.
- AV2 defines distinct CFL types:
  - `0`: explicit alpha
  - `1`: derived alpha
  - `2`: MHCCP multi-parameter

### Root Cause
Only explicit-alpha subset is partially modeled; derived-alpha and MHCCP paths are not implemented and are silently degraded to zero-alpha prediction.

### Reference Behavior
- `../av2/av2/common/enums.h` (CFL type definitions)
- `../av2/av2/decoder/decodemv.c` (cfl index/signaling)
- `../av2/av2/common/reconintra.c` and `../av2/av2/common/cfl.c` (actual CFL reconstruction)

### Required Fix
1. Preserve and store full CFL type (`cfl_idx`) and parameters.
2. Implement explicit-alpha path bit-exactly.
3. Implement derived-alpha path.
4. Implement MHCCP path (filter direction + multi-parameter prediction).

### Acceptance Criteria
- U/V PSNR on `decode_avmenc_noibc_psnr` increases from ~8–12 dB to significantly higher values.
- CFL blocks no longer collapse to DC-like chroma output.

---

## Issue 4 — Full q30 fixture enables post/extra tools not reconstructed

### Severity
High (for full-feature streams)

### Symptom
`decode_avmenc_128x128_q30` remains very poor after entropy/partition parsing due to missing reconstruction stages.

### Evidence
- Full q30 fixture enables tools such as CDEF/GDF/restoration/CCSO (seen in parsed sequence/frame flags).
- Current decoder mostly parses related syntax but does not apply full reconstruction effects.

### Root Cause
Bitstream parsing coverage is ahead of pixel-domain reconstruction coverage for full toolset.

### Reference Behavior
Reference decoder applies enabled in-loop and auxiliary tools during reconstruction pipeline.

### Required Fix
1. Implement enabled reconstruction tools in AV2-compliant order.
2. Gate each stage by parsed frame/sequence flags.
3. Validate per-tool impact with isolated fixtures.

### Acceptance Criteria
- Full q30 fixture PSNR and visual output move close to reference.
- Regression tests demonstrate no breakage on minimal/noibc/nofilter fixtures.

---

## Recommended Implementation Order

1. **Issue 1 (IntraBC BV derivation)**
2. **Issue 3 (CFL type support)**
3. **Issue 2 (UV mode list mapping)**
4. **Issue 4 (post/extra reconstruction tools)**

This order addresses the largest current error sources first and should quickly improve both luma and chroma agreement with `avmdec`.

---

## Implementation Progress (2026-02-18)

### Issue 1 — IntraBC (in progress)

Implemented:
- Added IntraBC BV candidate stack reconstruction in `src/decode/tile.rs`:
  - candidate collection from neighboring decoded IntraBC blocks,
  - default AV2 fallback BVs,
  - DRL index selection,
  - mode-dependent final BV derivation (`mode 1 => ref`, `mode 0 => ref + delta`).
- Added per-MI storage for decoded IntraBC BVs in `ModeInfoGrid`.
- Added precision reduction helper for reference BV when needed.
- Improved IntraBC context handling for `intrabc`, `skip_txfm`, `morph_pred` using AV2-like neighbor ordering semantics.
- Fixed IntraBC syntax consumption bug in luma-only tree:
  - no longer decodes chroma TUs for IntraBC blocks when `tree_type == Luma`.

Observed impact:
- `decode_avmenc_nofilter_psnr` Y improved from ~7.97 dB baseline to ~9.96 dB after current IntraBC fixes.
- Stream still diverges from reference IntraBC decisions in several regions; additional IntraBC syntax/context parity work remains.

### Issue 2 — UV mode list mapping (partially implemented)

Implemented:
- Added AV2 uv mode list reordering and mapping in `read_uv_mode()` (`src/decode/tile.rs`), instead of using raw `uv_mode_idx` directly.

Observed impact:
- `decode_avmenc_noibc_psnr` improved substantially:
  - U: **11.61 dB → 21.21 dB**
  - V: **7.96 dB → 11.06 dB**
  - Y unchanged (~48.04 dB)

Remaining:
- Chroma still not reference-accurate; additional MHCCP parity work is needed.

### Issue 3 — CFL derived/MHCCP (partially implemented)

Implemented:
- Added decode-time preservation of CFL tool type in `DecodedBlock`:
  - `cfl_idx` (explicit / derived / multi-parameter)
  - `mhccp_dir` for multi-parameter mode.
- `read_uv_mode()` now consumes and stores CFL type signaling instead of collapsing all non-explicit cases to zero-alpha.
- Reconstruction updated:
  - explicit-CFL path kept,
  - derived-alpha fallback implemented using neighbor-based luma/chroma regression,
  - approximate MHCCP predictor implemented with neighbor-driven 3-parameter fit and regularization.

Observed impact:
- Major chroma gain on noibc fixture (see above).
- nofilter fixture chroma changed slightly (`U=11.57 dB`, `V=10.29 dB`), with luma still primarily limited by remaining IntraBC/bitstream-path divergence.

Remaining:
- MHCCP implementation is still an approximation, not bit-exact AV2 behavior.
- Full derived/MHCCP parity requires closer replication of AV2 neighbor fetch, parameter derivation, and prediction stages.

### Issue 4 — Full-feature post/extra tools (not started)

Status:
- No implementation yet in this pass.
