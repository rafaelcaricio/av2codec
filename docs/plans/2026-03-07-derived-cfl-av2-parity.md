# Derived-CFL AV2 Parity Plan

## Goal

Replace the decoder's current derived-CFL approximation with an AV2-semantic port so `cfl_idx == 1` blocks match avmdec at the helper, alpha, and predictor level before any further MHCCP work.

## Scope

- Port the derived-CFL path in [`src/decode/mod.rs`](/Users/rafaelcaricio/development/wav2c/src/decode/mod.rs) to mirror AV2 neighbor fetch, luma DC derivation, implicit alpha derivation, and final predictor construction.
- Remove the current caller-side V-plane negation heuristic as part of the full port, not as an isolated sign fix.
- Use the two known `cfl_idx=1` blocks as hard parity gates:
  - `(32,8)` / `mi=(2,8)` / `cfl_idx=1`
  - `(80,64)` / `mi=(16,20)` / `cfl_idx=1`
- Defer MHCCP changes until derived-CFL parity is stable.

## Reference Semantics To Mirror

Mirror this AV2 helper chain exactly:

1. `cfl_store`
2. `cfl_implicit_fetch_neighbor_luma`
3. `cfl_calc_luma_dc`
4. `cfl_implicit_fetch_neighbor_chroma`
5. `cfl_derive_implicit_scaling_factor`
6. `av2_cfl_predict_block`

Key requirements from AV2:

- Use fetched top/left luma and chroma boundary buffers rather than scanning arbitrary reconstructed neighbors.
- Apply `cfl_ds_filter_index` during luma neighbor fetch.
- Derive `avg_l` from fetched luma refs, including AV2's large-block subsampling rule.
- Derive `alpha_u` and `alpha_v` independently from the fetched refs using AV2's 8-sample top/left distribution and sample-index selection.
- Use the derived signed alpha directly per plane. Do not special-case V sign outside the AV2-equivalent derivation path.
- Build the final predictor from DC chroma prediction plus scaled colocated luma AC relative to `avg_l`.

## Parity Checkpoints

For each checkpoint block, compare wav2c against avmdec on:

- fetched top luma refs
- fetched left luma refs
- fetched top chroma refs
- fetched left chroma refs
- `avg_l`
- sampled top/left indices
- `alpha_u`
- `alpha_v`
- final CFL predictor block for U and V

Primary checkpoints:

- `(32,8)` / `mi=(2,8)` / `cfl_idx=1`
- `(80,64)` / `mi=(16,20)` / `cfl_idx=1`

Current downstream target after derived-CFL parity:

- MHCCP gap at `(48,48)` / `Block[29]`

## Likely Touchpoints

- [`src/decode/mod.rs`](/Users/rafaelcaricio/development/wav2c/src/decode/mod.rs)
  - Replace `cfl_neighbor_avg_q3`
  - Replace `derive_cfl_alpha_q3_neighbors`
  - Add AV2-equivalent neighbor-fetch and alpha-derivation helpers
  - Update the `uv_mode == 13` reconstruction path to use the new helper chain
- [`src/decode/mod.rs`](/Users/rafaelcaricio/development/wav2c/src/decode/mod.rs)
  - Add a narrow regression around the first parity block once parity is confirmed

## Implementation Steps

1. Add explicit derived-CFL helper state in Rust
- Introduce small helper structs for fetched top/left luma refs, fetched top/left chroma refs, sampled indices, and derived alpha outputs.
- Keep the implementation local to the decoder path; do not change bitstream parsing.

2. Port AV2 luma neighbor fetch
- Implement Rust equivalents of `cfl_implicit_fetch_neighbor_luma` and its 4:2:0 fetch behavior.
- Honor chroma-reference anchoring, top/left availability, `cfl_ds_filter_index`, top-SB-boundary handling, and edge padding.

3. Port AV2 luma DC derivation
- Implement a Rust equivalent of `cfl_calc_luma_dc`.
- Derive `avg_l` only from the fetched luma ref buffers, including AV2's `width > 32` / `height > 32` subsampling behavior.

4. Port AV2 chroma neighbor fetch
- Implement a Rust equivalent of `cfl_implicit_fetch_neighbor_chroma`.
- Copy raw chroma top/left refs with AV2-style edge extension.

5. Port AV2 implicit alpha derivation
- Implement a Rust equivalent of `cfl_derive_implicit_scaling_factor`.
- Use AV2's 8-sample top/left split, index stepping, regression inputs, and signed alpha behavior.
- Remove the current V-specific negation at the call site only after this port is in place.

6. Rebuild the derived-CFL predictor path
- Keep DC prediction generation for U and V.
- Replace the current neighbor-average/least-squares approximation with the AV2 helper chain output.
- Use colocated luma AC relative to `avg_l` when applying the derived alpha.

7. Only after `cfl_idx=1` parity is solid, return to MHCCP
- Re-check `(48,48)` / `Block[29]` as the next UV error hotspot.
- Do not mix MHCCP debugging into the initial derived-CFL parity patch.

## Validation

Run after each derived-CFL change:

```bash
env WAV2C_TRACE_CFL=16,4 cargo test decode_avmenc_noibc_psnr -- --nocapture
env WAV2C_TRACE_CFL=40,32 cargo test decode_avmenc_noibc_psnr -- --nocapture
env WAV2C_TRACE_CFL_MI=2,8 /Users/rafaelcaricio/development/avm_build/avmdec --codec=av2 -o /tmp/derived_cfl_ref_1.y4m tests/fixtures/avmenc_128x128_q30_noibc.obu
env WAV2C_TRACE_CFL_MI=16,20 /Users/rafaelcaricio/development/avm_build/avmdec --codec=av2 -o /tmp/derived_cfl_ref_2.y4m tests/fixtures/avmenc_128x128_q30_noibc.obu
```

Then run full validation:

```bash
cargo test
cargo clippy --tests
cargo test decode_avmenc_nofilter_psnr -- --nocapture
cargo test decode_avmenc_noibc_psnr -- --nocapture
cargo test avmdec_wav2c_decoder_agreement_multi_sb -- --nocapture
```

Expected result:

- The two checkpoint blocks match avmdec on fetched refs, sampled indices, `avg_l`, derived alphas, and final U/V predictor blocks.
- No regression in broader decoder agreement tests.
- MHCCP remains the next meaningful UV gap instead of derived-CFL.

## Cleanup

After parity is confirmed:

- Remove `WAV2C_TRACE_CFL` hooks from [`src/decode/mod.rs`](/Users/rafaelcaricio/development/wav2c/src/decode/mod.rs).
- Remove local AV2 debug hooks from `../av2/av2/common/cfl.c` and `../av2/av2/common/reconintra.c`.
- Keep only the regression coverage needed to guard the parity block and broader decoder agreement.

## Regression-Test Plan

- Add a narrow regression test around the first parity block `(32,8)` / `mi=(2,8)` / `cfl_idx=1`.
- Assert block-level U/V predictor agreement against avmdec-derived expected values instead of relying only on whole-frame PSNR.
- Keep the existing decoder-agreement and PSNR tests as broader safety nets, but make the new block-level test the primary guard for the derived-CFL port.
