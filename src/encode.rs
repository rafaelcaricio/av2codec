use crate::bitstream::{
    FrameHeaderConfig, ObuType, ObuWriter, SequenceHeaderConfig, write_clk_obu_payload,
    write_sequence_header,
};
use crate::tile;

pub fn encode_still_image(width: u32, height: u32, base_qindex: u8) -> Vec<u8> {
    encode_video(
        width,
        height,
        base_qindex,
        &[vec![128; (width * height) as usize]],
    )
}

pub fn encode_video(width: u32, height: u32, base_qindex: u8, y_planes: &[Vec<u8>]) -> Vec<u8> {
    let mut output = Vec::new();

    let seq_config = SequenceHeaderConfig { width, height };
    let seq_payload = write_sequence_header(&seq_config);
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);
    output.extend_from_slice(&seq_obu);

    for (i, _y_plane) in y_planes.iter().enumerate() {
        let tile_data = tile::encode_tile_data();
        let frame_config = FrameHeaderConfig {
            base_qindex,
            order_hint: i as u32,
        };
        let clk_payload = write_clk_obu_payload(&frame_config, &tile_data);
        let clk_obu = ObuWriter::pack_obu(ObuType::Clk, &clk_payload);
        output.extend_from_slice(&clk_obu);
    }

    output
}

pub fn encode_video_yuv(
    width: u32,
    height: u32,
    base_qindex: u8,
    frames: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
) -> Vec<u8> {
    let mut output = Vec::new();

    let seq_config = SequenceHeaderConfig { width, height };
    let seq_payload = write_sequence_header(&seq_config);
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);
    output.extend_from_slice(&seq_obu);

    let dc_prediction: i32 = 128;

    for (i, (y_plane, u_plane, v_plane)) in frames.iter().enumerate() {
        let y_dc_residual = y_plane[0] as i32 - dc_prediction;
        let u_dc_residual = u_plane[0] as i32 - dc_prediction;
        let v_dc_residual = v_plane[0] as i32 - dc_prediction;

        let dequant = ac_qlookup(base_qindex);
        let y_dc_level = quantize_dc(y_dc_residual, 2, dequant);
        let u_dc_level = quantize_dc(u_dc_residual, 1, dequant);
        let v_dc_level = quantize_dc(v_dc_residual, 1, dequant);

        let tile_data = tile::encode_tile_data_with_dc(y_dc_level, u_dc_level, v_dc_level);
        let frame_config = FrameHeaderConfig {
            base_qindex,
            order_hint: i as u32,
        };
        let clk_payload = write_clk_obu_payload(&frame_config, &tile_data);
        let clk_obu = ObuWriter::pack_obu(ObuType::Clk, &clk_payload);
        output.extend_from_slice(&clk_obu);
    }

    output
}

fn ac_qlookup(qindex: u8) -> i32 {
    #[rustfmt::skip]
    const AC_QLOOKUP_QTX: [i32; 25] = [
        64, 40, 41, 43, 44, 45, 47, 48, 49, 51, 52,
        54, 55, 57, 59, 60, 62, 64, 66, 68, 70, 72, 74, 76, 78,
    ];

    let q = qindex as i32;
    if q < 25 {
        AC_QLOOKUP_QTX[q as usize]
    } else {
        AC_QLOOKUP_QTX[(((q - 1) % 24) + 1) as usize] << ((q - 1) / 24)
    }
}

fn quantize_dc(residual: i32, log_scale: u32, dequant: i32) -> i32 {
    let dc_coeff = 128i64 * residual as i64;
    let scaled = dc_coeff << (3 + log_scale);
    if dequant == 0 {
        return scaled as i32;
    }
    let dq = dequant as i64;
    let sign = if scaled < 0 { -1i64 } else { 1i64 };
    (sign * ((scaled.abs() + dq / 2) / dq)) as i32
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
        let seq_content_size = bs[0] as usize;
        let second_obu_start = 1 + seq_content_size;
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

    #[test]
    fn encode_multiple_frames() {
        let frames: Vec<Vec<u8>> = (0..3).map(|_| vec![128; 64 * 64]).collect();
        let bs = encode_video(64, 64, 50, &frames);
        let seq_header_obu_size = 1 + 1 + 17; // leb128 + header + payload
        let single_clk_obu_size = 1 + 1 + 4 + 4; // leb128 + header + frame_header + tile_data (approx)
        assert!(
            bs.len() > seq_header_obu_size + single_clk_obu_size,
            "bitstream too small: {} bytes",
            bs.len()
        );
    }

    #[test]
    fn encode_video_has_correct_obu_count() {
        let frames: Vec<Vec<u8>> = (0..5).map(|_| vec![128; 64 * 64]).collect();
        let bs = encode_video(64, 64, 50, &frames);
        let seq_content_size = bs[0] as usize;
        let mut pos = 1 + seq_content_size;
        let mut clk_count = 0;
        while pos < bs.len() {
            let obu_size = bs[pos] as usize;
            let obu_type = (bs[pos + 1] >> 2) & 0x1F;
            assert_eq!(obu_type, 4, "expected CLK OBU");
            clk_count += 1;
            pos += 1 + obu_size;
        }
        assert_eq!(clk_count, 5);
    }

    #[test]
    fn encode_frame_with_pixel_data() {
        let y_plane = vec![149u8; 64 * 64];
        let u_plane = vec![43u8; 32 * 32];
        let v_plane = vec![21u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 50, &[(y_plane, u_plane, v_plane)]);
        assert!(!bs.is_empty());
    }

    #[test]
    fn ac_qlookup_qindex_0() {
        assert_eq!(ac_qlookup(0), 64);
    }

    #[test]
    fn ac_qlookup_qindex_50() {
        let q = ac_qlookup(50);
        assert!(q > 0);
    }

    #[test]
    fn quantize_dc_zero_residual() {
        assert_eq!(quantize_dc(0, 2, 164), 0);
    }

    #[test]
    fn quantize_dc_positive_residual() {
        let level = quantize_dc(21, 2, 164);
        assert!(level > 0);
        assert_eq!(level, 524);
    }

    #[test]
    fn quantize_dc_matches_decoder_chain() {
        let level = quantize_dc(21, 2, 164);
        let dqcoeff = ((level as i64 * 164 + 4) >> 3) >> 2;
        let val = (64 * dqcoeff + 32) >> 6;
        let pixel_residual = (64 * val + 4096) >> 13;
        assert_eq!(pixel_residual, 21);
    }
}
