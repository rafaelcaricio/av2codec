# wav2c Decoder Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Pure Rust AV2 decoder that produces bit-exact output matching avmdec for wav2c-encoded bitstreams.

**Architecture:** Mirror-the-encoder approach. Each encoder stage gets a decoder counterpart. Shared types (CDF tables, scan order, dequant lookup) are reused. New modules: `BitReader`, `DaalaDecoder`, OBU/sequence/frame parsers, tile coefficient decoder, inverse DCT, and `Decoder` orchestrator.

**Tech Stack:** Pure Rust (no dependencies), same crate as encoder.

---

### Task 1: BitReader — bit-level reading from byte slices

**Files:**
- Create: `src/decode/bitreader.rs`
- Modify: `src/lib.rs` (add `pub mod decode;`)
- Create: `src/decode/mod.rs` (module declarations)

**Step 1: Write the failing test**

In `src/decode/bitreader.rs`, add tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::writer::{BitWriter, encode_leb128};

    #[test]
    fn read_single_bits() {
        let mut w = BitWriter::new();
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bit(), true);
        assert_eq!(r.read_bit(), false);
        assert_eq!(r.read_bit(), true);
    }

    #[test]
    fn read_bits_multi() {
        let mut w = BitWriter::new();
        w.write_bits(0b11010, 5);
        w.write_bits(0xFF, 8);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bits(5), 0b11010);
        assert_eq!(r.read_bits(8), 0xFF);
    }

    #[test]
    fn read_literal() {
        let mut w = BitWriter::new();
        w.write_literal(42, 8);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_literal(8), 42);
    }

    #[test]
    fn read_uvlc_values() {
        for val in [0u32, 1, 2, 3, 7, 15, 100] {
            let mut w = BitWriter::new();
            w.write_uvlc(val);
            let data = w.finalize();
            let mut r = BitReader::new(&data);
            assert_eq!(r.read_uvlc(), val, "UVLC roundtrip failed for {}", val);
        }
    }

    #[test]
    fn decode_leb128_values() {
        for val in [0u32, 1, 127, 128, 300, 16384] {
            let encoded = encode_leb128(val);
            let (decoded, consumed) = decode_leb128(&encoded);
            assert_eq!(decoded, val);
            assert_eq!(consumed, encoded.len());
        }
    }

    #[test]
    fn read_trailing_bits() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3);
        w.write_trailing_bits();
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        r.read_bits(3);
        r.read_trailing_bits();
        assert!(r.byte_aligned());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode::bitreader -- -v`
Expected: FAIL — module does not exist

**Step 3: Write minimal implementation**

Create `src/decode/mod.rs`:

```rust
pub mod bitreader;
```

Create `src/decode/bitreader.rs`:

```rust
pub struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    pub fn read_bit(&mut self) -> bool {
        let byte = self.data[self.byte_pos];
        let bit = (byte >> (7 - self.bit_pos)) & 1 == 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        bit
    }

    pub fn read_bits(&mut self, num_bits: u8) -> u64 {
        let mut value = 0u64;
        for _ in 0..num_bits {
            value = (value << 1) | (self.read_bit() as u64);
        }
        value
    }

    pub fn read_literal(&mut self, num_bits: u8) -> u32 {
        self.read_bits(num_bits) as u32
    }

    pub fn read_uvlc(&mut self) -> u32 {
        let mut leading_zeros = 0u8;
        while !self.read_bit() {
            leading_zeros += 1;
        }
        if leading_zeros == 0 {
            return 0;
        }
        let rest = self.read_bits(leading_zeros);
        ((1u64 << leading_zeros) | rest) as u32 - 1
    }

    pub fn read_trailing_bits(&mut self) {
        self.read_bit();
        while self.bit_pos != 0 {
            self.read_bit();
        }
    }

    pub fn byte_aligned(&self) -> bool {
        self.bit_pos == 0
    }

    pub fn position(&self) -> usize {
        self.byte_pos
    }

    pub fn remaining_bytes(&self) -> &'a [u8] {
        if self.byte_aligned() {
            &self.data[self.byte_pos..]
        } else {
            &self.data[self.byte_pos + 1..]
        }
    }

    pub fn skip_to_byte_boundary(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }
}

