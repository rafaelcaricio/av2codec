use super::bitreader::BitReader;
use super::sequence::SequenceParams;
use super::DecodeError;

fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

fn parse_tile_info(r: &mut BitReader, width: u32, height: u32, sb_size: u32) {
    let _uniform_spacing = r.read_bit();

    let mi_cols = (width + 3) / 4;
    let mi_rows = (height + 3) / 4;
    let mib_size = sb_size / 4;
    let sb_cols = (mi_cols + mib_size - 1) / mib_size;
    let sb_rows = (mi_rows + mib_size - 1) / mib_size;

    let max_tile_width_sb = 4096 / sb_size;
    let max_tile_cols = 64u32;
    let max_tile_area_sb = (4096 * 2304) / (sb_size * sb_size);

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

pub struct FrameParams<'a> {
    pub base_qindex: u8,
    pub order_hint: u32,
    pub tile_data: &'a [u8],
    pub disable_cdf_update: bool,
    pub tx_mode_select: bool,
    pub reduced_tx_set_used: u8,
    pub allow_intrabc: bool,
    pub allow_screen_content_tools: bool,
    pub cdef_frame_enable: bool,
    pub nb_cdef_strengths: u32,
    pub ccso_frame_enable: bool,
    pub ccso_enable: [bool; 3],
    pub delta_q_present: bool,
    pub delta_q_res: u32,
}

pub fn parse_frame_header<'a>(
    payload: &'a [u8],
    seq: &SequenceParams,
) -> Result<FrameParams<'a>, DecodeError> {
    if seq.single_picture_header_flag {
        parse_frame_header_sph1(payload, seq)
    } else {
        parse_frame_header_sph0(payload, seq)
    }
}

fn read_delta_q(r: &mut BitReader) -> i32 {
    if r.read_bit() {
        let magnitude = r.read_literal(6);
        let sign = r.read_bit();
        if sign { -(magnitude as i32) } else { magnitude as i32 }
    } else {
        0
    }
}

fn read_uniform(r: &mut BitReader, n: u32) -> u32 {
    if n <= 1 {
        return 0;
    }
    let l = 32 - (n - 1).leading_zeros();
    let m = (1u32 << l) - n;
    let v = r.read_literal((l - 1) as u8);
    if v < m {
        v
    } else {
        let extra = r.read_literal(1);
        (v << 1) - m + extra
    }
}

fn parse_quantization(r: &mut BitReader, seq: &SequenceParams) -> u8 {
    let base_qindex = r.read_literal(8) as u8;

    if seq.y_dc_delta_q_enabled {
        let _y_dc_delta_q = read_delta_q(r);
    }
    if seq.uv_dc_delta_q_enabled || seq.uv_ac_delta_q_enabled {
        let mut diff_uv_delta = false;
        if seq.separate_uv_delta_q {
            diff_uv_delta = r.read_bit();
        }
        if !seq.equal_ac_dc_q && seq.uv_dc_delta_q_enabled {
            let _u_dc_delta_q = read_delta_q(r);
        }
        if seq.uv_ac_delta_q_enabled {
            let _u_ac_delta_q = read_delta_q(r);
        }
        if diff_uv_delta {
            if !seq.equal_ac_dc_q && seq.uv_dc_delta_q_enabled {
                let _v_dc_delta_q = read_delta_q(r);
            }
            if seq.uv_ac_delta_q_enabled {
                let _v_ac_delta_q = read_delta_q(r);
            }
        }
    }

    base_qindex
}

fn parse_deblocking(r: &mut BitReader, df_par_bits: u32) {
    let apply0 = r.read_bit();
    let apply1 = r.read_bit();

    let (apply_u, apply_v) = if apply0 || apply1 {
        (r.read_bit(), r.read_bit())
    } else {
        (false, false)
    };

    if apply0 {
        let has_delta = r.read_bit();
        if has_delta {
            let _delta = r.read_literal(df_par_bits as u8);
        }
    }
    if apply1 {
        let has_delta = r.read_bit();
        if has_delta {
            let _delta = r.read_literal(df_par_bits as u8);
        }
    }
    if apply_u {
        let has_delta = r.read_bit();
        if has_delta {
            let _delta = r.read_literal(df_par_bits as u8);
        }
    }
    if apply_v {
        let has_delta = r.read_bit();
        if has_delta {
            let _delta = r.read_literal(df_par_bits as u8);
        }
    }
}

