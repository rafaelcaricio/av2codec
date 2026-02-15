# Phase 1: Multi-Frame All-Intra Encoding

## Goal

Encode a 2-second all-intra bitstream at 25fps (50 frames) with solid-color
DC-predicted frames. Output must be decodable by avmdec with correct pixel values.

## Architecture Changes

### From Phase 0 (still picture) to Phase 1 (video mode)

| Component | Phase 0 | Phase 1 |
|-----------|---------|---------|
| Sequence header | `single_picture_header_flag=1` | `=0`, video-mode fields |
| Frame header | No frame_type, no order_hint | CLK implicit KEY_FRAME, order_hint, show_frame |
| Tile data | Zero coefficients (all skip) | DC coefficient coded (non-zero DC, zero AC) |
| Frames | Single frame | 50 frames, each a CLK OBU |
| Input | None (hardcoded) | Y4M file parser |
| Output | `Vec<u8>` in memory | `.obu` file on disk |

### Bitstream Structure

```
[Sequence Header OBU]   ← once at start
[CLK OBU frame 0]       ← frame header + tile data with DC coefficients
[CLK OBU frame 1]
...
[CLK OBU frame 49]
```

Each CLK OBU implies KEY_FRAME — no explicit frame_type bit needed.

## New Modules

### `src/y4m.rs` — Y4M Parser

Reads multi-frame Y4M files (YUV4MPEG2 format):
- Parse header: width, height, framerate, chroma subsampling, interlacing
- Iterate frames: yield raw YUV 4:2:0 pixel data per frame
- Validate: 8-bit only, 4:2:0 only (match our encoder constraints)

### `src/bin/wav2c.rs` — CLI Binary

Simple command-line encoder:
```
wav2c input.y4m -o output.obu [--qp 50]
```

## Modified Modules

### `src/bitstream/sequence.rs` — Video Mode Sequence Header

Switch from `single_picture_header_flag=1` to `=0`. Additional fields:

After profile:
- `single_picture_header_flag`: 1 bit (0)
- `seq_lcr_id`: 3 bits (0 = LCR_ID_UNSPECIFIED)
- `still_picture`: 1 bit (0 = video)
- `seq_max_level_idx`: 5 bits (0)

After chroma/bitdepth:
- `seq_max_display_model_info_present_flag`: 1 bit (0)
- `decoder_model_info_present_flag`: 1 bit (0)

Layer fields:
- `max_tlayer_id`: 2 bits (0)
- `max_mlayer_id`: 3 bits (0)

Inter group changes (no longer skipped for video mode):
- Motion modes block: all motion modes disabled (INTERINTRA through WARP_EXTEND = 0)
- `seq_frame_motion_modes_present_flag`: not written (all disabled)
- `enable_six_param_warp_delta`: not written (warp_delta disabled)
- `enable_masked_compound`: 1 bit (0)
- `enable_ref_frame_mvs`: 1 bit (0)
- `order_hint_bits_minus_1`: 3 bits (value TBD, minimum needed for 50 frames)

Additional fields no longer skipped:
- `enable_inter_ddt`: 1 bit (0)
- `enable_avg_cdf`: 1 bit (0)
- `cdef_on_skip_txfm`: 1 bit (0)
- Various inter tools: `num_same_ref_compound`, `enable_tip`, `enable_mv_traj`,
  `enable_cwp`, `enable_imp_msk_bld`, `enable_lf_sub_pu`, `enable_opfl_refine`,
  `enable_refinemv`, `enable_bru`, `enable_adaptive_mvd`, `enable_mvd_sign_derive`,
  `enable_flex_mvres`, `enable_global_motion`, `enable_short_refresh_frame_flags`

### `src/bitstream/frame.rs` — Video Mode Frame Header

New fields for CLK OBU in video mode:
- `cur_mfh_id`: UVLC (0 for first, incrementing)
- `seq_header_id`: UVLC (0, written when cur_mfh_id matches)
- `long_term_id`: N bits (based on seq header lt_frame_id config)
- `immediate_output_picture`: 1 bit (1 = show_frame)
- `frame_size_override_flag`: 1 bit (0)
- `order_hint`: order_hint_bits (incrementing per frame)
- `refresh_frame_flags`: explicit for video mode CLK

### `src/tile.rs` — DC Coefficient Coding

Replace all-skip with actual DC coefficient encoding:

For each 64x64 superblock:
1. Partition: PARTITION_NONE (unchanged)
2. Intra mode: DC_PRED (unchanged)
3. Coefficient coding per plane (Y, U, V):
   - If residual == 0: EOB skip (current behavior)
   - If residual != 0: EOB=1 (one coeff), encode DC level + sign, skip AC

Coefficient encoding sequence for non-zero DC:
- `all_zero` = 0 (block has coefficients)
- `eob_extra` (EOB position encoding)
- Coefficient base level (CDF-based)
- Coefficient remainder (Golomb-Rice or Exp-Golomb)
- Sign bit

### `src/entropy.rs` — Coefficient CDF Tables

Add default CDF tables from AV2 spec for:
- EOB position CDFs (per transform size)
- Coefficient base level CDFs
- Coefficient base range CDFs
- Sign CDFs (bypass-coded, uniform)

### `src/encode.rs` — Multi-Frame Encoding

- Accept pixel data per frame (Y, U, V planes)
- Compute DC prediction value: 128 (no neighbors in all-intra, first block default)
- Compute residual: pixel_value - 128
- Forward DCT for DC-only: residual_dc = sum_of_residuals (scaled)
- Quantize DC coefficient using base_qindex
- Loop over all frames, produce one CLK OBU per frame
- Prepend sequence header OBU once

## DC Prediction & Coefficient Math

For a solid-color frame with DC prediction:
1. Predicted value = 128 (no reconstructed neighbors → mid-gray default)
2. Residual per pixel = actual_value - 128
3. Forward DCT of uniform NxN residual block: DC coefficient = residual × N
4. Quantize: `qcoeff = round(dc_coeff / quantizer_step)`
5. Dequantize: `recon_dc = qcoeff * quantizer_step`
6. Inverse DCT → recon_residual per pixel
7. Reconstructed = predicted + recon_residual

For pixel == 128: residual = 0, all-skip (zero cost).
For any other solid color: exactly one non-zero DC coefficient per block.

## Testing Strategy

### Unit Tests
- Y4M parser: header parsing, frame iteration, invalid input handling
- Updated sequence header bit patterns for video mode
- Updated frame header with video-mode fields
- DC coefficient encoding (zero and non-zero cases)

### Integration Tests
- Single solid-color frame → avmdec → verify pixel values match
- 50-frame solid green → avmdec → verify all 50 frames decode
- Multiple colors (green, red, gray) → verify each correct
- Roundtrip: encode → decode → pixel comparison

### Regression
- All Phase 0 unit tests updated for new sequence/frame header format
- Bitstream structure tests adapted to video mode

## Success Criteria

1. `wav2c input.y4m -o output.obu` produces valid AV2 bitstream
2. `avmdec output.obu -o decoded.y4m` decodes all frames without error
3. Decoded pixel values match input for solid-color frames
4. 50-frame encoding works end-to-end (2 seconds at 25fps)
5. All tests pass (`cargo test` + `cargo clippy --tests`)
