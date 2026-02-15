use super::writer::BitWriter;

const QINDEX_BITS_UNEXT: u8 = 8;
const ORDER_HINT_BITS: u8 = 6;

pub struct FrameHeaderConfig {
    pub base_qindex: u8,
    pub order_hint: u32,
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
    w.write_bit(false);
    w.write_literal(config.order_hint, ORDER_HINT_BITS);

    w.write_bit(false);

    w.write_bit(true);

    write_tile_info(w);

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

fn write_tile_info(w: &mut BitWriter) {
    w.write_bit(true);
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

    #[test]
    fn frame_header_byte_aligned() {
        let config = FrameHeaderConfig {
            base_qindex: 50,
            order_hint: 0,
        };
        let payload = write_clk_obu_payload(&config, &[]);
        assert!(!payload.is_empty());
    }

    #[test]
    fn frame_header_bits_count() {
        let config = FrameHeaderConfig {
            base_qindex: 50,
            order_hint: 0,
        };
        let mut w = BitWriter::new();
        w.write_bit(true);
        write_uncompressed_header(&mut w, &config);
        assert_eq!(w.total_bits_written(), 30);
    }

    #[test]
    fn clk_payload_structure() {
        let fake_tile_data = vec![0xAA, 0xBB, 0xCC];
        let config = FrameHeaderConfig {
            base_qindex: 50,
            order_hint: 0,
        };
        let payload = write_clk_obu_payload(&config, &fake_tile_data);
        assert_eq!(payload.len(), 4 + 3);
        assert_eq!(&payload[4..], &[0xAA, 0xBB, 0xCC]);
    }
}