fn parse_cdef_params(r: &mut BitReader, enable_cdef_on_skip_txfm: u32) -> u32 {
    let _damping = r.read_literal(2);
    let nb_strengths = r.read_literal(3) + 1;

    if enable_cdef_on_skip_txfm == 2 {
        let _cdef_on_skip_txfm = r.read_bit();
    }

    for _ in 0..nb_strengths {
        let less_4 = r.read_bit();
        if less_4 {
            let _y_strength = r.read_literal(2);
        } else {
            let _y_strength = r.read_literal(6);
        }

        let uv_less_4 = r.read_bit();
        if uv_less_4 {
            let _uv_strength = r.read_literal(2);
        } else {
            let _uv_strength = r.read_literal(6);
        }
    }

    nb_strengths
}

const CCSO_QUANT_SZ: [[u16; 4]; 4] = [
    [16, 8, 32, 0],
    [56, 40, 64, 128],
    [48, 24, 96, 192],
    [80, 112, 160, 256],
];

fn read_ccso_offset_idx(r: &mut BitReader) -> usize {
    let mut idx = 0;
    for _ in 0..7 {
        if !r.read_bit() {
            break;
        }
        idx += 1;
    }
    idx
}

fn parse_ccso_params(r: &mut BitReader) -> [bool; 3] {
    let mut ccso_enable_planes = [false; 3];
    for plane in 0..3 {
        let ccso_enable = r.read_bit();
        ccso_enable_planes[plane] = ccso_enable;
        if ccso_enable {
            let ccso_bo_only = r.read_bit();
            let scale_idx = r.read_literal(2) as usize;

            let (max_band_log2, num_edge_intervals) = if ccso_bo_only {
                let mbl = r.read_literal(3);
                (mbl, 1usize)
            } else {
                let quant_idx = r.read_literal(2) as usize;
                let _ext_filter_support = r.read_literal(3);
                let edge_clf = if CCSO_QUANT_SZ[scale_idx][quant_idx] != 0 {
                    r.read_bit() as usize
                } else {
                    0
                };
                let mbl = r.read_literal(2);
                let max_edge_interval = if edge_clf == 0 { 3 } else { 2 };
                (mbl, max_edge_interval)
            };

            let max_band = 1u32 << max_band_log2;

            for _d0 in 0..num_edge_intervals {
                for _d1 in 0..num_edge_intervals {
                    for _band in 0..max_band {
                        let _offset_idx = read_ccso_offset_idx(r);
                    }
                }
            }
        }
    }
    ccso_enable_planes
}

fn parse_frame_header_sph0<'a>(
    payload: &'a [u8],
    seq: &SequenceParams,
) -> Result<FrameParams<'a>, DecodeError> {
    let mut r = BitReader::new(payload);

    let _first_tile_group = r.read_bit();

    let mfh_id = r.read_uvlc();
    if mfh_id == 0 {
        let _seq_header_id = r.read_uvlc();
    }

    let _long_term_id = r.read_literal(seq.number_of_bits_for_lt_frame_id as u8);

    let immediate_output_picture = r.read_bit();
    if !immediate_output_picture {
        let _implicit_output_picture = r.read_bit();
    }

    let frame_size_override_flag = r.read_bit();

    let order_hint = if seq.order_hint_bits > 0 {
        r.read_literal(seq.order_hint_bits as u8)
    } else {
        0
    };

    if frame_size_override_flag {
        let _override_width = r.read_literal(16);
        let _override_height = r.read_literal(16);
    }

    let allow_screen_content_tools = if seq.force_screen_content_tools == 2 {
        r.read_bit()
    } else {
        seq.force_screen_content_tools != 0
    };


    if allow_screen_content_tools && seq.force_integer_mv == 2 {
        let _cur_frame_force_integer_mv = r.read_bit();
    }

    let allow_intrabc = r.read_bit();
    if allow_intrabc {
        let allow_global_intrabc = r.read_bit();
        if allow_global_intrabc {
            let _allow_local_intrabc = r.read_bit();
        }
        if seq.allow_frame_max_bvp_drl_bits {
            let _max_bvp_drl_bits = read_uniform(&mut r, 4);
        }
    }

    let disable_cdf_update = r.read_bit();

    parse_tile_info(&mut r, seq.width, seq.height, seq.sb_size);

    let base_qindex = parse_quantization(&mut r, seq);

    let _seg_enabled = r.read_bit();
    let _using_qmatrix = r.read_bit();

    let mut delta_q_present = false;
    let mut delta_q_res = 0u32;
    if base_qindex > 0 {
        delta_q_present = r.read_bit();
        if delta_q_present {
            delta_q_res = r.read_literal(2);
        }
    }

    let coded_lossless = base_qindex == 0;

    let tcq_mode = if coded_lossless {
        false
    } else if seq.enable_tcq >= 2 {
        r.read_bit()
    } else {
        seq.enable_tcq == 1
    };

    if !coded_lossless && seq.enable_parity_hiding && !tcq_mode {
        let _allow_parity_hiding = r.read_bit();
    }

    if !coded_lossless {
        parse_deblocking(&mut r, seq.df_par_bits);
    }

    if !coded_lossless && seq.enable_gdf {
        let gdf_mode = r.read_bit();
        if gdf_mode {
            let gdf_block_num = {
                let gdf_block_size = seq.sb_size;
                ((seq.width + gdf_block_size - 1) / gdf_block_size)
                    * ((seq.height + gdf_block_size - 1) / gdf_block_size)
            };
            if gdf_block_num > 1 {
                let _gdf_mode_inc = r.read_bit();
            }
            let _gdf_pic_qp_idx = r.read_literal(2);
            let _gdf_pic_scale_idx = r.read_literal(2);
        }
    }

    let mut cdef_frame_enable = false;
    let mut nb_cdef_strengths = 0u32;
    if !coded_lossless && seq.enable_cdef {
        cdef_frame_enable = r.read_bit();
        if cdef_frame_enable {
            nb_cdef_strengths = parse_cdef_params(&mut r, seq.enable_cdef_on_skip_txfm);
        }
    }

    if !coded_lossless && seq.enable_restoration {
        for plane in 0..3 {
            let _restoration_type = read_uniform(&mut r, seq.lr_frame_tools_count[plane]);
        }
    }

    let mut ccso_frame_enable = false;
    let mut ccso_enable = [false; 3];
    if !coded_lossless && seq.enable_ccso {
        ccso_frame_enable = r.read_bit();
        if ccso_frame_enable {
            ccso_enable = parse_ccso_params(&mut r);
        }
    }

    let tx_mode_select = if !coded_lossless {
        r.read_bit()
    } else {
        false
    };

    let reduced_tx_set_used = r.read_literal(2) as u8;

    if seq.film_grain_params_present && immediate_output_picture {
        let apply_grain = r.read_bit();
        if apply_grain {
            let _fgm_id = r.read_literal(3);
            let _random_seed = r.read_literal(16);
        }
    }

    let header_bits = r.total_bits_read();
    r.skip_to_byte_boundary();

    let tile_data = if r.has_remaining() {
        r.remaining_bytes()
    } else {
        &[]
    };
    eprintln!("[FH] header_bits={} tile_data_len={} first8={:02x?}",
        header_bits, tile_data.len(), &tile_data[..tile_data.len().min(8)]);

    Ok(FrameParams {
        base_qindex,
        order_hint,
        tile_data,
        disable_cdf_update,
        tx_mode_select,
        reduced_tx_set_used,
        allow_intrabc,
        allow_screen_content_tools,
        cdef_frame_enable,
        nb_cdef_strengths,
        ccso_frame_enable,
        ccso_enable,
        delta_q_present,
        delta_q_res,
    })
}

