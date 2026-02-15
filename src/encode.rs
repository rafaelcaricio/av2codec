use crate::bitstream::{
    FrameHeaderConfig, ObuType, ObuWriter, SequenceHeaderConfig, write_clk_obu_payload,
    write_sequence_header,
};
use crate::tile;

pub fn encode_still_image(width: u32, height: u32, base_qindex: u8) -> Vec<u8> {
    let seq_payload = write_sequence_header(&SequenceHeaderConfig { width, height });
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);

    let tile_data = tile::encode_tile_data();
    let clk_payload = write_clk_obu_payload(&FrameHeaderConfig { base_qindex }, &tile_data);
    let clk_obu = ObuWriter::pack_obu(ObuType::Clk, &clk_payload);

    let mut bitstream = seq_obu;
    bitstream.extend_from_slice(&clk_obu);
    bitstream
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitstream_starts_with_seq_header_obu() {
        let bs = encode_still_image(64, 64, 50);
        assert!(bs.len() > 13);
        let obu_type = (bs[1] >> 2) & 0x1F;
        assert_eq!(obu_type, 1, "first OBU should be SequenceHeader");
    }

    #[test]
    fn bitstream_contains_two_obus() {
        let bs = encode_still_image(64, 64, 50);
        // OBU format: LEB128(content_size) | header_byte | payload
        // LEB128 value = 1 (header) + payload_len
        // Total first OBU bytes = LEB128_prefix_bytes + LEB128_value
        let seq_content_size = bs[0] as usize; // 12 for 64x64 (1 byte LEB128)
        let second_obu_start = 1 + seq_content_size; // LEB128 prefix (1 byte) + content
        let remaining = &bs[second_obu_start..];
        assert!(
            !remaining.is_empty(),
            "should have CLK OBU after seq header"
        );
        let clk_type = (remaining[1] >> 2) & 0x1F;
        assert_eq!(clk_type, 4, "second OBU should be CLK");
    }

    #[test]
    fn bitstream_reasonable_size() {
        let bs = encode_still_image(64, 64, 50);
        assert!(bs.len() <= 50, "bitstream too large: {} bytes", bs.len());
    }
}
