# wav2c — Wondrous AV2 Coder

A clean-room, pure safe Rust AV2 (AVM) intra-only video encoder.

wav2c produces spec-compliant AV2 bitstreams decodable by the reference
[avmdec](https://gitlab.com/AOMediaCodec/avm) decoder. It targets still
picture mode (`single_picture_header_flag=1`) with 8-bit 4:2:0 YUV, fixed QP,
and no dependencies on unsafe code or C libraries.

## Status

Phase 0 is complete. The encoder produces a valid 19-byte bitstream for a
64x64 DC-predicted frame with zero coefficients. The output has been verified
against the reference AVM decoder.

Current capabilities:

- Sequence header (profile 0, 8-bit, 4:2:0)
- Frame header (CLK OBU, still picture mode)
- Daala arithmetic range coder (entropy coding)
- Tile encoding (partition, intra modes, coefficient skip)
- OBU packaging with LEB128 framing

## Building

```
cargo build
```

## Testing

```
cargo test
```

There are 45 unit tests covering the bitwriter, OBU framing, sequence/frame
headers, arithmetic coder, tile encoding, and bitstream assembly.

To validate against the reference decoder:

```
cargo test dump_bitstream -- --nocapture  # writes /tmp/wav2c_test.obu
avmdec --summary -o /tmp/decoded.y4m /tmp/wav2c_test.obu
```

## Architecture

```
src/
  lib.rs                  Crate root
  encode.rs               Top-level: assembles OBUs into a complete bitstream
  tile.rs                 Tile data: partition, mode, and coefficient symbols
  entropy.rs              Daala arithmetic range coder (od_ec_enc port)
  bitstream/
    writer.rs             Bit-level writer (literals, UVLC, LEB128, trailing bits)
    obu.rs                OBU header + LEB128 size framing
    sequence.rs           Sequence header payload
    frame.rs              Frame header (CLK OBU) payload
```

## License

TBD