fn parse_frame_header_sph1<'a>(
    payload: &'a [u8],
    seq: &SequenceParams,
) -> Result<FrameParams<'a>, DecodeError> {
    let mut r = BitReader::new(payload);

    let _is_first_tile_group = r.read_bit();

    let mfh_id = r.read_uvlc();
    if mfh_id == 0 {
        let _seq_header_id = r.read_uvlc();
    }

    let allow_screen_content_tools = r.read_bit();
    if allow_screen_content_tools {
        let _cur_frame_force_integer_mv = r.read_bit();
    }

    let allow_intrabc = r.read_bit();
    if allow_intrabc {
        let allow_global_intrabc = r.read_bit();
        if allow_global_intrabc {
            let _allow_local_intrabc = r.read_bit();
        }
        if seq.allow_frame_max_bvp_drl_bits {
            let _max_bvp_drl_bits = read_uniform(&mut r, 4);
        }
    }

    let disable_cdf_update = r.read_bit();

    parse_tile_info(&mut r, seq.width, seq.height, seq.sb_size);

    let base_qindex = parse_quantization(&mut r, seq);

    let _seg_enabled = r.read_bit();
    let _using_qmatrix = r.read_bit();

    let mut delta_q_present = false;
    let mut delta_q_res = 0u32;
    if base_qindex > 0 {
        delta_q_present = r.read_bit();
        if delta_q_present {
            delta_q_res = r.read_literal(2);
        }
    }

    let coded_lossless = base_qindex == 0;

    let tcq_mode = if coded_lossless {
        false
    } else if seq.enable_tcq >= 2 {
        r.read_bit()
    } else {
        seq.enable_tcq == 1
    };

    if !coded_lossless && seq.enable_parity_hiding && !tcq_mode {
        let _allow_parity_hiding = r.read_bit();
    }

    if !coded_lossless {
        parse_deblocking(&mut r, seq.df_par_bits);
    }

    let mut gdf_mode = 0u32;
    if !coded_lossless && seq.enable_gdf {
        gdf_mode = 1;
        let gdf_block_num = {
            let gdf_block_size = seq.sb_size;
            ((seq.width + gdf_block_size - 1) / gdf_block_size)
                * ((seq.height + gdf_block_size - 1) / gdf_block_size)
        };
        if gdf_block_num > 1 {
            gdf_mode += r.read_bit() as u32;
        }
        let _gdf_pic_qp_idx = r.read_literal(2);
        let _gdf_pic_scale_idx = r.read_literal(2);
    }

    let mut cdef_frame_enable = false;
    let mut nb_cdef_strengths = 0u32;
    if !coded_lossless && seq.enable_cdef {
        cdef_frame_enable = true;
        nb_cdef_strengths = parse_cdef_params(&mut r, seq.enable_cdef_on_skip_txfm);
    }

    if !coded_lossless && seq.enable_restoration {
        for plane in 0..3 {
            let _restoration_type = read_uniform(&mut r, seq.lr_frame_tools_count[plane]);
        }
    }

    let mut ccso_frame_enable = false;
    let mut ccso_enable = [false; 3];
    if !coded_lossless && seq.enable_ccso {
        ccso_frame_enable = true;
        ccso_enable = parse_ccso_params(&mut r);
    }

    let tx_mode_select = if !coded_lossless {
        r.read_bit()
    } else {
        false
    };

    let reduced_tx_set_used = r.read_literal(2) as u8;

    let header_bits = r.total_bits_read();
    r.skip_to_byte_boundary();

    let tile_data = if r.has_remaining() {
        r.remaining_bytes()
    } else {
        &[]
    };
    eprintln!("[FH_SPH1] header_bits={} tile_data_len={} first8={:02x?} delta_q_present={} ccso_enable={:?} nb_cdef={} gdf_mode={} reduced_tx_set={}",
        header_bits, tile_data.len(), &tile_data[..tile_data.len().min(8)],
        delta_q_present, ccso_enable, nb_cdef_strengths, gdf_mode, reduced_tx_set_used);

    Ok(FrameParams {
        base_qindex,
        order_hint: 0,
        tile_data,
        disable_cdf_update,
        tx_mode_select,
        reduced_tx_set_used,
        allow_intrabc,
        allow_screen_content_tools,
        cdef_frame_enable,
        nb_cdef_strengths,
        ccso_frame_enable,
        ccso_enable,
        delta_q_present,
        delta_q_res,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::{write_clk_obu_payload, FrameHeaderConfig};

    fn sph0_seq(width: u32, height: u32) -> SequenceParams {
        SequenceParams {
            width,
            height,
            single_picture_header_flag: false,
            sb_size: 64,
            enable_sdp: false,
            enable_cfl_intra: false,
            enable_mrls: false,
            enable_intra_dip: false,
            order_hint_bits: 6,
            number_of_bits_for_lt_frame_id: 3,
            force_screen_content_tools: 0,
            force_integer_mv: 2,
            allow_frame_max_bvp_drl_bits: false,
            enable_fsc: false,
            enable_idtx_intra: false,
            enable_ist: false,
            enable_cctx: false,
            reduced_tx_part_set: false,
            enable_tcq: 0,
            enable_parity_hiding: false,
            separate_uv_delta_q: false,
            equal_ac_dc_q: true,
            y_dc_delta_q_enabled: false,
            uv_dc_delta_q_enabled: false,
            uv_ac_delta_q_enabled: false,
            enable_cdef: false,
            enable_gdf: false,
            enable_restoration: false,
            enable_ccso: false,
            enable_cdef_on_skip_txfm: 0,
            lr_frame_tools_count: [0, 0, 0],
            df_par_bits: 2,
            film_grain_params_present: false,
        }
    }

    #[test]
    fn roundtrip_64x64_qindex50() {
        let config = FrameHeaderConfig {
            width: 64,
            height: 64,
            base_qindex: 50,
            order_hint: 0,
            disable_cdf_update: true,
        };
        let tile_data = vec![0xAA, 0xBB];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let seq = sph0_seq(64, 64);
        let params = parse_frame_header(&payload, &seq).unwrap();
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
            disable_cdf_update: true,
        };
        let tile_data = vec![0x01, 0x02, 0x03, 0x04];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let seq = sph0_seq(128, 128);
        let params = parse_frame_header(&payload, &seq).unwrap();
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
            disable_cdf_update: true,
        };
        let tile_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let seq = sph0_seq(256, 192);
        let params = parse_frame_header(&payload, &seq).unwrap();
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
            disable_cdf_update: true,
        };
        let tile_data = vec![0xFF];
        let payload = write_clk_obu_payload(&config, &tile_data);
        let seq = sph0_seq(64, 64);
        let params = parse_frame_header(&payload, &seq).unwrap();
        assert_eq!(params.base_qindex, 0);
        assert_eq!(params.order_hint, 7);
        assert_eq!(params.tile_data, &[0xFF]);
    }
}