pub fn decode_leb128(data: &[u8]) -> (u32, usize) {
    let mut value = 0u32;
    let mut shift = 0;
    let mut i = 0;
    loop {
        let byte = data[i];
        value |= ((byte & 0x7F) as u32) << shift;
        i += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    (value, i)
}
```

Add to `src/lib.rs`:

```rust
pub mod decode;
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode::bitreader`
Expected: PASS (all 6 tests)

**Step 5: Commit**

```bash
git add src/decode/mod.rs src/decode/bitreader.rs src/lib.rs
git commit -m "feat(decode): add BitReader for bit-level parsing"
```

---

### Task 2: OBU parser — split bitstream into OBU units

**Files:**
- Create: `src/decode/obu.rs`
- Modify: `src/decode/mod.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::obu::{ObuType, ObuWriter};

    #[test]
    fn parse_single_obu() {
        let payload = vec![0xAA, 0xBB];
        let packed = ObuWriter::pack_obu(ObuType::SequenceHeader, &payload);
        let obus = parse_obus(&packed);
        assert_eq!(obus.len(), 1);
        assert_eq!(obus[0].obu_type, 1);
        assert_eq!(obus[0].payload, &[0xAA, 0xBB]);
    }

    #[test]
    fn parse_multiple_obus() {
        let mut data = Vec::new();
        data.extend_from_slice(&ObuWriter::pack_obu(ObuType::SequenceHeader, &[0x01]));
        data.extend_from_slice(&ObuWriter::pack_obu(ObuType::Clk, &[0x02, 0x03]));
        let obus = parse_obus(&data);
        assert_eq!(obus.len(), 2);
        assert_eq!(obus[0].obu_type, 1);
        assert_eq!(obus[1].obu_type, 4);
        assert_eq!(obus[1].payload, &[0x02, 0x03]);
    }

    #[test]
    fn parse_header_byte() {
        let (obu_type, ext) = decode_header_byte(0b0_00100_00);
        assert_eq!(obu_type, 1);
        assert_eq!(ext, false);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode::obu`

**Step 3: Write minimal implementation**

```rust
use super::bitreader::decode_leb128;

pub struct RawObu<'a> {
    pub obu_type: u8,
    pub payload: &'a [u8],
}

pub fn decode_header_byte(byte: u8) -> (u8, bool) {
    let ext = (byte >> 7) & 1 == 1;
    let obu_type = (byte >> 2) & 0x1F;
    (obu_type, ext)
}

pub fn parse_obus(data: &[u8]) -> Vec<RawObu<'_>> {
    let mut obus = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let (obu_size, leb_len) = decode_leb128(&data[pos..]);
        pos += leb_len;
        let header_byte = data[pos];
        let (obu_type, _ext) = decode_header_byte(header_byte);
        let payload_size = obu_size as usize - 1;
        let payload = &data[pos + 1..pos + 1 + payload_size];
        obus.push(RawObu { obu_type, payload });
        pos += obu_size as usize;
    }
    obus
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode::obu`

**Step 5: Commit**

```bash
git add src/decode/obu.rs src/decode/mod.rs
git commit -m "feat(decode): add OBU parser"
```

---

### Task 3: DaalaDecoder — arithmetic range decoder

**Files:**
- Create: `src/decode/entropy.rs`
- Modify: `src/decode/mod.rs`

This is the most critical component. The Daala range decoder mirrors the encoder using a 64-bit window (`dif`), 16-bit range (`rng`), bit counter (`cnt`), and byte read pointer.

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::DaalaCoder;

    #[test]
    fn roundtrip_bool_false() {
        let mut enc = DaalaCoder::new();
        enc.encode_bool(false, 16384);
        let data = enc.done();
        let mut dec = DaalaDecoder::new(&data);
        assert_eq!(dec.decode_bool(16384), false);
    }

    #[test]
    fn roundtrip_bool_true() {
        let mut enc = DaalaCoder::new();
        enc.encode_bool(true, 16384);
        let data = enc.done();
        let mut dec = DaalaDecoder::new(&data);
        assert_eq!(dec.decode_bool(16384), true);
    }

    #[test]
    fn roundtrip_symbol() {
        let icdf = [12276u16, 0];
        let mut enc = DaalaCoder::new();
        enc.encode_symbol(0, &icdf);
        let data = enc.done();
        let mut dec = DaalaDecoder::new(&data);
        assert_eq!(dec.decode_symbol(&icdf), 0);
    }

    #[test]
    fn roundtrip_symbol_second() {
        let icdf = [12276u16, 0];
        let mut enc = DaalaCoder::new();
        enc.encode_symbol(1, &icdf);
        let data = enc.done();
        let mut dec = DaalaDecoder::new(&data);
        assert_eq!(dec.decode_symbol(&icdf), 1);
    }

    #[test]
    fn roundtrip_literal() {
        let mut enc = DaalaCoder::new();
        enc.encode_literal(42, 8);
        let data = enc.done();
        let mut dec = DaalaDecoder::new(&data);
        assert_eq!(dec.decode_literal(8), 42);
    }

    #[test]
    fn roundtrip_multiple_symbols() {
        let icdf2 = [12276u16, 0];
        let icdf4 = [3905u16, 1746, 1044, 0];
        let icdf8 = [17593u16, 12693, 11040, 8670, 6363, 5113, 3908, 0];

        let mut enc = DaalaCoder::new();
        enc.encode_symbol(0, &icdf2);
        enc.encode_symbol(1, &icdf2);
        enc.encode_symbol(0, &icdf4);
        enc.encode_symbol(3, &icdf8);
        enc.encode_literal(7, 3);
        enc.encode_bool(true, 8000);
        let data = enc.done();

        let mut dec = DaalaDecoder::new(&data);
        assert_eq!(dec.decode_symbol(&icdf2), 0);
        assert_eq!(dec.decode_symbol(&icdf2), 1);
        assert_eq!(dec.decode_symbol(&icdf4), 0);
        assert_eq!(dec.decode_symbol(&icdf8), 3);
        assert_eq!(dec.decode_literal(3), 7);
        assert_eq!(dec.decode_bool(8000), true);
    }

    #[test]
    fn roundtrip_tile_data_symbols() {
        let do_split = [12276u16, 0];
        let y_mode_set = [3905u16, 1746, 1044, 0];
        let y_mode_idx = [17593u16, 12693, 11040, 8670, 6363, 5113, 3908, 0];
        let uv_mode = [23405u16, 11811, 9903, 8015, 6357, 4785, 2340, 0];
        let txb_skip = [29968u16, 0];

        let mut enc = DaalaCoder::new();
        enc.encode_symbol(0, &do_split);
        enc.encode_symbol(0, &y_mode_set);
        enc.encode_symbol(0, &y_mode_idx);
        enc.encode_symbol(0, &uv_mode);
        enc.encode_symbol(1, &txb_skip);
        enc.encode_symbol(1, &[31468, 0]);
        enc.encode_symbol(1, &[31329, 0]);
        let data = enc.done();

        let mut dec = DaalaDecoder::new(&data);
        assert_eq!(dec.decode_symbol(&do_split), 0);
        assert_eq!(dec.decode_symbol(&y_mode_set), 0);
        assert_eq!(dec.decode_symbol(&y_mode_idx), 0);
        assert_eq!(dec.decode_symbol(&uv_mode), 0);
        assert_eq!(dec.decode_symbol(&txb_skip), 1);
        assert_eq!(dec.decode_symbol(&[31468, 0]), 1);
        assert_eq!(dec.decode_symbol(&[31329, 0]), 1);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode::entropy`

