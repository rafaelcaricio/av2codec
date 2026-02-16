# wav2c Decoder Design

## Goal

Pure Rust AV2 decoder inside the wav2c crate, initially targeting bitstreams produced by wav2c (DC prediction, no partition splitting, DCT-2 only, Profile 0 8-bit 4:2:0). Validation via bit-exact comparison against the reference avmdec decoder.

## Architecture: Mirror the Encoder

Each encoder stage has a decoder counterpart. Shared types (CDF tables, scan order, dequant lookup, constants) are reused without duplication.

## Module Layout

```
src/
├── decode.rs              # Decoder struct, public API, orchestrator
├── decode/
│   ├── bitreader.rs       # BitReader (inverse of BitWriter)
│   ├── entropy.rs         # DaalaDecoder (inverse of DaalaCoder)
│   ├── obu.rs             # OBU parser (inverse of OBU packer)
│   ├── sequence.rs        # Sequence header parser
│   ├── frame.rs           # CLK OBU / frame header parser
│   └── tile.rs            # Coefficient decoding (inverse of tile encoding)
├── dct.rs                 # Existing: add/reuse inverse DCT functions
├── cdf_tables.rs          # Shared: all CDF/ICDF tables
├── scan.rs                # Shared: DEFAULT_SCAN_32X32
└── encode.rs              # Shared: ac_qlookup()
```

## Data Flow

```
OBU bytes
  → BitReader::new(bytes)
  → parse LEB128 size + OBU header → identify OBU type
  → Sequence Header OBU: extract width, height, profile, level
  → CLK OBU: parse frame header (qindex, order_hint, tile info)
  → Extract tile data bytes from CLK payload
  → DaalaDecoder::new(tile_bytes)
  → For each superblock in raster order:
      → decode partition (NONE), intra mode (DC_PRED)
      → For Y, U, V planes:
          → decode txb_skip, eob, base levels, BR, sign, HR
          → produce quantized levels[1024] in scan order
  → Dequantize: levels → DCT coefficients
  → Inverse DCT: coefficients → spatial residuals
  → Reconstruct: residuals + 128 → pixels
  → Output YUV planes
```

## Public API

```rust
pub struct Decoder { ... }

pub struct DecodedFrame {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub frame_number: u64,
}

pub enum DecodeError {
    InvalidObu,
    UnsupportedProfile,
    UnsupportedFeature(&'static str),
    BitstreamCorrupt,
    UnexpectedEof,
}

impl Decoder {
    pub fn new() -> Self;
    pub fn decode_obu(&mut self, data: &[u8]) -> Result<Option<DecodedFrame>, DecodeError>;
    pub fn decode_all(data: &[u8]) -> Result<Vec<DecodedFrame>, DecodeError>;
}
```

## Key Components

### BitReader (inverse of BitWriter)
- Reads from byte slice, tracks bit position
- `read_bit()`, `read_bits(n)`, `read_literal(n)`, `read_uvlc()`
- `read_trailing_bits()`, `byte_aligned()`
- `decode_leb128(bytes) -> (value, bytes_consumed)`

### DaalaDecoder (inverse of DaalaCoder)
- State: `rng`, `dif`, `cnt`, byte buffer with read position
- `decode_symbol(icdf: &[u16]) -> usize`
- `decode_literal(n_bits: u32) -> u32`
- `decode_bool(f: u16) -> bool`

### Coefficient Decoding (inverse of tile.rs)
- Same context computation as encoder
- Reuses CDF tables, scan order, LF region detection
- Reads symbols in same order encoder writes them
- DC BR context uses `get_br_lf_ctx_dc_2d` (no +7 offset)

### Inverse DCT
- Already exists in `dct.rs`: `inverse_dct2_32x32`, `inverse_dct2_64x64`
- Reuse directly

### Dequantization
- `dqcoeff = ((abs_level * dequant + 4) >> 3) >> log_scale`
- Reuse `ac_qlookup()` from `encode.rs`

## Validation Strategy

### Bit-exact testing against avmdec
1. Encode with wav2c → `test.obu`
2. Decode with avmdec → `reference.yuv`
3. Decode with wav2c decoder → in-memory YUV
4. Compare pixel-for-pixel (must be identical)

### Test progression
1. Single solid-color frame (DC only)
2. Multi-frame solid colors
3. Gradient (DC + AC)
4. testsrc2 pattern (complex)
5. Arbitrary frame sizes (128x128, 256x192)

### PSNR/VMAF as development guides
- PSNR tracks progress toward bit-exactness (approaches infinity)
- VMAF for perceptual quality on real content
- Computed via ffmpeg lavfi filters

## CLI Binary

```
wav2c-decode input.obu output.y4m
```

Binary at `src/bin/wav2c_decode.rs`:
- Reads OBU file, parses all OBUs
- Decodes each CLK frame
- Writes Y4M output (header + frame data)
- Reports frame count and dimensions
