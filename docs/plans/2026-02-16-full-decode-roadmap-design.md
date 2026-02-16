# Full AV2 Decode Capability — Phases 3-6 Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement each phase's plan task-by-task.

**Goal:** Decode avmenc-encoded AV2 intra-only bitstreams to pixel-accurate output matching the reference avmdec decoder.

**Architecture:** Four phases in strict dependency order — each unlocks the next. CDF adaptation enables correct entropy decoding, which enables partition parsing, which requires multi-size transforms, which benefits from the full intra mode set.

**Validation:** Each phase runs `scripts/compare_decoders.sh` against the test photo to visualize progress.

---

## Dependency Chain

```
CDF Adaptation → Partition Parsing → Multi-size Transforms → All Intra Modes
     (Phase 3)       (Phase 4)           (Phase 5)              (Phase 6)
```

---

## Phase 3: CDF Adaptation Engine

**Problem:** avmenc encodes with `disable_cdf_update=0`. Our decoder uses static ICDF tables. After the first few symbols, probability estimates diverge and every subsequent symbol decodes incorrectly.

**Solution:** Implement `update_cdf()` — a counter-based exponential moving average that adapts CDF probabilities after each symbol decode.

### AV2 CDF Format

Each CDF table has `nsymbs + 1` entries:
- `cdf[0..nsymbs]` — cumulative probabilities (Q15 format, 0..32768)
- `cdf[nsymbs]` — adaptation counter (controls update rate)

The update algorithm (from AV2 spec):
```
rate = 4 + (count < 32 ? 0 : 1) + log2floor(nsymbs)
for each symbol i:
    if i == decoded_symbol:
        cdf[i] += (32768 - cdf[i]) >> rate
    else:
        cdf[i] -= cdf[i] >> rate
count = min(count + 1, 32)
```

### Changes Required

1. **Mutable CDF tables** — Convert from `&[u16]` static ICDF slices to owned `Vec<u16>` CDF tables, initialized per-frame from defaults
2. **CDF ↔ ICDF conversion** — AV2 uses CDF internally (cumulative ascending), our Daala decoder uses ICDF (cumulative descending). Either convert tables or adapt the update logic
3. **`update_cdf()` in DaalaDecoder** — Called after every `decode_symbol()` when `allow_update_cdf=true`
4. **Per-frame CDF reset** — At frame start, copy default CDFs into working set
5. **Encoder side** — Add `update_cdf()` to DaalaCoder to keep encoder/decoder CDF state synchronized (needed for our own bitstreams when `disable_cdf_update=false`)

### Files Affected
- `src/decode/entropy.rs` — Add update logic to DaalaDecoder
- `src/entropy.rs` — Add update logic to DaalaCoder
- `src/cdf_tables.rs` — Add CDF-format default tables (or conversion from existing ICDF)
- `src/decode/tile.rs` — Initialize mutable CDF context per frame
- `src/tile.rs` — Mirror on encoder side

### Visible Result
Still gray output for avmenc bitstreams (partition parsing still not implemented), but the entropy engine correctly adapts probabilities. Our own encode→decode roundtrip still works with CDF updates enabled.

---

## Phase 4: Recursive Partition Tree

**Problem:** avmenc splits 64x64 superblocks into variable-size sub-blocks using a recursive partition tree. Our decoder assumes flat 64x64 Y / 32x32 UV blocks.

**Solution:** Parse the partition tree symbols and recursively decode blocks at their actual sizes.

### Partition Types (10 total)

| Type | Split | Description |
|------|-------|-------------|
| PARTITION_NONE | No | Single block at current size |
| PARTITION_SPLIT | 4-way | Recursive quad-split |
| PARTITION_HORZ | 2-way | Top + bottom halves |
| PARTITION_VERT | 2-way | Left + right halves |
| PARTITION_HORZ_3 | 3-way | Uneven horizontal (4:1, 2:1, 4:1) |
| PARTITION_VERT_3 | 3-way | Uneven vertical (4:1, 2:1, 4:1) |
| PARTITION_HORZ_4A | 4-way | Uneven horizontal (1:2:4:1) |
| PARTITION_HORZ_4B | 4-way | Uneven horizontal (1:4:2:1) |
| PARTITION_VERT_4A | 4-way | Uneven vertical (1:2:4:1) |
| PARTITION_VERT_4B | 4-way | Uneven vertical (1:4:2:1) |

### Parsing Decision Tree (5 binary CDF decisions)

1. `do_split` (CDF: `do_split_cdf[plane][ctx]`) → split or NONE?
2. `do_square_split` (CDF: `do_square_split_cdf[plane][ctx]`) → quad or rect?
3. `rect_type` (CDF: `rect_type_cdf[plane][ctx]`) → HORZ or VERT?
4. `do_ext_partition` (CDF: `do_ext_partition_cdf[plane][ctx]`) → 2-way or 3/4-way?
5. `do_uneven_4way` + 1 literal bit → which extended type?

### Implementation Plan

**Incremental approach:**
1. First: PARTITION_NONE + PARTITION_SPLIT only (covers the recursive quad tree)
2. Then: PARTITION_HORZ + PARTITION_VERT (rectangular splits)
3. Then: Extended partitions (3-way and 4-way)

### Block Sizes

AV2 uses block sizes from 4x4 to 128x128 (superblock). The partition tree recursively splits:
- 128x128 → 64x64 → 32x32 → 16x16 → 8x8 → 4x4

Each leaf block gets:
- Intra prediction mode (decoded from CDFs)
- Transform size (derived from block size or explicitly signaled)
- Quantized coefficients

### Changes Required

