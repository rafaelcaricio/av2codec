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
        let remaining = self.read_bits(leading_zeros);
        ((1u64 << leading_zeros) | remaining) as u32 - 1
    }

    pub fn read_trailing_bits(&mut self) {
        let _marker = self.read_bit();
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    pub fn byte_aligned(&self) -> bool {
        self.bit_pos == 0
    }

    pub fn position(&self) -> usize {
        self.byte_pos
    }

    pub fn remaining_bytes(&self) -> &'a [u8] {
        assert!(self.byte_aligned(), "must be byte-aligned to get remaining bytes");
        &self.data[self.byte_pos..]
    }

    pub fn skip_to_byte_boundary(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    pub fn has_remaining(&self) -> bool {
        self.byte_pos < self.data.len()
    }

    pub fn total_bits_read(&self) -> usize {
        self.byte_pos * 8 + self.bit_pos as usize
    }
}

pub fn decode_leb128(data: &[u8]) -> (u32, usize) {
    let mut value = 0u32;
    let mut bytes_consumed = 0usize;
    for (i, &byte) in data.iter().enumerate() {
        value |= ((byte & 0x7F) as u32) << (i * 7);
        bytes_consumed += 1;
        if byte & 0x80 == 0 {
            break;
        }
    }
    (value, bytes_consumed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::BitWriter;

    #[test]
    fn read_bit_roundtrip() {
        let mut w = BitWriter::new();
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(false);
        w.write_bit(true);
        w.write_bit(false);
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        assert!(r.read_bit());
        assert!(!r.read_bit());
        assert!(r.read_bit());
        assert!(r.read_bit());
        assert!(!r.read_bit());
        assert!(!r.read_bit());
        assert!(r.read_bit());
        assert!(!r.read_bit());
    }

    #[test]
    fn read_bits_multi_byte_roundtrip() {
        let mut w = BitWriter::new();
        w.write_bits(0xFF, 8);
        w.write_bits(0x00, 8);
        w.write_bits(0xAB, 8);
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bits(8), 0xFF);
        assert_eq!(r.read_bits(8), 0x00);
        assert_eq!(r.read_bits(8), 0xAB);
    }

    #[test]
    fn read_bits_partial_roundtrip() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3);
        w.write_bits(0b1100, 4);
        w.write_bits(0b1, 1);
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bits(3), 0b101);
        assert_eq!(r.read_bits(4), 0b1100);
        assert_eq!(r.read_bits(1), 0b1);
    }

    #[test]
    fn read_literal_roundtrip() {
        let mut w = BitWriter::new();
        w.write_literal(5, 3);
        w.write_literal(1023, 10);
        w.write_literal(0, 5);
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        assert_eq!(r.read_literal(3), 5);
        assert_eq!(r.read_literal(10), 1023);
        assert_eq!(r.read_literal(5), 0);
    }

    #[test]
    fn read_uvlc_zero() {
        let mut w = BitWriter::new();
        w.write_uvlc(0);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 0);
    }

    #[test]
    fn read_uvlc_one() {
        let mut w = BitWriter::new();
        w.write_uvlc(1);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 1);
    }

    #[test]
    fn read_uvlc_two() {
        let mut w = BitWriter::new();
        w.write_uvlc(2);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 2);
    }

    #[test]
    fn read_uvlc_three() {
        let mut w = BitWriter::new();
        w.write_uvlc(3);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 3);
    }

    #[test]
    fn read_uvlc_seven() {
        let mut w = BitWriter::new();
        w.write_uvlc(7);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 7);
    }

    #[test]
    fn read_uvlc_fifteen() {
        let mut w = BitWriter::new();
        w.write_uvlc(15);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 15);
    }

    #[test]
    fn read_uvlc_hundred() {
        let mut w = BitWriter::new();
        w.write_uvlc(100);
        let data = w.finalize();
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 100);
    }

    #[test]
    fn read_uvlc_multiple_values() {
        let mut w = BitWriter::new();
        w.write_uvlc(0);
        w.write_uvlc(1);
        w.write_uvlc(3);
        w.write_uvlc(100);
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        assert_eq!(r.read_uvlc(), 0);
        assert_eq!(r.read_uvlc(), 1);
        assert_eq!(r.read_uvlc(), 3);
        assert_eq!(r.read_uvlc(), 100);
    }

    #[test]
    fn decode_leb128_zero() {
        let data = [0x00];
        let (value, consumed) = decode_leb128(&data);
        assert_eq!(value, 0);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn decode_leb128_one() {
        let data = [0x01];
        let (value, consumed) = decode_leb128(&data);
        assert_eq!(value, 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn decode_leb128_127() {
        let data = [0x7F];
        let (value, consumed) = decode_leb128(&data);
        assert_eq!(value, 127);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn decode_leb128_128() {
        let data = [0x80, 0x01];
        let (value, consumed) = decode_leb128(&data);
        assert_eq!(value, 128);
        assert_eq!(consumed, 2);
    }

    #[test]
    fn decode_leb128_300() {
        let data = [0xAC, 0x02];
        let (value, consumed) = decode_leb128(&data);
        assert_eq!(value, 300);
        assert_eq!(consumed, 2);
    }

    #[test]
    fn decode_leb128_16384() {
        let data = [0x80, 0x80, 0x01];
        let (value, consumed) = decode_leb128(&data);
        assert_eq!(value, 16384);
        assert_eq!(consumed, 3);
    }

    #[test]
    fn trailing_bits_and_byte_aligned() {
        let mut w = BitWriter::new();
        w.write_bits(0b1111111, 7);
        w.write_trailing_bits();
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        r.read_bits(7);
        assert!(!r.byte_aligned());
        r.read_trailing_bits();
        assert!(r.byte_aligned());
    }

    #[test]
    fn trailing_bits_from_empty() {
        let mut w = BitWriter::new();
        w.write_trailing_bits();
        let data = w.finalize();
        assert_eq!(data, vec![0x80]);

        let mut r = BitReader::new(&data);
        r.read_trailing_bits();
        assert!(r.byte_aligned());
        assert_eq!(r.position(), 1);
    }

    #[test]
    fn position_and_remaining_bytes() {
        let data = [0xAA, 0xBB, 0xCC, 0xDD];
        let mut r = BitReader::new(&data);

        assert_eq!(r.position(), 0);
        assert_eq!(r.remaining_bytes(), &[0xAA, 0xBB, 0xCC, 0xDD]);

        r.read_bits(8);
        assert_eq!(r.position(), 1);
        assert_eq!(r.remaining_bytes(), &[0xBB, 0xCC, 0xDD]);

        r.read_bits(16);
        assert_eq!(r.position(), 3);
        assert_eq!(r.remaining_bytes(), &[0xDD]);
    }

    #[test]
    fn skip_to_byte_boundary() {
        let data = [0xFF, 0xAA];
        let mut r = BitReader::new(&data);

        r.read_bits(3);
        assert!(!r.byte_aligned());
        r.skip_to_byte_boundary();
        assert!(r.byte_aligned());
        assert_eq!(r.position(), 1);
        assert_eq!(r.read_bits(8), 0xAA);
    }

    #[test]
    fn skip_to_byte_boundary_already_aligned() {
        let data = [0xFF, 0xAA];
        let mut r = BitReader::new(&data);

        assert!(r.byte_aligned());
        r.skip_to_byte_boundary();
        assert!(r.byte_aligned());
        assert_eq!(r.position(), 0);
    }

    #[test]
    fn read_bits_crossing_byte_boundary() {
        let mut w = BitWriter::new();
        w.write_bits(0b1010, 4);
        w.write_bits(0b11001100, 8);
        w.write_bits(0b0101, 4);
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bits(4), 0b1010);
        assert_eq!(r.read_bits(8), 0b11001100);
        assert_eq!(r.read_bits(4), 0b0101);
    }

    #[test]
    fn read_large_value() {
        let mut w = BitWriter::new();
        w.write_bits(0xDEADBEEF, 32);
        let data = w.finalize();

        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bits(32), 0xDEADBEEF);
    }
}