**Step 3: Write minimal implementation**

The decoder mirrors the encoder's arithmetic, following the AV2 reference `od_ec_dec` implementation:

```rust
const CDF_PROB_TOP: u32 = 32768;
const EC_PROB_SHIFT: u32 = 7;
const OD_EC_WINDOW_SIZE: u32 = 64;

#[rustfmt::skip]
static PROB_INC_TBL: [[u16; 16]; 15] = [
    // same table as encoder — import or duplicate
];

fn ilog_nz(x: u32) -> u32 {
    debug_assert!(x > 0);
    32 - x.leading_zeros()
}

fn prob_scale(p: u16, r: u32, n: usize, nsym: usize) -> u32 {
    let rr = r >> 8;
    let mut pp = (u32::from(p) >> EC_PROB_SHIFT) << 4;
    pp += u32::from(PROB_INC_TBL[nsym - 2][n]);
    ((rr * pp) >> 7) << 3
}

pub struct DaalaDecoder<'a> {
    buf: &'a [u8],
    bptr: usize,
    dif: u64,
    rng: u32,
    cnt: i32,
}

impl<'a> DaalaDecoder<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        let mut dec = Self {
            buf,
            bptr: 0,
            dif: (1u64 << (OD_EC_WINDOW_SIZE - 1)) - 1,
            rng: 0x8000,
            cnt: -15,
        };
        dec.refill();
        dec
    }

    fn refill(&mut self) {
        let mut s = (OD_EC_WINDOW_SIZE as i32) - 9 - (self.cnt + 15);
        while s >= 0 && self.bptr < self.buf.len() {
            self.dif ^= (self.buf[self.bptr] as u64) << s as u64;
            self.cnt += 8;
            self.bptr += 1;
            s -= 8;
        }
        if self.bptr >= self.buf.len() {
            self.cnt = 0x4000;
        }
    }

    fn normalize(&mut self, mut dif: u64, rng: u32, n_bypass: i32) {
        let d = if n_bypass > 0 {
            self.cnt -= n_bypass;
            self.dif = ((dif.wrapping_add(1)) << n_bypass as u64).wrapping_sub(1);
            if self.cnt < 8 {
                self.refill();
            }
            return;
        } else {
            16 - ilog_nz(rng) as i32
        };
        self.cnt -= d;
        self.dif = ((dif.wrapping_add(1)) << d as u64).wrapping_sub(1);
        self.rng = rng << d as u32;
        if self.cnt < 8 {
            self.refill();
        }
    }

    pub fn decode_bool(&mut self, f: u16) -> bool {
        let dif = self.dif;
        let r = self.rng;
        let v = prob_scale(f, r, 0, 2);
        let vw = (v as u64) << (OD_EC_WINDOW_SIZE - 16);
        if dif >= vw {
            self.normalize(dif - vw, r - v, 0);
            false
        } else {
            self.normalize(dif, v, 0);
            true
        }
    }

    pub fn decode_symbol(&mut self, icdf: &[u16]) -> usize {
        let dif = self.dif;
        let r = self.rng;
        let nsyms = icdf.len();
        let c = (dif >> (OD_EC_WINDOW_SIZE - 16)) as u32;
        let mut v = r;
        let mut ret = 0usize;
        let mut u;
        loop {
            u = v;
            v = prob_scale(icdf[ret], r, ret, nsyms);
            if c >= v {
                break;
            }
            ret += 1;
        }
        let new_dif = dif - ((v as u64) << (OD_EC_WINDOW_SIZE - 16));
        self.normalize(new_dif, u - v, 0);
        ret
    }

    pub fn decode_literal(&mut self, n_bits: u32) -> u32 {
        let mut dif = self.dif;
        let r = self.rng;
        let mut vw = (r as u64) << (OD_EC_WINDOW_SIZE - 16);
        let mut ret = 0u32;
        for _ in 0..n_bits {
            vw >>= 1;
            ret <<= 1;
            if dif >= vw {
                dif -= vw;
            } else {
                ret |= 1;
            }
        }
        self.normalize(dif, 0, n_bits as i32);
        ret
    }
}
```

