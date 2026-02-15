use super::writer::BitWriter;

const PROFILE_BITS: u8 = 5;
const LEVEL_BITS: u8 = 5;
const DELTA_DCQUANT_BITS: u8 = 5;
const DELTA_DCQUANT_MAX: i32 = 1 << (DELTA_DCQUANT_BITS - 2);
const DELTA_DCQUANT_MIN: i32 = DELTA_DCQUANT_MAX - (1 << DELTA_DCQUANT_BITS) + 1;

pub struct SequenceHeaderConfig {
    pub width: u32,
    pub height: u32,
}

pub fn write_sequence_header(config: &SequenceHeaderConfig) -> Vec<u8> {
    let mut w = BitWriter::new();

    w.write_uvlc(0); // seq_header_id
    w.write_literal(0, PROFILE_BITS); // profile 0
    w.write_bit(true); // single_picture_header_flag
    w.write_literal(0, LEVEL_BITS); // seq_max_level_idx = 0

    let num_bits_width = bits_needed(config.width);
    let num_bits_height = bits_needed(config.height);
    w.write_literal(num_bits_width - 1, 4);
    w.write_literal(num_bits_height - 1, 4);
    w.write_literal(config.width - 1, num_bits_width as u8);
    w.write_literal(config.height - 1, num_bits_height as u8);

    w.write_bit(false); // conf_win_enabled_flag

    w.write_uvlc(0); // chroma_format_idc = CHROMA_FORMAT_420
    w.write_uvlc(1); // bitdepth_lut_idx = 1 (8-bit)

    write_partition_group(&mut w);
    write_segment_group(&mut w);
    write_intra_group(&mut w);
    write_inter_group(&mut w);
    // SCC group: entirely skipped for single_picture_header_flag=1
    write_transform_quant_entropy_group(&mut w);
    write_filter_group(&mut w);

    w.write_bit(false); // seq_tile_info_present_flag
    w.write_bit(false); // film_grain_params_present
    w.write_bit(false); // seq_extension_present_flag

    w.write_trailing_bits();
    w.finalize()
}

fn bits_needed(value: u32) -> u32 {
    if value <= 1 {
        return 1;
    }
    32 - (value - 1).leading_zeros()
}

fn write_quniform(w: &mut BitWriter, n: u16, v: u16) {
    if n <= 1 {
        return;
    }
    assert!(v < n);
    let l = 16 - (n - 1).leading_zeros();
    let m = (1u16 << l) - n;
    if v < m {
        w.write_literal(v as u32, (l - 1) as u8);
    } else {
        w.write_literal((m + ((v - m) >> 1)) as u32, (l - 1) as u8);
        w.write_bit((v - m) & 1 == 1);
    }
}

fn write_partition_group(w: &mut BitWriter) {
    w.write_bit(false); // is_256 = false
    w.write_bit(false); // is_128 = false → sb_size = 64x64
    w.write_bit(false); // enable_sdp = false (!monochrome so written)
    w.write_bit(false); // enable_ext_partitions = false
    w.write_bit(true); // max_pb_aspect_ratio_log2_m1 < 2 = true
    w.write_bit(false); // max_pb_aspect_ratio_log2_m1 = 0
}

fn write_segment_group(w: &mut BitWriter) {
    w.write_bit(false); // enable_ext_seg
    w.write_bit(false); // seq_seg_info_present_flag
}

fn write_intra_group(w: &mut BitWriter) {
    w.write_bit(false); // enable_intra_dip
    w.write_bit(false); // enable_intra_edge_filter
    w.write_bit(false); // enable_mrls
    w.write_bit(false); // enable_cfl_intra
    w.write_literal(0, 2); // cfl_ds_filter_index (always written for !monochrome)
    w.write_bit(false); // enable_mhccp
    w.write_bit(false); // enable_ibp
}

fn write_inter_group(w: &mut BitWriter) {
    // Motion modes block: SKIPPED for single_picture_header_flag=1

    w.write_bit(false); // enable_refmvbank
    w.write_bit(true); // is_drl_reorder_disable = true (DRL_REORDER_DISABLED)
    // No constraint bit since disabled

    // ref_frame_map, dpb, lt_frame_id, drl_bits, allow_frame_drl:
    // all asserted for single_picture_header, not written

    // def_max_bvp_drl_bits: quniform(n=3, v=0) where v = def_max_bvp_drl_bits - MIN
    // MIN_MAX_IBC_DRL_BITS=1, MAX_MAX_IBC_DRL_BITS=3, n=3-1+1=3, v=1-1=0
    write_quniform(w, 3, 0);
    w.write_bit(false); // allow_frame_max_bvp_drl_bits

    // num_same_ref_compound, enable_tip: asserted 0 for single_picture
    // enable_mv_traj: asserted for single_picture

    w.write_bit(false); // enable_bawp

    // enable_cwp, enable_imp_msk_bld: asserted for single_picture
    // enable_lf_sub_pu: asserted for single_picture
    // enable_opfl_refine, enable_refinemv: asserted for single_picture
    // enable_bru, enable_adaptive_mvd, enable_mvd_sign_derive, enable_flex_mvres: asserted
    // enable_global_motion: asserted
    // enable_short_refresh_frame_flags: asserted
}

