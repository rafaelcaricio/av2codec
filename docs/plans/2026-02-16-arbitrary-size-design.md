# Arbitrary-Size Encoding via Pad-to-64

## Problem

wav2c requires frame dimensions to be exact multiples of 64 (the superblock size). This prevents encoding standard resolutions like 1920x1080, 640x480, or any photo with non-aligned dimensions.

## Approach

Accept any dimensions that are multiples of 4 (the AV2 MI unit size). Internally pad input planes to the next multiple of 64 with midgray (128) and use padded dimensions in both the SB grid and the sequence/frame headers. The decoder outputs the padded frame; external tools crop to the original size.

This avoids implementing the AV2 partition tree for boundary superblocks, which the decoder would require if we wrote the real (non-aligned) dimensions in the header.

## Design

### Dimension handling

```
original_w, original_h  = user-provided (multiple of 4)
padded_w  = round_up_to_64(original_w)
padded_h  = round_up_to_64(original_h)
```

The `Encoder` struct stores both original and padded dimensions. The API (`width()`, `height()`) returns original dimensions. All internal encoding uses padded dimensions.

### Plane padding

A helper `pad_plane(src, orig_w, orig_h, padded_w, padded_h) -> Vec<u8>` copies original pixels row by row into a padded buffer. Pixels beyond the original boundary are filled with 128 (midgray). For 4:2:0 chroma planes, dimensions are halved.

### Encoding pipeline

`encode_frame_yuv` receives already-padded planes and padded dimensions. The SB grid, reconstruction loop, DCT, quantization, and entropy coding are unchanged — they always operate on full 64x64 (Y) / 32x32 (UV) blocks.

### Header dimensions

The sequence header and frame header use padded dimensions. This ensures the decoder creates a full SB grid with no boundary partitions needed.

### What stays the same

- Forward/inverse DCT (all sizes)
- Quantization and dequantization
- Entropy coding and CDF tables
- Tile encoding
- Reconstruction loop

## Limitations

- Output frame is padded (e.g., 1920x1080 input produces 1920x1088 output)
- Consumer must crop to original dimensions
- Wastes bits encoding up to 63 rows/columns of padding
- Future work: implement partition tree for boundary SBs to encode real dimensions

## Test strategy

- All existing tests pass unchanged
- New unit tests for dimension validation (accepts multiples of 4, rejects others)
- New unit test for pad_plane correctness
- Integration test: encode non-aligned frame, decode with avmdec, verify visible-region pixels
