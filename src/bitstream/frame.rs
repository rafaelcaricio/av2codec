use super::writer::BitWriter;

const QINDEX_BITS_UNEXT: u8 = 8;

pub struct FrameHeaderConfig {
    pub base_qindex: u8,
}

pub fn write_clk_obu_payload(config: &FrameHeaderConfig, tile_data: &[u8]) -> Vec<u8> {
    let mut w = BitWriter::new();

    w.write_bit(true); // first_tile_group_in_frame = 1

    write_uncompressed_header(&mut w, config);

    // tile_indices: tiles_log2 = 0 (1 tile), so nothing written

    // Pad frame header to byte boundary (trailing bits without the standard
    // trailing bit pattern - just zero-fill remaining bits in the last byte)
    // NOTE: We do NOT use write_trailing_bits here. The tile group header
    // is simply byte-padded by avm_wb_bytes_written rounding up.
    while !w.byte_aligned() {
        w.write_bit(false);
    }

    let mut result = w.finalize();
    result.extend_from_slice(tile_data);
    result
}

fn write_uncompressed_header(w: &mut BitWriter, config: &FrameHeaderConfig) {
    w.write_uvlc(0); // cur_mfh_id = 0
    w.write_uvlc(0); // seq_header_id (written because cur_mfh_id == 0)

    // single_picture_header_flag=1 skips:
    //   frame type, show_existing_frame, order_hint, primary_ref, etc.

    // CLK with max_mlayer_id=0: refresh_frame_flags = all 1s (implicit)

    // KEY_FRAME path:
    // write_frame_size: frame_size_override=0, not bridge → nothing written
    write_screen_content_params(w);
    write_intrabc_params(w);

    // Not bru_inactive, not bridge, not TIP_FRAME_AS_OUTPUT
    w.write_bit(false); // disable_cdf_update = 0

    write_tile_info(w);

    encode_quantization(w, config.base_qindex);
    encode_segmentation(w);
    encode_qm_params(w);

    // delta_q: base_qindex > 0
    if config.base_qindex > 0 {
        w.write_bit(false); // delta_q_present_flag = 0
    }

    // TCQ: enable_tcq=0 (TCQ_DISABLE), not TCQ_8ST_FR → assert, no bit
    // Parity hiding: enable_parity_hiding=0 → assert, no bit

    // Not all_lossless (base_qindex > 0):
    if config.base_qindex > 0 {
        encode_loopfilter(w);
        // encode_gdf: enable_gdf=0 → returns
        // encode_cdef: enable_cdef=0 → returns
        // encode_restoration_mode: enable_restoration=0 → returns
        // encode_ccso: enable_ccso=0 → not called
    }

    // tx_mode
    w.write_bit(false); // TX_MODE_LARGEST = 0

    // frame_is_intra_only → skip reference_mode, skip_mode, bawp, warpmv

    w.write_literal(0, 2); // reduced_tx_set_used = 0

    // frame_is_intra_only → skip global_motion
    // film_grain_params_present=0 → skip film_grain
}

fn write_screen_content_params(w: &mut BitWriter) {
    // force_screen_content_tools = 2 (SELECT) for single_picture_header_flag=1
    w.write_bit(false); // allow_screen_content_tools = 0
    // Since allow_screen_content_tools=0, force_integer_mv is asserted 0, not written
}

fn write_intrabc_params(w: &mut BitWriter) {
    w.write_bit(false); // allow_intrabc = 0
    // Since allow_intrabc=0, nothing else written
}

fn write_tile_info(w: &mut BitWriter) {
    // No seq_tile_info for first frame → no reuse
    // write_tile_info_max_tile:
    w.write_bit(true); // uniform_spacing = 1
    // For 64x64 frame with sb_size=64: 1x1 SBs
    // min_log2_cols=0, log2_cols=0, max_log2_cols=0
    // ones=0, log2_cols < max_log2_cols → false → no terminator
    // Same for rows
    // tiles.rows * tiles.cols = 1, not > 1 → no tile_id or tile_size_bytes
}

fn encode_quantization(w: &mut BitWriter, base_qindex: u8) {
    w.write_literal(base_qindex as u32, QINDEX_BITS_UNEXT);
    // y_dc_delta_q_enabled=0 → skip y_dc_delta_q
    // uv_dc_delta_q_enabled=0 AND uv_ac_delta_q_enabled=0 → skip all uv delta_q
}

fn encode_segmentation(w: &mut BitWriter) {
    w.write_bit(false); // seg->enabled = 0
}

fn encode_qm_params(w: &mut BitWriter) {
    w.write_bit(false); // using_qmatrix = 0
}

fn encode_loopfilter(w: &mut BitWriter) {
    // Not INTER_FRAME → skip lf_sub_pu
    // Not TIP_FRAME_AS_OUTPUT → continue
    // mfh_deblocking_filter_update_flag=0 for first frame:
    w.write_bit(false); // apply_deblocking_filter[0] = 0
    w.write_bit(false); // apply_deblocking_filter[1] = 0
    // Neither true → no U/V deblocking bits
    // No delta_q bits since deblocking is disabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_byte_aligned() {
        let config = FrameHeaderConfig { base_qindex: 50 };
        let payload = write_clk_obu_payload(&config, &[]);
        assert!(!payload.is_empty());
    }

    #[test]
    fn frame_header_bits_count() {
        let config = FrameHeaderConfig { base_qindex: 50 };
        let mut w = BitWriter::new();
        w.write_bit(true); // first_tile_group
        write_uncompressed_header(&mut w, &config);

        // Expected bits:
        // 1 (first_tile_group)
        // 1 (mfh_id UVLC(0))
        // 1 (seq_header_id UVLC(0))
        // 1 (allow_screen_content)
        // 1 (allow_intrabc)
        // 1 (disable_cdf_update)
        // 1 (uniform_spacing)
        // 8 (base_qindex)
        // 1 (seg enabled)
        // 1 (using_qmatrix)
        // 1 (delta_q_present)
        // 1 (apply_deblocking[0])
        // 1 (apply_deblocking[1])
        // 1 (tx_mode)
        // 2 (reduced_tx_set)
        // Total: 23 bits
        assert_eq!(w.total_bits_written(), 23);
    }

    #[test]
    fn clk_payload_structure() {
        let fake_tile_data = vec![0xAA, 0xBB, 0xCC];
        let config = FrameHeaderConfig { base_qindex: 50 };
        let payload = write_clk_obu_payload(&config, &fake_tile_data);

        // 23 bits → 3 bytes (padded)
        // Then 3 bytes of tile data
        assert_eq!(payload.len(), 3 + 3);

        // Tile data should be at the end
        assert_eq!(&payload[3..], &[0xAA, 0xBB, 0xCC]);
    }
}