**Note:** The PROB_INC_TBL is shared with the encoder. Either import from `crate::entropy` (requires making it `pub`) or copy. The implementation agent should extract it to a shared location or make the encoder's version public.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode::entropy`

**Step 5: Commit**

```bash
git add src/decode/entropy.rs src/decode/mod.rs
git commit -m "feat(decode): add Daala arithmetic range decoder"
```

---

### Task 4: Sequence header parser

**Files:**
- Create: `src/decode/sequence.rs`
- Modify: `src/decode/mod.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::{SequenceHeaderConfig, write_sequence_header};

    #[test]
    fn roundtrip_64x64() {
        let config = SequenceHeaderConfig { width: 64, height: 64 };
        let payload = write_sequence_header(&config);
        let parsed = parse_sequence_header(&payload).unwrap();
        assert_eq!(parsed.width, 64);
        assert_eq!(parsed.height, 64);
    }

    #[test]
    fn roundtrip_128x128() {
        let config = SequenceHeaderConfig { width: 128, height: 128 };
        let payload = write_sequence_header(&config);
        let parsed = parse_sequence_header(&payload).unwrap();
        assert_eq!(parsed.width, 128);
        assert_eq!(parsed.height, 128);
    }

    #[test]
    fn roundtrip_256x192() {
        let config = SequenceHeaderConfig { width: 256, height: 192 };
        let payload = write_sequence_header(&config);
        let parsed = parse_sequence_header(&payload).unwrap();
        assert_eq!(parsed.width, 256);
        assert_eq!(parsed.height, 192);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode::sequence`

**Step 3: Write minimal implementation**

Read the sequence header fields in the same order the encoder writes them. Only extract width and height — skip all the feature flags.

```rust
use super::bitreader::BitReader;
use crate::decode::DecodeError;

pub struct SequenceParams {
    pub width: u32,
    pub height: u32,
}

pub fn parse_sequence_header(payload: &[u8]) -> Result<SequenceParams, DecodeError> {
    let mut r = BitReader::new(payload);
    r.read_uvlc(); // seq_profile
    r.read_literal(5); // profile_bits
    r.read_bit(); // still_picture_flag
    r.read_literal(3); // padding
    r.read_bit(); // reduced_still_picture_hdr
    r.read_literal(5); // level

    let num_bits_width = r.read_literal(4) + 1;
    let num_bits_height = r.read_literal(4) + 1;
    let width = r.read_literal(num_bits_width as u8) + 1;
    let height = r.read_literal(num_bits_height as u8) + 1;

    Ok(SequenceParams { width, height })
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode::sequence`

**Step 5: Commit**

```bash
git add src/decode/sequence.rs src/decode/mod.rs
git commit -m "feat(decode): add sequence header parser"
```

---

### Task 5: Frame header parser (CLK OBU)

**Files:**
- Create: `src/decode/frame.rs`
- Modify: `src/decode/mod.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::{FrameHeaderConfig, write_clk_obu_payload};

    #[test]
    fn roundtrip_64x64_qindex50() {
        let tile_data = vec![0xAA, 0xBB];
        let config = FrameHeaderConfig { width: 64, height: 64, base_qindex: 50, order_hint: 0 };
        let payload = write_clk_obu_payload(&config, &tile_data);
        let parsed = parse_frame_header(&payload, 64, 64).unwrap();
        assert_eq!(parsed.base_qindex, 50);
        assert_eq!(parsed.order_hint, 0);
        assert_eq!(parsed.tile_data, &[0xAA, 0xBB]);
    }

    #[test]
    fn roundtrip_128x128_order_hint_5() {
        let tile_data = vec![0x01, 0x02, 0x03];
        let config = FrameHeaderConfig { width: 128, height: 128, base_qindex: 80, order_hint: 5 };
        let payload = write_clk_obu_payload(&config, &tile_data);
        let parsed = parse_frame_header(&payload, 128, 128).unwrap();
        assert_eq!(parsed.base_qindex, 80);
        assert_eq!(parsed.order_hint, 5);
        assert_eq!(parsed.tile_data, &[0x01, 0x02, 0x03]);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode::frame`

**Step 3: Write minimal implementation**

Parse the CLK OBU header fields in the same order the encoder writes them, then extract the remaining bytes as tile data.

