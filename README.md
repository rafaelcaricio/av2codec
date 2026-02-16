# wav2c

**Wondrous AV2 Coder** — a clean-room AV2 video encoder and decoder written in pure safe Rust.

wav2c encodes raw YUV video into [AV2 (AOM Video 2)](https://aomedia.org/) bitstreams and decodes them back. Encoded bitstreams are compatible with the reference [avmdec](https://gitlab.com/AOMediaCodec/avm) decoder. No unsafe code, no C dependencies, no assembly — just Rust.

## Quick start

```sh
# Build
cargo build --release

# Encode a Y4M file to AV2
cargo run --release --bin wav2c -- input.y4m output.obu

# Decode back to Y4M (using the built-in decoder)
cargo run --release --bin wav2c_decode -- output.obu -o decoded.y4m

# Or decode with the reference decoder
avmdec --i420 -o decoded.yuv output.obu
```

## Usage

### CLI

#### Encoding

```
wav2c <input.y4m> <output.obu> [--qindex N]
```

| Argument | Description |
|---|---|
| `input.y4m` | Input video in Y4M format (8-bit 4:2:0 I420) |
| `output.obu` | Output AV2 bitstream (OBU format) |
| `--qindex N` | Base quantizer index, 0-255 (default: 50) |

#### Decoding

```
wav2c_decode <input.obu> [-o <output.y4m>]
```

| Argument | Description |
|---|---|
| `input.obu` | Input AV2 bitstream |
| `-o output.y4m` | Output Y4M file (default: stdout) |

### Library API

#### Encoding (streaming)

```rust
use wav2c::Encoder;

let mut enc = Encoder::new(640, 480, 50)?;
let mut output = enc.headers();

for frame in frames {
    enc.send_frame(&frame.y, &frame.u, &frame.v)?;
    let pkt = enc.receive_packet().unwrap();
    output.extend_from_slice(&pkt.data);
}

enc.flush();
std::fs::write("output.obu", &output)?;
```

#### Decoding

```rust
use wav2c::{Decoder, DecodedFrame, DecodeError};

let bitstream = std::fs::read("input.obu")?;
let frames: Vec<DecodedFrame> = Decoder::decode_all(&bitstream)?;

for frame in &frames {
    println!(
        "Frame {}: {}x{}, Y={} bytes, U={} bytes, V={} bytes",
        frame.frame_number,
        frame.width,
        frame.height,
        frame.y.len(),
        frame.u.len(),
        frame.v.len(),
    );

    // Access raw pixel planes directly
    let y_pixel = frame.y[0];      // top-left luma pixel
    let u_pixel = frame.u[0];      // top-left Cb pixel (half resolution)
    let v_pixel = frame.v[0];      // top-left Cr pixel (half resolution)
}
```

The `DecodedFrame` struct provides direct access to the decoded YUV 4:2:0 planes:

| Field | Type | Description |
|---|---|---|
| `y` | `Vec<u8>` | Luma plane (width x height) |
| `u` | `Vec<u8>` | Cb chroma plane (width/2 x height/2) |
| `v` | `Vec<u8>` | Cr chroma plane (width/2 x height/2) |
| `width` | `u32` | Frame width in pixels |
| `height` | `u32` | Frame height in pixels |
| `frame_number` | `u64` | Sequential frame index (0-based) |

#### Encode-decode roundtrip

```rust
use wav2c::{Encoder, Decoder};

// Encode
let mut enc = Encoder::new(128, 128, 50).unwrap();
let mut bitstream = enc.headers();

let y = vec![149u8; 128 * 128];
let u = vec![43u8; 64 * 64];
let v = vec![21u8; 64 * 64];
enc.send_frame(&y, &u, &v).unwrap();
bitstream.extend_from_slice(&enc.receive_packet().unwrap().data);

// Decode
let frames = Decoder::decode_all(&bitstream).unwrap();
assert_eq!(frames.len(), 1);
assert_eq!(frames[0].width, 128);
assert_eq!(frames[0].height, 128);
```

See `examples/encode.rs` for a complete working example that reads raw I420 frames from stdin:

```sh
ffmpeg -f lavfi -i testsrc2=size=64x64:duration=1:rate=25 \
  -pix_fmt yuv420p -f rawvideo - | cargo run --example encode -- output.obu

# Decode with the built-in decoder
cargo run --release --bin wav2c_decode -- output.obu -o decoded.y4m

# Or decode with the reference decoder
avmdec --i420 -o decoded.yuv output.obu
```

## What it does

wav2c reads Y4M video frames and produces an all-intra AV2 bitstream where every frame is an independent keyframe. It also includes a pure Rust decoder that can reconstruct YUV frames from wav2c-produced bitstreams.

The encoding pipeline for each frame:

1. Read YUV 4:2:0 pixel data (Y, U, V planes)
2. Compute residual against mid-gray prediction (128)
3. Forward 2D DCT (64x64 for Y, 32x32 for U/V)
4. Quantization using AV2 dequant tables
5. Entropy-code coefficients using Daala arithmetic coding (base levels, BR, sign, adaptive HR)
6. Pack into a CLK OBU (implicit keyframe)

The decoding pipeline reverses these steps:

1. Parse OBU stream (sequence header, CLK frames)
2. Decode entropy-coded coefficients using Daala arithmetic range decoder
3. Dequantize coefficient levels
4. Inverse 2D DCT to reconstruct residuals
5. Add mid-gray prediction (128) and clamp to [0, 255]

A sequence header OBU is prepended once at the start of the bitstream.

## Current capabilities

- Profile 0, 8-bit, 4:2:0 (YCbCr)
- All-intra encoding (every frame is a keyframe)
- Full DC + AC coefficient coding with forward/inverse DCT
- Multi-frame video encoding (each frame as a CLK OBU)
- **Pure Rust decoder** — decodes wav2c bitstreams without any external dependencies
- Streaming encoder API (`send_frame` / `receive_packet`)
- Batch encoding API (`encode_video_yuv`) and decoding API (`Decoder::decode_all`)
- Y4M input parsing and output writing
- Daala-derived arithmetic range coder/decoder for entropy coding
- OBU bitstream packaging with LEB128 size framing

## Limitations

- Dimensions must be multiples of 64 (superblock aligned)
- Fixed QP only (no rate control)
- 8-bit 4:2:0 only
- No deblocking filters (CDEF/GDF) — expect visible block boundaries at higher quantization

## Testing

Unit tests (200 tests across all modules):

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
  dct.rs              Forward and inverse 2D DCT (32x32, 64x64)
  tile.rs             Tile data: partition, intra mode, coefficient coding
  entropy.rs          Daala arithmetic range coder (od_ec_enc port)
  scan.rs             Coefficient scan order tables
  cdf_tables.rs       Initial CDF tables for entropy coding
  y4m.rs              Y4M file parser
  bin/
    wav2c.rs          Encoder CLI binary
    wav2c_decode.rs   Decoder CLI binary
  decode/
    mod.rs            Decoder orchestrator, frame reconstruction
    bitreader.rs      Bit-level reader (MSB-first, UVLC, LEB128)
    entropy.rs        Daala arithmetic range decoder
    obu.rs            OBU stream parser
    sequence.rs       Sequence header decoder
    frame.rs          Frame header decoder
    tile.rs           Tile coefficient decoder (base, BR, sign, HR)
    transform.rs      Dequantization and inverse DCT
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
| 3 | Pure Rust decoder | Done |

## License

TBD
