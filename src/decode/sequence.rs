use super::bitreader::BitReader;
use super::DecodeError;

pub struct SequenceParams {
    pub width: u32,
    pub height: u32,
}

pub fn parse_sequence_header(payload: &[u8]) -> Result<SequenceParams, DecodeError> {
    let mut r = BitReader::new(payload);

    let profile = r.read_uvlc();
    if profile != 0 {
        return Err(DecodeError::UnsupportedProfile);
    }

    let _profile_bits = r.read_literal(5);
    let _still_picture_flag = r.read_bit();
    let _padding = r.read_literal(3);
    let _reduced_still_picture_hdr = r.read_bit();
    let _level = r.read_literal(5);

    let num_bits_width = r.read_literal(4) + 1;
    let num_bits_height = r.read_literal(4) + 1;
    let width = r.read_literal(num_bits_width as u8) + 1;
    let height = r.read_literal(num_bits_height as u8) + 1;

    Ok(SequenceParams { width, height })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::{SequenceHeaderConfig, write_sequence_header};

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
}