```rust
use super::bitreader::BitReader;
use crate::decode::DecodeError;

pub struct FrameParams<'a> {
    pub base_qindex: u8,
    pub order_hint: u32,
    pub tile_data: &'a [u8],
}

pub fn parse_frame_header<'a>(
    payload: &'a [u8],
    width: u32,
    height: u32,
) -> Result<FrameParams<'a>, DecodeError> {
    let mut r = BitReader::new(payload);

    r.read_bit(); // show_frame
    r.read_uvlc(); // frame_presentation_time_info
    r.read_uvlc(); // order_hint_base

    r.read_bit(); // error_resilient_mode
    r.read_bit(); // disable_cdf_update
    let order_hint = r.read_literal(6);

    r.read_bit(); // refresh_frame_flags
    r.read_bit(); // frame_size_present

    // tile_info
    parse_tile_info(&mut r, width, height);

    let base_qindex = r.read_literal(8) as u8;

    r.read_bit(); // segmentation
    r.read_bit(); // qm_params

    if base_qindex > 0 {
        r.read_bit(); // delta_lf_present
    }
    if base_qindex > 0 {
        r.read_bit(); // loopfilter[0]
        r.read_bit(); // loopfilter[1]
    }

    r.read_bit(); // loop_restoration_present
    r.read_literal(2); // frame_type hints

    r.skip_to_byte_boundary();

    let tile_data = &payload[r.position()..];
    Ok(FrameParams { base_qindex, order_hint, tile_data })
}

fn parse_tile_info(r: &mut BitReader, width: u32, height: u32) {
    r.read_bit(); // uniform_tile_spacing

    let sb_cols = width / 64;
    let sb_rows = height / 64;

    let max_tile_width_sb = 4096 / 64;
    let max_tile_cols = 64u32;
    let max_tile_area_sb = (4096 * 2304) / (64 * 64);

    let min_log2_cols = tile_log2(max_tile_width_sb, sb_cols);
    let max_log2_cols = tile_log2(1, sb_cols.min(max_tile_cols));
    let min_log2 = tile_log2(max_tile_area_sb, sb_cols * sb_rows).max(min_log2_cols);
    let min_log2_rows = if min_log2 > 0 { min_log2 } else { 0 };
    let max_log2_rows = tile_log2(1, sb_rows.min(64));

    let mut log2_cols = min_log2_cols;
    while log2_cols < max_log2_cols {
        if !r.read_bit() { break; }
        log2_cols += 1;
    }
    let mut log2_rows = min_log2_rows;
    while log2_rows < max_log2_rows {
        if !r.read_bit() { break; }
        log2_rows += 1;
    }
}

fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode::frame`

**Step 5: Commit**

```bash
git add src/decode/frame.rs src/decode/mod.rs
git commit -m "feat(decode): add frame header parser"
```

---

### Task 6: Inverse DCT and dequantization

**Files:**
- Modify: `src/dct.rs` (add inverse DCT if not already present, make functions public)
- Create: `src/decode/transform.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dct;
    use crate::encode::{ac_qlookup, quantize_block, compute_residual_block};
    use crate::scan::DEFAULT_SCAN_32X32;

    #[test]
    fn dequantize_dc_roundtrip() {
        let dequant = ac_qlookup(50);
        let level = 524i32;
        let dqcoeff = dequantize_coeff(level, 2, dequant);
        let expected = ((524i64 * dequant as i64 + 4) >> 3) >> 2;
        assert_eq!(dqcoeff, expected as i32);
    }

    #[test]
    fn dequantize_block_roundtrip_solid() {
        let dequant = ac_qlookup(50);
        let pixels = vec![200u8; 32 * 32];
        let residual = compute_residual_block(&pixels, 32, 32);
        let mut res_arr = [0i32; 1024];
        res_arr.copy_from_slice(&residual);
        let dct_coeffs = dct::forward_dct2_32x32(&res_arr);
        let (levels, eob) = quantize_block(&dct_coeffs, dequant, 1);
        let reconstructed = dequantize_and_inverse(&levels, eob, dequant, 1, false);
        for pixel in &reconstructed {
            let val = (*pixel + 128).clamp(0, 255) as u8;
            assert!((val as i32 - 200).abs() <= 1, "pixel {} != 200", val);
        }
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode::transform`

**Step 3: Write minimal implementation**

```rust
use crate::dct;
use crate::scan::DEFAULT_SCAN_32X32;

pub fn dequantize_coeff(level: i32, log_scale: u32, dequant: i32) -> i32 {
    let abs_level = level.unsigned_abs() as i64;
    let sign = if level < 0 { -1i32 } else { 1i32 };
    let dqcoeff = ((abs_level * dequant as i64 + 4) >> 3) >> log_scale;
    sign * dqcoeff as i32
}

pub fn dequantize_block(
    levels: &[i32; 1024],
    eob: u16,
    dequant: i32,
    log_scale: u32,
) -> [i32; 1024] {
    let mut dct_coeffs = [0i32; 1024];
    for scan_idx in 0..eob as usize {
        let coeff_idx = DEFAULT_SCAN_32X32[scan_idx] as usize;
        dct_coeffs[coeff_idx] = dequantize_coeff(levels[scan_idx], log_scale, dequant);
    }
    dct_coeffs
}

pub fn dequantize_and_inverse(
    levels: &[i32; 1024],
    eob: u16,
    dequant: i32,
    log_scale: u32,
    is_64x64: bool,
) -> Vec<i32> {
    let dct_coeffs = dequantize_block(levels, eob, dequant, log_scale);
    if is_64x64 {
        dct::inverse_dct2_64x64(&dct_coeffs).to_vec()
    } else {
        dct::inverse_dct2_32x32(&dct_coeffs).to_vec()
    }
}
```

**Note:** The inverse DCT functions (`inverse_dct2_32x32`, `inverse_dct2_64x64`) may need to be added to `src/dct.rs` if they don't exist yet. They use the transpose of the forward kernel with shifts `{6, 13}`.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode::transform`

**Step 5: Commit**

