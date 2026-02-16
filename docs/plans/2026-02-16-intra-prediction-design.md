# Non-Directional Intra Prediction Modes — Design

## Goal

Add non-directional intra prediction modes (DC, SMOOTH, SMOOTH_V, SMOOTH_H, PAETH) with brute-force SAD mode selection to dramatically improve visual quality on real images.

## Architecture

Currently every block uses DC_PRED which predicts a flat value from neighbor borders. This wastes bits encoding large residuals for blocks with gradients or edges. Adding SMOOTH variants and PAETH captures these patterns with much lower residual energy.

We implement only the 5 non-directional modes (positions 0-4 in the decoder's mode list). These positions are fixed regardless of neighbor modes, so no dynamic mode list construction is needed. The CDF context for both Y and UV mode signaling stays at 0 since no angular modes are used.

## Mode Descriptions

| Mode | Index | Prediction |
|------|-------|------------|
| DC_PRED | 0 | Flat fill with average of above+left borders |
| SMOOTH_PRED | 1 | Bi-directional weighted interpolation of all 4 borders |
| SMOOTH_V_PRED | 2 | Vertical-only weighted interpolation (above → bottom) |
| SMOOTH_H_PRED | 3 | Horizontal-only weighted interpolation (left → right) |
| PAETH_PRED | 4 | Per-pixel: picks above/left/top-left based on gradient direction |

## Reference Pixel Construction

Matches decoder's `build_intra_predictors` (reconintra.c:1110-1260):

| Boundary | above[] | left[] | top_left |
|----------|---------|--------|----------|
| No above, no left | [127; bsize] | [129; bsize] | 128 |
| Has above, no left | from recon | [above[0]; bsize] | above[0] |
| No above, has left | [left[0]; bsize] | from recon | left[0] |
| Both available | from recon | from recon | recon[y-1][x-1] |

## Prediction Formulas

**SMOOTH**: `pred = (w_h[r]*above[c] + (256-w_h[r])*below + w_w[c]*left[r] + (256-w_w[c])*right + 256) >> 9`

**SMOOTH_V**: `pred = (w_h[r]*above[c] + (256-w_h[r])*below + 128) >> 8`

**SMOOTH_H**: `pred = (w_w[c]*left[r] + (256-w_w[c])*right + 128) >> 8`

**PAETH**: `base = above[c]+left[r]-tl; return nearest of {left[r], above[c], tl} to base`

Where: `below = left[bsize-1]`, `right = above[bsize-1]`, weights from `sm_weight_arrays`.

## Smooth Weight Tables

Scale = 256 (sm_weight_log2_scale = 8).

**Block size 32:**
```
255, 240, 225, 210, 196, 182, 169, 157, 145, 133, 122, 111, 101, 92, 83, 74,
66, 59, 52, 45, 39, 34, 29, 25, 21, 17, 14, 12, 10, 9, 8, 8
```

**Block size 64:**
```
255, 248, 240, 233, 225, 218, 210, 203, 196, 189, 182, 176, 169, 163, 156, 150,
144, 138, 133, 127, 121, 116, 111, 106, 101, 96, 91, 86, 82, 77, 73, 69, 65, 61,
57, 54, 50, 47, 44, 41, 38, 35, 32, 29, 27, 25, 22, 20, 18, 16, 15, 13, 12, 10,
9, 8, 7, 6, 6, 5, 5, 4, 4, 4
```

## Mode Selection

Brute-force SAD: for each of 5 modes, generate full prediction block, compute `sum(|pixel[i] - pred[i]|)`, pick mode with lowest SAD. Ties broken by lowest mode index (DC preferred).

## Tile Encoding

```
ec.encode_symbol(0, &Y_MODE_SET_ICDF);                    // Always set 0
ec.encode_symbol(sb.y_mode as usize, &Y_MODE_IDX_ICDF);   // Mode 0-4
ec.encode_symbol(sb.uv_mode as usize, &UV_MODE_ICDF);     // Mode 0-4
```

Context is always 0 because no angular modes are used, so existing ICDF tables are correct.

## Data Flow

```
get_ref_pixels() → for each mode: predict_block() → SAD → pick best
→ compute_residual(pixels, pred_block) → DCT → quantize → levels/eob
→ dequant → inv DCT → reconstruct(pred_block + inv_residual)
```

`compute_residual_block` and `reconstruct_block` change from single `u8 prediction` to `&[u8] prediction_block` for per-pixel prediction.

## Files Modified

| File | Changes |
|------|---------|
| `src/encode.rs` | IntraMode enum, smooth weight tables, get_ref_pixels, prediction functions, select_intra_mode, updated compute_residual_block/reconstruct_block, updated encode_plane_block_64/32 |
| `src/tile.rs` | y_mode/uv_mode in SbCoeffs, mode encoding from SbCoeffs |

## Testing

- All existing 153 unit + 16 integration tests pass (DC selected for solid-color blocks)
- Unit tests for each prediction function (known reference pixels → expected output)
- Unit tests for mode selection (gradient → SMOOTH, edge → PAETH, flat → DC)
- Integration test: real photo encode → avmdec decode → improved PSNR
