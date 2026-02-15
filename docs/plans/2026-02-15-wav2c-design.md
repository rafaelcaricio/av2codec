# Wondrous AV2 Coder (wav2c) — Design Document

## Overview

wav2c is a clean-room, pure safe Rust AV2 intra-only video encoder. It produces
spec-compliant bitstreams decodable by the reference AVM decoder (avmdec).

## Goals

- **Clean-room**: No C dependencies, no FFI, no code derived from the reference encoder
- **Safe Rust**: Zero `unsafe` blocks. Correctness and memory safety over performance
- **Spec-compliant**: Output must decode correctly with avmdec (bit-exact reconstruction)
- **Minimal scope**: Intra-only encoding (every frame is a keyframe)
- **Incremental**: Start with single-color frame, grow to full intra encoder

## Target Format

- 8-bit 4:2:0 (YCbCr)
- AV2 Profile 0
- Still picture mode (`single_picture_header_flag = 1`)
- Fixed QP (constant quantizer, no rate control)

## Architecture

```
wav2c/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API
│   ├── encoder.rs          # Top-level encoder state & encode loop
│   ├── frame.rs            # Frame/image types (YUV planes, pixel traits)
│   ├── partition.rs        # Block sizes, partition types, partition search
│   ├── predict/
│   │   ├── mod.rs          # Prediction trait
│   │   └── intra.rs        # All 13 intra prediction modes
│   ├── transform/
│   │   ├── mod.rs          # Transform dispatch
│   │   └── forward.rs      # Forward DCT, ADST (integer arithmetic)
│   ├── quantize.rs         # Quantization & dequantization
│   ├── entropy/
│   │   ├── mod.rs          # Entropy coding public interface
│   │   ├── cdf.rs          # Default CDF tables & CDF management
│   │   ├── writer.rs       # Arithmetic range encoder
│   │   └── context.rs      # Symbol contexts (modes, coefficients, MVs)
│   ├── bitstream/
│   │   ├── mod.rs          # OBU assembly
│   │   ├── obu.rs          # OBU header, LEB128, trailing bits
│   │   ├── sequence.rs     # Sequence header writing
│   │   ├── frame_header.rs # Frame header writing
│   │   └── tile.rs         # Tile group data writing
│   ├── rdo.rs              # Rate-distortion optimization
│   ├── loopfilter.rs       # Deblocking loop filter
│   ├── scan.rs             # Coefficient scan orders
│   └── tables.rs           # Lookup tables
├── tests/
│   └── roundtrip.rs        # Encode → avmdec decode → compare
└── examples/
    └── encode_y4m.rs       # CLI encoder
```

## Encoding Pipeline (per frame)

1. Initialize frame context (CDF tables, QP)
2. For each superblock (128x128) in raster order:
   a. Partition search (recursive RD-based split)
   b. For each leaf block: intra mode search (all 13 modes)
      - Generate prediction from reconstructed neighbors
      - Compute residual = source - prediction
      - Forward transform (DCT/ADST)
      - Quantize coefficients
      - Compute RD cost = distortion + λ × rate
      - Pick best mode
   c. Encode winning mode: inverse quantize → inverse transform → reconstruct
   d. Store mode info for bitstream
3. Deblocking loop filter (on reconstruction buffer)
4. Pack bitstream (OBU sequence header + tile group)

## Bitstream Format

Minimal OBU sequence for each frame:
1. `OBU_SEQUENCE_HEADER` — profile, level, dimensions, tool flags
2. `OBU_REGULAR_TILE_GROUP` — frame header + compressed tile data

Still picture mode (`single_picture_header_flag = 1`) eliminates temporal
features and simplifies the header.

## Disabled Features (signaled off in sequence header)

| Feature | Flag | Value |
|---------|------|-------|
| CDEF | `enable_cdef` | 0 |
| GDF | `enable_gdf` | 0 |
| Loop Restoration | `enable_restoration` | 0 |
| IntraBC | `enable_intrabc` | 0 |
| Palette | `enable_palette` | 0 |
| Warped motion | `enable_warped_motion` | 0 |
| Global motion | `enable_global_motion` | 0 |
| Secondary transforms | `enable_ist` | 0 |
| Angle delta | `enable_angle_delta` | 0 |
| CfL | `enable_cfl` | 0 |
| All inter tools | various | 0 |
| Film grain | n/a | not signaled |

## Enabled Features

- All 13 base intra modes (DC, V, H, D45-D203, Smooth×3, Paeth)
- DCT + ADST forward/inverse transforms
- Uniform quantization with fixed QP
- Deblocking loop filter
- Partition splitting (NONE + SPLIT)
- CDF-based arithmetic entropy coding with default tables
- Single tile

## Entropy Coding

Arithmetic range encoder (Daala-derived) with CDF-based symbol coding.
Default CDF tables initialized per the AV2 spec. For still picture mode,
tile-based CDF averaging is forced.

Key entropy subsystems needed:
- Partition type CDFs
- Intra mode CDFs (Y and UV)
- Transform size/type CDFs
- Coefficient coding (EOB, base level, base range, sign)
- Skip/segment CDFs

## Verification Strategy

1. Build avmdec from the AVM reference repository (decoder-only, with debug logging)
2. Encode test frame with wav2c → output.obu
3. Decode with avmdec → reconstructed output
4. Compare reconstructed pixels against wav2c internal reconstruction
5. Must be bit-exact

Incremental approach:
- Phase 0: Build infrastructure + empty bitstream that avmdec parses
- Phase 1: Single solid-color block → avmdec decodes
- Phase 2: Full solid-color frame → avmdec decodes
- Phase 3: Real content with mode selection
- Phase 4+: Partition search, transforms, loop filter

## Development Phases

### Phase 0: Infrastructure
- Initialize Cargo project
- Build avmdec decoder-only (with optional debug logging)
- Create test frame generation tooling (ffmpeg)
- Implement OBU writer, LEB128, bit writer

### Phase 1: Minimal Bitstream
- Sequence header for still picture, 8-bit 4:2:0
- Frame header (KEY_FRAME, fixed QP)
- Single-tile data with simplest possible content
- Goal: avmdec accepts and decodes the bitstream

### Phase 2: Single-Color Frame
- DC prediction (solid color → zero residual)
- Coefficient coding (all-skip blocks)
- Full frame with uniform color
- Goal: avmdec produces correct solid-color output

### Phase 3: Real Intra Prediction
- All 13 intra modes
- Forward DCT transform + quantization
- Coefficient entropy coding (non-zero coefficients)
- Mode selection via RD cost
- Goal: encode arbitrary intra content

### Phase 4: Quality Improvements
- ADST transforms
- Partition search (SPLIT)
- Deblocking loop filter
- Goal: reasonable compression quality

## Development Rules

- No code comments — self-documenting code through clear naming
- No `unsafe` Rust
- All tests pass before committing (cargo test + cargo clippy --tests)
- Every phase adds regression tests protecting previous work
- End-to-end validation: wav2c encode → avmdec decode
- Test frames generated with FFmpeg
- Debug logging in avmdec allowed; behavioral changes forbidden