1. **Partition tree parser** — Recursive function that reads partition symbols and returns block layout
2. **Variable-size block decoder** — Mode and coefficient decoding for arbitrary block sizes
3. **Block-level reconstruction** — Prediction + inverse transform + residual add for each block
4. **Context propagation** — Partition context depends on neighboring blocks' partition state

### Files Affected
- `src/decode/tile.rs` — Major rewrite: recursive partition parsing, variable block decode
- `src/decode/mod.rs` — Frame reconstruction with variable block sizes
- `src/decode/transform.rs` — Accept variable block sizes (delegate to Phase 5 for new kernels)

### Visible Result
Recognizable output. Using DC prediction as fallback for all blocks, the photo appears blurry but the structure (face, shoulders, background) is visible. Block boundaries may show artifacts.

---

## Phase 5: Multi-size Transforms

**Problem:** Variable-size blocks need matching transform sizes. We only have 32x32 and 64x64 DCT-2 kernels.

**Solution:** Add DCT-2 for all square sizes (4x4, 8x8, 16x16) and rectangular transforms by composing 1D transforms.

### Transform Sizes Needed

**Square (minimum viable):**
- TX_4X4 (shifts: fwd={3,?}, inv={?,?})
- TX_8X8
- TX_16X16
- TX_32X32 (already implemented)
- TX_64X64 (already implemented)

**Rectangular (for full support):**
- TX_4X8, TX_8X4, TX_8X16, TX_16X8, TX_16X32, TX_32X16
- TX_32X64, TX_64X32, TX_4X16, TX_16X4, TX_8X32, TX_32X8

### 1D DCT Kernels

The reference decoder has integer DCT-2 kernels in `av2/common/txb_common.c`:
- `tx_kernel_dct2_size4[4][4]`
- `tx_kernel_dct2_size8[8][8]`
- `tx_kernel_dct2_size16[16][16]`
- `tx_kernel_dct2_size32[32][32]` (we already have this)
- `tx_kernel_dct2_size64[64][64]` (we already have this)

2D transforms = column transform → row transform with appropriate shift/round.

### TX_MODE and tx_size Selection

- `TX_MODE_LARGEST` — Use largest possible transform for block size
- `TX_MODE_SELECT` — Per-block transform size signaled via `read_tx_partition()`
- Intra frames typically use TX_MODE_SELECT

### Changes Required

1. **New DCT kernels** — Extract 4x4, 8x8, 16x16 from reference
2. **Parameterized inverse transform** — `inverse_dct2(coeffs, width, height)` dispatching to correct kernel
3. **Rectangular transform support** — Compose 1D transforms of different sizes for rows vs columns
4. **tx_size parsing** — Read transform partition info from bitstream
5. **Dequantization scaling** — log_scale varies by transform size

### Files Affected
- `src/dct.rs` — New kernels, parameterized forward/inverse functions
- `src/decode/transform.rs` — Variable-size dequant + inverse transform
- `src/scan.rs` — Scan tables for smaller block sizes

### Visible Result
Major quality jump. Coefficients now correctly dequantize and reconstruct at actual block sizes. The photo is clearly recognizable with good detail, though wrong prediction modes cause block-boundary artifacts.

---

## Phase 6: All Intra Prediction Modes

**Problem:** We have 5 of 13 intra modes. Missing: V, H, D45, D135, D113, D157, D203, D67 (directional) and CFL (chroma-from-luma).

**Solution:** Implement the 8 directional angle-based predictors and CFL.

### Directional Modes

Each directional mode extrapolates from reference pixels along a specific angle:
- V_PRED (90°), H_PRED (180°) — Axis-aligned, simplest
- D45 (45°), D135 (135°), D113 (113°), D157 (157°), D203 (203°), D67 (67°) — Diagonal

Implementation uses a lookup table mapping angle → dx/dy displacement, then sub-pixel interpolation from above/left reference pixels.

### Chroma-from-Luma (CFL)

UV planes predicted from reconstructed luma using a linear model:
```
chroma_pred[x,y] = dc_pred + alpha * (luma_ac[x,y])
```
Where `alpha` is signaled per-block and `luma_ac` is the AC component of the subsampled luma.

### Mode Decoding

Luma mode uses a 2-level index:
1. `y_mode_set_cdf` (4 symbols) — mode set selection
2. `y_mode_idx_cdf` (8 symbols) — mode within set
3. Optional `y_mode_idx_offset_cdf` (6 symbols) — for extended modes

Chroma mode:
1. `uv_mode_cdf` (8 symbols) — mode index
2. Optional offset for extended modes
3. CFL alpha parameters if CFL mode selected

### Changes Required

1. **8 directional predictors** — Angle-based extrapolation with sub-pixel filtering
2. **CFL predictor** — Luma downsampling + linear model
3. **Mode decode update** — Parse full mode set/index/offset from CDF
4. **Encoder support** — Add directional modes to mode selection (improves our own encoding too)

### Files Affected
- `src/encode.rs` — IntraMode enum expanded, new predictors, updated mode selection
- `src/decode/tile.rs` — Full mode decoding
- `src/cdf_tables.rs` — CFL and extended mode CDFs

### Visible Result
Near-pixel-perfect decode matching avmdec. The comparison script shows three nearly identical images.

---

## Validation Strategy

Each phase adds:
1. **Unit tests** for new functionality
2. **Roundtrip tests** ensuring our own encode→decode still works
3. **Visual comparison** via `scripts/compare_decoders.sh -e /tmp/eu_avmenc.obu photo.png`
4. **PSNR measurement** against avmdec output to track convergence

## Success Metric

Phase 6 complete when: `compare_decoders.sh` shows visually identical output between avmdec and wav2c for the test photo, with PSNR > 40dB.
