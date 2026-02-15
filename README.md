# wav2c

**Wondrous AV2 Coder** — a clean-room AV2 video encoder written in pure safe Rust.

wav2c encodes raw YUV video into [AV2 (AOM Video 2)](https://aomedia.org/) bitstreams that decode correctly with the reference [avmdec](https://gitlab.com/AOMediaCodec/avm) decoder. No unsafe code, no C dependencies, no assembly — just Rust.

## Quick start

```sh
# Build
cargo build --release

# Encode a Y4M file to AV2
cargo run --release -- input.y4m output.obu

# Decode with the reference decoder
avmdec --i420 -o decoded.yuv output.obu
```

## Usage

### CLI

```
wav2c <input.y4m> <output.obu> [--qindex N]
```

| Argument | Description |
|---|---|
| `input.y4m` | Input video in Y4M format (8-bit 4:2:0 I420) |
| `output.obu` | Output AV2 bitstream (OBU format) |
| `--qindex N` | Base quantizer index, 0-255 (default: 50) |

### Library (streaming API)

```rust
use wav2c::Encoder;

let mut enc = Encoder::new(64, 64, 50)?;
let mut output = enc.headers();

for frame in frames {
    enc.send_frame(&frame.y, &frame.u, &frame.v)?;
    let pkt = enc.receive_packet().unwrap();
    output.extend_from_slice(&pkt.data);
}

enc.flush();
std::fs::write("output.obu", &output)?;
```

See `examples/encode.rs` for a complete working example that reads raw I420 frames from stdin:

```sh
ffmpeg -f lavfi -i testsrc2=size=64x64:duration=1:rate=25 \
  -pix_fmt yuv420p -f rawvideo - | cargo run --example encode -- output.obu

# Decode with the reference decoder
avmdec --i420 -o decoded.yuv output.obu
```

## What it does

wav2c reads Y4M video frames and produces an all-intra AV2 bitstream where every frame is an independent keyframe. Each frame uses DC prediction with forward DCT, quantization, and full entropy coding of both DC and AC coefficients.

The encoding pipeline for each frame:

1. Read YUV 4:2:0 pixel data (Y, U, V planes)
2. Compute residual against mid-gray prediction (128)
3. Forward 2D DCT (64x64 for Y, 32x32 for U/V)
4. Quantization using AV2 dequant tables
5. Entropy-code coefficients using Daala arithmetic coding (base levels, BR, sign, adaptive HR)
6. Pack into a CLK OBU (implicit keyframe)

A sequence header OBU is prepended once at the start of the bitstream.

## Current capabilities

- Profile 0, 8-bit, 4:2:0 (YCbCr)
- All-intra encoding (every frame is a keyframe)
- Full DC + AC coefficient coding with forward DCT
- Multi-frame video encoding (each frame as a CLK OBU)
- Streaming encoder API (`send_frame` / `receive_packet`)
- Batch encoding API (`encode_video_yuv`)
- Y4M input parsing
- Daala-derived arithmetic range coder for entropy coding
- OBU bitstream packaging with LEB128 size framing

## Limitations

- Single 64x64 superblock (no tiling, no spatial partitioning beyond the first block)
- Fixed QP only (no rate control)
- 8-bit 4:2:0 only

## Testing

Unit tests (105 tests across all modules):

```sh
cargo test
```

Integration tests that encode video, decode with avmdec, and verify pixel values:

```sh
# Requires avmdec built at ../avm_build/avmdec
cargo test --test avmdec_integration -- --ignored
```

The integration suite covers single-frame, multi-frame, multiple colors (green, red, gray), all three YUV planes, gradient/testsrc2 patterns with PSNR validation, and CLI roundtrip encoding.

## Project structure

```
src/
  lib.rs              Crate root, module declarations, re-exports
  encode.rs           Streaming Encoder, batch API, quantization math
  dct.rs              Forward 2D DCT (32x32, 64x64)
  tile.rs             Tile data: partition, intra mode, coefficient coding
  entropy.rs          Daala arithmetic range coder (od_ec_enc port)
  scan.rs             Coefficient scan order tables
  cdf_tables.rs       Initial CDF tables for entropy coding
  y4m.rs              Y4M file parser
  bin/
    wav2c.rs          CLI binary
  bitstream/
    mod.rs            Bitstream module re-exports
    writer.rs         Bit-level writer (literals, UVLC, LEB128)
    obu.rs            OBU header and LEB128 size framing
    sequence.rs       Sequence header payload
    frame.rs          Frame header (CLK OBU) payload
examples/
  encode.rs           Streaming API example (solid-color frames)
tests/
  avmdec_integration.rs   Encode-decode roundtrip tests
  fixtures/               Y4M test inputs (solid colors, gradients, patterns)
docs/
  plans/                  Design documents and implementation plans
```

## Building the reference decoder

wav2c validates against the AVM reference decoder. To build it:

```sh
git clone https://gitlab.com/AoMediaCodec/avm.git ../av2
mkdir -p ../avm_build && cd ../avm_build
cmake ../av2 -DCONFIG_AV2_ENCODER=0
make -j$(nproc) avmdec
```

## Roadmap

| Phase | Description | Status |
|---|---|---|
| 0 | Infrastructure (bitwriter, OBU, headers, entropy coder) | Done |
| 1 | Multi-frame all-intra with DC coefficients | Done |
| 2 | AC coefficient support | Done |

## License

TBD