fn write_transform_quant_entropy_group(w: &mut BitWriter) {
    w.write_bit(false); // enable_fsc = false
    w.write_bit(false); // enable_idtx_intra (written because !fsc)
    w.write_bit(false); // enable_ist
    w.write_bit(false); // enable_inter_ist
    w.write_bit(false); // enable_chroma_dctonly (!monochrome)
    // enable_inter_ddt: asserted for single_picture, not written
    w.write_bit(false); // reduced_tx_part_set
    w.write_bit(false); // enable_cctx (!monochrome)
    w.write_bit(false); // enable_tcq = TCQ_DISABLE
    // Since TCQ_DISABLE: enable_parity_hiding is written
    w.write_bit(false); // enable_parity_hiding
    // enable_avg_cdf, avg_cdf_type: asserted for single_picture, not written
    w.write_bit(false); // separate_uv_delta_q (!monochrome)
    w.write_bit(true); // equal_ac_dc_q = true
    // Since equal_ac_dc_q: dc delta stuff is asserted, not written
    // But for !monochrome, base_uv_ac_delta_q and uv_ac_delta_q_enabled ARE written
    let base_uv_ac_delta_q: i32 = 0;
    let offset = (base_uv_ac_delta_q - DELTA_DCQUANT_MIN) as u32;
    w.write_literal(offset, DELTA_DCQUANT_BITS); // base_uv_ac_delta_q offset
    w.write_bit(false); // uv_ac_delta_q_enabled
}

fn write_filter_group(w: &mut BitWriter) {
    w.write_bit(false); // disable_loopfilters_across_tiles
    w.write_bit(false); // enable_cdef
    w.write_bit(false); // enable_gdf
    w.write_bit(false); // enable_restoration (no lr_tools bits follow)
    w.write_bit(false); // enable_ccso
    // cdef_on_skip_txfm: asserted for single_picture, not written
    w.write_literal(0, 2); // df_par_bits_minus2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::ObuWriter;
    use crate::bitstream::obu::ObuType;

    #[test]
    fn sequence_header_payload_size() {
        let config = SequenceHeaderConfig {
            width: 64,
            height: 64,
        };
        let payload = write_sequence_header(&config);
        assert_eq!(payload.len(), 11, "Expected 11 bytes payload for 64x64");
    }

    #[test]
    fn bits_needed_cases() {
        assert_eq!(bits_needed(1), 1);
        assert_eq!(bits_needed(2), 1);
        assert_eq!(bits_needed(64), 6);
        assert_eq!(bits_needed(128), 7);
        assert_eq!(bits_needed(256), 8);
        assert_eq!(bits_needed(1920), 11);
    }

    #[test]
    fn quniform_n3_v0() {
        let mut w = BitWriter::new();
        write_quniform(&mut w, 3, 0);
        assert_eq!(w.total_bits_written(), 1);
        let data = w.finalize();
        assert_eq!(data, vec![0b0000_0000]);
    }

    #[test]
    fn quniform_n3_v1() {
        let mut w = BitWriter::new();
        write_quniform(&mut w, 3, 1);
        assert_eq!(w.total_bits_written(), 2);
    }

    #[test]
    fn quniform_n3_v2() {
        let mut w = BitWriter::new();
        write_quniform(&mut w, 3, 2);
        assert_eq!(w.total_bits_written(), 2);
    }

    #[test]
    fn sequence_header_starts_with_uvlc_zero() {
        let config = SequenceHeaderConfig {
            width: 64,
            height: 64,
        };
        let payload = write_sequence_header(&config);
        assert_eq!(payload[0] >> 7, 1, "First bit should be 1 (UVLC(0))");
    }

    #[test]
    fn sequence_header_obu_well_formed() {
        let config = SequenceHeaderConfig {
            width: 64,
            height: 64,
        };
        let payload = write_sequence_header(&config);
        let obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &payload);
        assert_eq!(obu[0], 12); // LEB128 size = 1 (header) + 11 (payload)
        assert_eq!((obu[1] >> 2) & 0x1F, 1); // OBU type = SequenceHeader
    }

    #[test]
    fn sequence_header_bit_pattern_64x64() {
        let config = SequenceHeaderConfig {
            width: 64,
            height: 64,
        };
        let payload = write_sequence_header(&config);

        let bits: Vec<u8> = payload
            .iter()
            .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1))
            .collect();

        let mut pos = 0;
        let mut rb = |n: usize| -> u32 {
            let mut val = 0u32;
            for _ in 0..n {
                val = (val << 1) | bits[pos] as u32;
                pos += 1;
            }
            val
        };

        assert_eq!(rb(1), 1); // UVLC(0) = 1
        assert_eq!(rb(5), 0); // profile = 0
        assert_eq!(rb(1), 1); // single_picture_header_flag
        assert_eq!(rb(5), 0); // level = 0
        assert_eq!(rb(4), 5); // num_bits_width - 1
        assert_eq!(rb(4), 5); // num_bits_height - 1
        assert_eq!(rb(6), 63); // max_frame_width - 1
        assert_eq!(rb(6), 63); // max_frame_height - 1
        assert_eq!(rb(1), 0); // conf_win_enabled
        assert_eq!(rb(1), 1); // UVLC(0) for chroma
        assert_eq!(rb(3), 0b010); // UVLC(1) for bitdepth
    }
}
