use super::bitreader::BitReader;
use super::DecodeError;

const SEQ_LEVEL_4_0: u32 = 8;

pub struct SequenceParams {
    pub width: u32,
    pub height: u32,
}

pub fn parse_sequence_header(payload: &[u8]) -> Result<SequenceParams, DecodeError> {
    let mut r = BitReader::new(payload);

    let _seq_header_id = r.read_uvlc();
    let profile = r.read_literal(5);
    if profile != 0 {
        return Err(DecodeError::UnsupportedProfile);
    }

    let single_picture_header_flag = r.read_bit();
    if !single_picture_header_flag {
        let _seq_lcr_id = r.read_literal(3);
        let _still_picture = r.read_bit();
    }

    let level = r.read_literal(5);
    if level >= SEQ_LEVEL_4_0 && !single_picture_header_flag {
        let _seq_tier = r.read_bit();
    }

    let num_bits_width = r.read_literal(4) + 1;
    let num_bits_height = r.read_literal(4) + 1;
    let width = r.read_literal(num_bits_width as u8) + 1;
    let height = r.read_literal(num_bits_height as u8) + 1;

    Ok(SequenceParams { width, height })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::{BitWriter, SequenceHeaderConfig, write_sequence_header};

    #[test]
    fn roundtrip_64x64() {
        let payload = write_sequence_header(&SequenceHeaderConfig {
            width: 64,
            height: 64,
        });
        let params = parse_sequence_header(&payload).unwrap();
        assert_eq!(params.width, 64);
        assert_eq!(params.height, 64);
    }

    #[test]
    fn roundtrip_128x128() {
        let payload = write_sequence_header(&SequenceHeaderConfig {
            width: 128,
            height: 128,
        });
        let params = parse_sequence_header(&payload).unwrap();
        assert_eq!(params.width, 128);
        assert_eq!(params.height, 128);
    }

    #[test]
    fn roundtrip_256x192() {
        let payload = write_sequence_header(&SequenceHeaderConfig {
            width: 256,
            height: 192,
        });
        let params = parse_sequence_header(&payload).unwrap();
        assert_eq!(params.width, 256);
        assert_eq!(params.height, 192);
    }

    #[test]
    fn roundtrip_256x256() {
        let payload = write_sequence_header(&SequenceHeaderConfig {
            width: 256,
            height: 256,
        });
        let params = parse_sequence_header(&payload).unwrap();
        assert_eq!(params.width, 256);
        assert_eq!(params.height, 256);
    }

    #[test]
    fn roundtrip_1920x1080() {
        let payload = write_sequence_header(&SequenceHeaderConfig {
            width: 1920,
            height: 1080,
        });
        let params = parse_sequence_header(&payload).unwrap();
        assert_eq!(params.width, 1920);
        assert_eq!(params.height, 1080);
    }

    #[test]
    fn parse_sph1_768x768() {
        let mut w = BitWriter::new();
        w.write_uvlc(0);
        w.write_literal(0, 5);
        w.write_bit(true);
        w.write_literal(4, 5);
        let num_bits_w = 10u32;
        let num_bits_h = 10u32;
        w.write_literal(num_bits_w - 1, 4);
        w.write_literal(num_bits_h - 1, 4);
        w.write_literal(768 - 1, num_bits_w as u8);
        w.write_literal(768 - 1, num_bits_h as u8);
        w.write_trailing_bits();
        let payload = w.finalize();

        let params = parse_sequence_header(&payload).unwrap();
        assert_eq!(params.width, 768);
        assert_eq!(params.height, 768);
    }

    #[test]
    fn parse_sph1_1920x1080() {
        let mut w = BitWriter::new();
        w.write_uvlc(0);
        w.write_literal(0, 5);
        w.write_bit(true);
        w.write_literal(8, 5);
        let num_bits_w = 11u32;
        let num_bits_h = 11u32;
        w.write_literal(num_bits_w - 1, 4);
        w.write_literal(num_bits_h - 1, 4);
        w.write_literal(1920 - 1, num_bits_w as u8);
        w.write_literal(1080 - 1, num_bits_h as u8);
        w.write_trailing_bits();
        let payload = w.finalize();

        let params = parse_sequence_header(&payload).unwrap();
        assert_eq!(params.width, 1920);
        assert_eq!(params.height, 1080);
    }
}
