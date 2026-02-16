use super::writer::BitWriter;

const QINDEX_BITS_UNEXT: u8 = 8;
const ORDER_HINT_BITS: u8 = 6;

pub struct FrameHeaderConfig {
    pub width: u32,
    pub height: u32,
    pub base_qindex: u8,
    pub order_hint: u32,
    pub disable_cdf_update: bool,
}

pub fn write_clk_obu_payload(config: &FrameHeaderConfig, tile_data: &[u8]) -> Vec<u8> {
    let mut w = BitWriter::new();

    w.write_bit(true);

    write_uncompressed_header(&mut w, config);

    while !w.byte_aligned() {
        w.write_bit(false);
    }

    let mut result = w.finalize();
    result.extend_from_slice(tile_data);
    result
}

fn write_uncompressed_header(w: &mut BitWriter, config: &FrameHeaderConfig) {
    w.write_uvlc(0);
    w.write_uvlc(0);

    w.write_bit(true);
    w.write_bit(config.disable_cdf_update);
    w.write_literal(config.order_hint, ORDER_HINT_BITS);

    w.write_bit(false);

    w.write_bit(true);

    write_tile_info(w, config.width, config.height);

    encode_quantization(w, config.base_qindex);
    encode_segmentation(w);
    encode_qm_params(w);

    if config.base_qindex > 0 {
        w.write_bit(false);
    }

    if config.base_qindex > 0 {
        encode_loopfilter(w);
    }

    w.write_bit(false);

    w.write_literal(0, 2);
}

fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

fn write_tile_info(w: &mut BitWriter, width: u32, height: u32) {
    let sb_cols = width / 64;
    let sb_rows = height / 64;

    const MAX_TILE_WIDTH_SB: u32 = 4096 / 64;
    const MAX_TILE_COLS: u32 = 64;
    const MAX_TILE_ROWS: u32 = 64;
    const MAX_TILE_AREA_SB: u32 = (4096 * 2304) / (64 * 64);

    let min_log2_cols = tile_log2(MAX_TILE_WIDTH_SB, sb_cols);
    let max_log2_cols = tile_log2(1, sb_cols.min(MAX_TILE_COLS));
    let min_log2 = tile_log2(MAX_TILE_AREA_SB, sb_cols * sb_rows).max(min_log2_cols);
    let min_log2_rows = if min_log2 > 0 { min_log2 } else { 0 };
    let max_log2_rows = tile_log2(1, sb_rows.min(MAX_TILE_ROWS));

    w.write_bit(true);

    let mut log2_cols = min_log2_cols;
    while log2_cols < max_log2_cols {
        let increment = false;
        w.write_bit(increment);
        if !increment {
            break;
        }
        log2_cols += 1;
    }

    let mut log2_rows = min_log2_rows;
    while log2_rows < max_log2_rows {
        let increment = false;
        w.write_bit(increment);
        if !increment {
            break;
        }
        log2_rows += 1;
    }
}

fn encode_quantization(w: &mut BitWriter, base_qindex: u8) {
    w.write_literal(base_qindex as u32, QINDEX_BITS_UNEXT);
}

fn encode_segmentation(w: &mut BitWriter) {
    w.write_bit(false);
}

fn encode_qm_params(w: &mut BitWriter) {
    w.write_bit(false);
}

fn encode_loopfilter(w: &mut BitWriter) {
    w.write_bit(false);
    w.write_bit(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_64x64() -> FrameHeaderConfig {
        FrameHeaderConfig {
            width: 64,
            height: 64,
            base_qindex: 50,
            order_hint: 0,
            disable_cdf_update: true,
        }
    }

    #[test]
    fn frame_header_byte_aligned() {
        let payload = write_clk_obu_payload(&config_64x64(), &[]);
        assert!(!payload.is_empty());
    }

    #[test]
    fn frame_header_bits_count_64x64() {
        let mut w = BitWriter::new();
        w.write_bit(true);
        write_uncompressed_header(&mut w, &config_64x64());
        assert_eq!(w.total_bits_written(), 30);
    }

    #[test]
    fn frame_header_bits_count_128x128() {
        let config = FrameHeaderConfig {
            width: 128,
            height: 128,
            base_qindex: 50,
            order_hint: 0,
            disable_cdf_update: true,
        };
        let mut w = BitWriter::new();
        w.write_bit(true);
        write_uncompressed_header(&mut w, &config);
        assert_eq!(w.total_bits_written(), 32);
    }

    #[test]
    fn clk_payload_structure() {
        let fake_tile_data = vec![0xAA, 0xBB, 0xCC];
        let payload = write_clk_obu_payload(&config_64x64(), &fake_tile_data);
        assert_eq!(payload.len(), 4 + 3);
        assert_eq!(&payload[4..], &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn tile_log2_basic() {
        assert_eq!(tile_log2(64, 1), 0);
        assert_eq!(tile_log2(64, 2), 0);
        assert_eq!(tile_log2(64, 64), 0);
        assert_eq!(tile_log2(1, 1), 0);
        assert_eq!(tile_log2(1, 2), 1);
        assert_eq!(tile_log2(1, 4), 2);
        assert_eq!(tile_log2(1, 64), 6);
    }

    #[test]
    fn tile_info_64x64_is_one_bit() {
        let mut w = BitWriter::new();
        write_tile_info(&mut w, 64, 64);
        assert_eq!(w.total_bits_written(), 1);
    }

    #[test]
    fn tile_info_128x128_is_three_bits() {
        let mut w = BitWriter::new();
        write_tile_info(&mut w, 128, 128);
        assert_eq!(w.total_bits_written(), 3);
    }

    #[test]
    fn tile_info_256x128_is_three_bits() {
        let mut w = BitWriter::new();
        write_tile_info(&mut w, 256, 128);
        assert_eq!(w.total_bits_written(), 3);
    }
}
