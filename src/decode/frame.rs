use super::bitreader::BitReader;
use super::DecodeError;

pub struct FrameParams<'a> {
    pub base_qindex: u8,
    pub order_hint: u32,
    pub tile_data: &'a [u8],
}

fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

fn parse_tile_info(r: &mut BitReader, width: u32, height: u32) {
    r.read_bit();

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
        if !r.read_bit() {
            break;
        }
        log2_cols += 1;
    }
    let mut log2_rows = min_log2_rows;
    while log2_rows < max_log2_rows {
        if !r.read_bit() {
            break;
        }
        log2_rows += 1;
    }
}

pub fn parse_frame_header<'a>(
    payload: &'a [u8],
    width: u32,
    height: u32,
) -> Result<FrameParams<'a>, DecodeError> {
    let mut r = BitReader::new(payload);

    let _show_frame = r.read_bit();

    r.read_uvlc();
    r.read_uvlc();

    let _error_resilient_mode = r.read_bit();
    let _disable_cdf_update = r.read_bit();
    let order_hint = r.read_literal(6);

    let _refresh_frame_flags = r.read_bit();

    let _frame_size_present = r.read_bit();

    parse_tile_info(&mut r, width, height);

    let base_qindex = r.read_literal(8) as u8;

    let _segmentation = r.read_bit();
    let _qm_params = r.read_bit();

    if base_qindex > 0 {
        let _delta_lf_present = r.read_bit();
    }

    if base_qindex > 0 {
        let _loopfilter_0 = r.read_bit();
        let _loopfilter_1 = r.read_bit();
    }

    let _loop_restoration_present = r.read_bit();

    let _frame_type_hints = r.read_literal(2);

    r.skip_to_byte_boundary();

    let tile_data = r.remaining_bytes();

    Ok(FrameParams {
        base_qindex,
        order_hint,
        tile_data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::{write_clk_obu_payload, FrameHeaderConfig};

    #[test]
    fn roundtrip_64x64_qindex50() {
        let config = FrameHeaderConfig {
            width: 64,
            height: 64,
            base_qindex: 50,
            order_hint: 0,
        };
        let tile_data = vec![0xAA, 0xBB];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let params = parse_frame_header(&payload, 64, 64).unwrap();
        assert_eq!(params.base_qindex, 50);
        assert_eq!(params.order_hint, 0);
        assert_eq!(params.tile_data, &[0xAA, 0xBB]);
    }

    #[test]
    fn roundtrip_128x128_order_hint_5() {
        let config = FrameHeaderConfig {
            width: 128,
            height: 128,
            base_qindex: 80,
            order_hint: 5,
        };
        let tile_data = vec![0x01, 0x02, 0x03, 0x04];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let params = parse_frame_header(&payload, 128, 128).unwrap();
        assert_eq!(params.base_qindex, 80);
        assert_eq!(params.order_hint, 5);
        assert_eq!(params.tile_data, &[0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn roundtrip_256x192() {
        let config = FrameHeaderConfig {
            width: 256,
            height: 192,
            base_qindex: 100,
            order_hint: 3,
        };
        let tile_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let params = parse_frame_header(&payload, 256, 192).unwrap();
        assert_eq!(params.base_qindex, 100);
        assert_eq!(params.order_hint, 3);
        assert_eq!(params.tile_data, &[0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE]);
    }

    #[test]
    fn roundtrip_qindex_0() {
        let config = FrameHeaderConfig {
            width: 64,
            height: 64,
            base_qindex: 0,
            order_hint: 7,
        };
        let tile_data = vec![0xFF];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let params = parse_frame_header(&payload, 64, 64).unwrap();
        assert_eq!(params.base_qindex, 0);
        assert_eq!(params.order_hint, 7);
        assert_eq!(params.tile_data, &[0xFF]);
    }
}
