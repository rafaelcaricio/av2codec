# wav2c — Wondrous AV2 Coder

A clean-room, pure safe Rust AV2 (AVM) intra-only video encoder.

wav2c produces spec-compliant AV2 bitstreams decodable by the reference
[avmdec](https://gitlab.com/AOMediaCodec/avm) decoder. It targets intra-only
encoding with 8-bit 4:2:0 YUV, fixed QP, and no dependencies on unsafe code or
C libraries.

## Status

Phase 1 is complete. The encoder produces valid multi-frame all-intra AV2
bitstreams with DC coefficient coding. Solid-color frames encode and decode
with exact pixel-level accuracy across all three YUV planes.

Current capabilities:

- Video-mode sequence header (profile 0, 8-bit, 4:2:0)
- Frame header (CLK OBU, implicit KEY_FRAME, order_hint)
- Daala arithmetic range coder (entropy coding)
- DC coefficient coding (non-zero DC per plane, quantization)
- Y4M input file parsing (I420)
- Multi-frame encoding (each frame as separate CLK OBU)
- OBU packaging with LEB128 framing
- CLI binary for Y4M → .obu encoding

## Usage

```
cargo run -- input.y4m output.obu [--qindex 50]
```

Arguments:
- `input.y4m` — Input Y4M file (8-bit 4:2:0)
- `output.obu` — Output AV2 bitstream
- `--qindex N` — Base quantizer index (0-255, default 50)

Decode with the reference decoder:

```
avmdec --i420 -o decoded.yuv output.obu
```

## Building

```
cargo build
```

## Testing

```
cargo test
```

There are 60 unit tests covering the bitwriter, OBU framing, sequence/frame
headers, arithmetic coder, tile encoding, quantization, Y4M parsing, and
bitstream assembly.

Integration tests (require avmdec at `../avm_build/avmdec`):

```
cargo test --test avmdec_integration -- --ignored
```

7 integration tests verify encode → decode → pixel value correctness for:
- Single-frame mid-gray (Y=128)
- Multi-frame mid-gray (3 frames)
- Solid green with DC coefficients (Y=149, U=43, V=21)
- Solid red with DC coefficients (Y=81, U=90, V=240)
- Multi-frame multi-color (green, red, gray)
- CLI encoding from Y4M fixtures (single and multi-frame)

## Architecture

```
src/
  lib.rs                  Crate root
  encode.rs               Top-level: assembles OBUs, quantization math
  tile.rs                 Tile data: partition, mode, DC coefficient coding
  entropy.rs              Daala arithmetic range coder (od_ec_enc port)
  y4m.rs                  Y4M file parser (I420 frames)
  bin/
    wav2c.rs              CLI binary: Y4M input → .obu output
  bitstream/
    writer.rs             Bit-level writer (literals, UVLC, LEB128, trailing bits)
    obu.rs                OBU header + LEB128 size framing
    sequence.rs           Sequence header payload (video mode)
    frame.rs              Frame header (CLK OBU) payload
```

## Phase Roadmap

- Phase 0: Infrastructure — **Complete**
- Phase 1: Multi-frame all-intra encoding with DC coefficients — **Complete**
- Phase 2: AC coefficient support (planned)

## License

TBD