```bash
git add src/decode/transform.rs src/decode/mod.rs src/dct.rs
git commit -m "feat(decode): add dequantization and inverse DCT"
```

---

### Task 7: Tile coefficient decoder

**Files:**
- Create: `src/decode/tile.rs`
- Modify: `src/decode/mod.rs`

This is the largest task — decode coefficients from the arithmetic stream. Mirrors `src/tile.rs` exactly, using the same context functions and CDF tables.

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile;

    #[test]
    fn roundtrip_all_zero() {
        let data = tile::encode_tile_data();
        let mut dec = DaalaDecoder::new(&data);
        let sb = decode_superblock(&mut dec, 0, &[0u8; 16], &[0u8; 16], &[0u8; 8], &[0u8; 8], &[0u8; 8], &[0u8; 8]);
        assert_eq!(sb.y_eob, 0);
        assert_eq!(sb.u_eob, 0);
        assert_eq!(sb.v_eob, 0);
    }

    #[test]
    fn roundtrip_dc_only() {
        let data = tile::encode_tile_data_with_dc(50, -10, 5);
        let mut dec = DaalaDecoder::new(&data);
        let sb = decode_superblock(&mut dec, 0, &[0u8; 16], &[0u8; 16], &[0u8; 8], &[0u8; 8], &[0u8; 8], &[0u8; 8]);
        assert_eq!(sb.y_levels[0], 50);
        assert_eq!(sb.y_eob, 1);
        assert_eq!(sb.u_levels[0], -10);
        assert_eq!(sb.u_eob, 1);
        assert_eq!(sb.v_levels[0], 5);
        assert_eq!(sb.v_eob, 1);
    }

    #[test]
    fn roundtrip_ac_coefficients() {
        let mut y_levels = [0i32; 1024];
        y_levels[0] = 100;
        y_levels[1] = -20;
        y_levels[2] = 10;
        let data = tile::encode_tile_data_with_coeffs(&y_levels, 3, &[0i32; 1024], 0, &[0i32; 1024], 0);
        let mut dec = DaalaDecoder::new(&data);
        let sb = decode_superblock(&mut dec, 0, &[0u8; 16], &[0u8; 16], &[0u8; 8], &[0u8; 8], &[0u8; 8], &[0u8; 8]);
        assert_eq!(sb.y_eob, 3);
        assert_eq!(sb.y_levels[0], 100);
        assert_eq!(sb.y_levels[1], -20);
        assert_eq!(sb.y_levels[2], 10);
    }

    #[test]
    fn roundtrip_multi_sb_2x2() {
        let sbs: Vec<crate::tile::SbCoeffs> = (0..4).map(|i| {
            let mut sb = crate::tile::SbCoeffs {
                y_levels: [0i32; 1024], y_eob: 0,
                u_levels: [0i32; 1024], u_eob: 0,
                v_levels: [0i32; 1024], v_eob: 0,
            };
            sb.y_levels[0] = (i + 1) * 10;
            sb.y_eob = 1;
            sb.u_levels[0] = (i + 1) * 5;
            sb.u_eob = 1;
            sb.v_levels[0] = (i + 1) * 3;
            sb.v_eob = 1;
            sb
        }).collect();
        let data = crate::tile::encode_tile_data_multi_sb(2, 2, &sbs);
        let decoded = decode_tile_data_multi_sb(&data, 2, 2);
        for (i, dsb) in decoded.iter().enumerate() {
            assert_eq!(dsb.y_levels[0], (i as i32 + 1) * 10);
            assert_eq!(dsb.u_levels[0], (i as i32 + 1) * 5);
            assert_eq!(dsb.v_levels[0], (i as i32 + 1) * 3);
        }
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode::tile`

**Step 3: Write minimal implementation**

The tile decoder mirrors the encoder's `encode_plane_full` function exactly — reading symbols in the same order with the same context computation. The context functions (`get_lower_levels_ctx_*`, `get_br_*`, `is_lf_2d`, etc.) should be extracted from `src/tile.rs` into shared helpers, or the decoder can call them directly if made public.

Key structure:
- `decode_superblock()` reads partition mode, intra mode, then Y/U/V planes
- `decode_plane_full()` reads txb_skip, EOB, coefficients in reverse scan order
- Context computation is identical to encoder

The implementation agent must replicate the exact encoding order:
1. Base levels (forward scan, eob position first, then eob-1..1, then DC=0)
2. Sign + high-range (reverse scan, eob-1..0)

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode::tile`

**Step 5: Commit**

```bash
git add src/decode/tile.rs src/decode/mod.rs
git commit -m "feat(decode): add tile coefficient decoder"
```

---

### Task 8: Decoder orchestrator and public API

**Files:**
- Modify: `src/decode/mod.rs`
- Modify: `src/lib.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{encode_video_yuv, Encoder};

    #[test]
    fn decode_solid_gray() {
        let y = vec![128u8; 64 * 64];
        let u = vec![128u8; 32 * 32];
        let v = vec![128u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].width, 64);
        assert_eq!(frames[0].height, 64);
        assert!(frames[0].y.iter().all(|&p| p == 128));
    }

    #[test]
    fn decode_solid_green() {
        let y = vec![149u8; 64 * 64];
        let u = vec![43u8; 32 * 32];
        let v = vec![21u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 50, &[(y.clone(), u.clone(), v.clone())]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        for (i, &p) in frames[0].y.iter().enumerate() {
            assert!((p as i32 - 149).abs() <= 1, "Y pixel {} = {}", i, p);
        }
    }

    #[test]
    fn decode_multi_frame() {
        let frames_in: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = (0..3).map(|i| {
            let val = 100 + i * 20;
            (vec![val; 64 * 64], vec![128u8; 32 * 32], vec![128u8; 32 * 32])
        }).collect();
        let bs = encode_video_yuv(64, 64, 50, &frames_in);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 3);
    }

    #[test]
    fn decode_128x128() {
        let y = vec![200u8; 128 * 128];
        let u = vec![100u8; 64 * 64];
        let v = vec![150u8; 64 * 64];
        let bs = encode_video_yuv(128, 128, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].width, 128);
        assert_eq!(frames[0].height, 128);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib decode -- -v`

**Step 3: Write minimal implementation**

The `Decoder` orchestrates all components:
1. Parse OBUs
2. First OBU = sequence header → extract width, height
3. Each CLK OBU → parse frame header → decode tile data → dequantize → inverse DCT → reconstruct pixels

```rust
pub mod bitreader;
pub mod entropy;
pub mod obu;
pub mod sequence;
pub mod frame;
pub mod tile;
pub mod transform;

use std::fmt;

#[derive(Debug)]
pub enum DecodeError {
    InvalidObu,
    UnsupportedProfile,
    UnsupportedFeature(&'static str),
    BitstreamCorrupt,
    UnexpectedEof,
}

impl fmt::Display for DecodeError { ... }
impl std::error::Error for DecodeError {}

pub struct DecodedFrame {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub frame_number: u64,
}

pub struct Decoder {
    width: u32,
    height: u32,
}

impl Decoder {
    pub fn decode_all(data: &[u8]) -> Result<Vec<DecodedFrame>, DecodeError> {
        let obus = obu::parse_obus(data);
        if obus.is_empty() {
            return Err(DecodeError::InvalidObu);
        }

        let seq = sequence::parse_sequence_header(obus[0].payload)?;
        let dequant = crate::encode::ac_qlookup(0); // will be set per frame

        let mut frames = Vec::new();
        let mut frame_number = 0u64;

        for raw_obu in &obus[1..] {
            if raw_obu.obu_type != 4 { continue; } // Only CLK OBUs
            let fp = frame::parse_frame_header(raw_obu.payload, seq.width, seq.height)?;
            let dequant = crate::encode::ac_qlookup(fp.base_qindex);
            let sb_cols = (seq.width / 64) as usize;
            let sb_rows = (seq.height / 64) as usize;

            let decoded_sbs = tile::decode_tile_data_multi_sb(fp.tile_data, sb_cols, sb_rows);

            // Reconstruct pixels from decoded coefficients
            let frame = reconstruct_frame(&decoded_sbs, sb_cols, sb_rows, seq.width, seq.height, dequant);
            frames.push(DecodedFrame {
                y: frame.0, u: frame.1, v: frame.2,
                width: seq.width, height: seq.height,
                frame_number,
            });
            frame_number += 1;
        }
        Ok(frames)
    }
}

fn reconstruct_frame(
    sbs: &[tile::DecodedSbCoeffs],
    sb_cols: usize, sb_rows: usize,
    width: u32, height: u32,
    dequant: i32,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    let mut y_plane = vec![0u8; w * h];
    let mut u_plane = vec![0u8; (w/2) * (h/2)];
    let mut v_plane = vec![0u8; (w/2) * (h/2)];

    for sb_row in 0..sb_rows {
        for sb_col in 0..sb_cols {
            let sb = &sbs[sb_row * sb_cols + sb_col];

            // Y: dequantize + inverse DCT 64x64
            let y_residuals = transform::dequantize_and_inverse(&sb.y_levels, sb.y_eob, dequant, 2, true);
            for row in 0..64 {
                for col in 0..64 {
                    let px = (y_residuals[row * 64 + col] + 128).clamp(0, 255) as u8;
                    y_plane[(sb_row * 64 + row) * w + sb_col * 64 + col] = px;
                }
            }

            // U: dequantize + inverse DCT 32x32
            let u_residuals = transform::dequantize_and_inverse(&sb.u_levels, sb.u_eob, dequant, 1, false);
            for row in 0..32 {
                for col in 0..32 {
                    let px = (u_residuals[row * 32 + col] + 128).clamp(0, 255) as u8;
                    u_plane[(sb_row * 32 + row) * (w/2) + sb_col * 32 + col] = px;
                }
            }

            // V: dequantize + inverse DCT 32x32
            let v_residuals = transform::dequantize_and_inverse(&sb.v_levels, sb.v_eob, dequant, 1, false);
            for row in 0..32 {
                for col in 0..32 {
                    let px = (v_residuals[row * 32 + col] + 128).clamp(0, 255) as u8;
                    v_plane[(sb_row * 32 + row) * (w/2) + sb_col * 32 + col] = px;
                }
            }
        }
    }

    (y_plane, u_plane, v_plane)
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib decode`

**Step 5: Commit**

```bash
git add src/decode/ src/lib.rs
git commit -m "feat(decode): add Decoder orchestrator with public API"
```

---

### Task 9: Integration test — bit-exact vs avmdec

**Files:**
- Modify: `tests/avmdec_integration.rs` (add decoder tests)

**Step 1: Write the failing test**

```rust
#[test]
#[ignore]
fn wav2c_decode_matches_avmdec_solid_green() {
    let y = vec![149u8; 64 * 64];
    let u = vec![43u8; 32 * 32];
    let v = vec![21u8; 32 * 32];
    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &[(y, u, v)]);

    // Decode with wav2c decoder
    let our_frames = wav2c::decode::Decoder::decode_all(&bs).unwrap();
    assert_eq!(our_frames.len(), 1);

    // Decode with avmdec
    let obu_path = std::env::temp_dir().join("test_decode_green.obu");
    let yuv_path = std::env::temp_dir().join("test_decode_green.yuv");
    std::fs::write(&obu_path, &bs).unwrap();

    let status = std::process::Command::new("../avm_build/avmdec")
        .args(["--i420", "-o"])
        .arg(&yuv_path)
        .arg(&obu_path)
        .status()
        .expect("avmdec not found");
    assert!(status.success());

    let ref_yuv = std::fs::read(&yuv_path).unwrap();
    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    let ref_y = &ref_yuv[0..y_size];
    let ref_u = &ref_yuv[y_size..y_size + uv_size];
    let ref_v = &ref_yuv[y_size + uv_size..y_size + 2 * uv_size];

    // Bit-exact comparison
    assert_eq!(our_frames[0].y, ref_y, "Y plane mismatch");
    assert_eq!(our_frames[0].u, ref_u, "U plane mismatch");
    assert_eq!(our_frames[0].v, ref_v, "V plane mismatch");
}

#[test]
#[ignore]
fn wav2c_decode_matches_avmdec_gradient() {
    // Similar test with gradient input
    // Generate gradient, encode, decode with both, compare
}

#[test]
#[ignore]
fn wav2c_decode_matches_avmdec_256x192() {
    // Multi-superblock test
}
```

**Step 2: Run test**

Run: `cargo test --test avmdec_integration wav2c_decode_matches -- --ignored`

**Step 3: Fix any mismatches**

Debug pixel differences, fix dequantization/inverse DCT issues.

**Step 4: Commit**

```bash
git add tests/avmdec_integration.rs
git commit -m "test: add bit-exact decoder vs avmdec integration tests"
```

---

### Task 10: CLI binary — wav2c-decode

**Files:**
- Create: `src/bin/wav2c_decode.rs`

**Step 1: Write the binary**

```rust
use std::env;
use std::fs;
use std::io::Write;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: wav2c-decode <input.obu> <output.y4m>");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];

    let data = fs::read(input_path).expect("failed to read input file");
    let frames = wav2c::decode::Decoder::decode_all(&data).expect("decode failed");

    if frames.is_empty() {
        eprintln!("No frames decoded");
        std::process::exit(1);
    }

    let width = frames[0].width;
    let height = frames[0].height;

    let mut out = fs::File::create(output_path).expect("failed to create output file");
    write!(out, "YUV4MPEG2 W{} H{} F25:1 Ip C420\n", width, height).unwrap();

    for frame in &frames {
        write!(out, "FRAME\n").unwrap();
        out.write_all(&frame.y).unwrap();
        out.write_all(&frame.u).unwrap();
        out.write_all(&frame.v).unwrap();
    }

    eprintln!("Decoded {} frame(s), {}x{}", frames.len(), width, height);
}
```

**Step 2: Build and test**

```bash
cargo build --bin wav2c_decode
cargo run --bin wav2c -- tests/fixtures/solid_green_64x64.y4m /tmp/test.obu --qindex 50
cargo run --bin wav2c_decode -- /tmp/test.obu /tmp/decoded.y4m
ffplay /tmp/decoded.y4m
```

**Step 3: Commit**

```bash
git add src/bin/wav2c_decode.rs
git commit -m "feat: add wav2c-decode CLI binary"
```

---

### Task 11: PSNR/VMAF quality metrics

**Files:**
- Add PSNR calculation utility for use in tests

**Step 1: Add PSNR helper to integration tests**

```rust
fn compute_psnr(original: &[u8], decoded: &[u8]) -> f64 {
    assert_eq!(original.len(), decoded.len());
    let mse: f64 = original.iter().zip(decoded.iter())
        .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
        .sum::<f64>() / original.len() as f64;
    if mse == 0.0 { return f64::INFINITY; }
    10.0 * (255.0f64 * 255.0 / mse).log10()
}
```

**Step 2: Add quality tests**

```rust
#[test]
fn decoder_psnr_solid_is_infinity() {
    let y = vec![128u8; 64 * 64];
    let u = vec![128u8; 32 * 32];
    let v = vec![128u8; 32 * 32];
    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &[(y.clone(), u.clone(), v.clone())]);
    let frames = wav2c::decode::Decoder::decode_all(&bs).unwrap();
    let psnr = compute_psnr(&y, &frames[0].y);
    assert!(psnr > 60.0, "PSNR should be very high for lossless roundtrip: {}", psnr);
}
```

**Step 3: Commit**

```bash
git add tests/
git commit -m "test: add PSNR quality metrics for decoder validation"
```
