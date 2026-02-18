use crate::cdf_tables::*;
use crate::cdf_tables_qctx::*;
use crate::block::TxSize;

fn make_cdf(icdf: &[u16], para: u16) -> Vec<u16> {
    let mut cdf = icdf.to_vec();
    cdf.push(0);
    cdf.push(para);
    cdf
}

pub fn get_q_ctx(qindex: u8) -> usize {
    if qindex <= 90 {
        0
    } else if qindex <= 140 {
        1
    } else if qindex <= 190 {
        2
    } else {
        3
    }
}

const DO_SPLIT_ICDF_LEGACY: [u16; 2] = [12276, 0];

pub struct FrameCdfContext {
    pub do_split: Vec<u16>,
    pub y_mode_set: Vec<u16>,
    pub y_mode_idx: [Vec<u16>; 3],
    pub y_mode_idx_offset: [Vec<u16>; 3],
    pub uv_mode: [Vec<u16>; 2],
    pub cfl: [Vec<u16>; 3],
    pub cfl_index: Vec<u16>,
    pub cfl_sign: Vec<u16>,
    pub cfl_alpha: [Vec<u16>; 6],
    pub txb_skip_y: [[Vec<u16>; 10]; 5],
    pub txb_skip_u: [Vec<u16>; 3],
    pub txb_skip_v: [Vec<u16>; 3],
    pub txb_skip_v_u_nz: [Vec<u16>; 3],
    pub dc_sign_y: [Vec<u16>; 3],
    pub dc_sign_uv: Vec<u16>,
    pub eob_multi_1024: [Vec<u16>; 3],
    pub eob_extra: Vec<u16>,
    pub coeff_base_lf_y: [[[Vec<u16>; 2]; 33]; 5],
    pub coeff_base_lf_eob_y: [[Vec<u16>; 4]; 5],
    pub coeff_base_y: [[[Vec<u16>; 2]; 20]; 5],
    pub coeff_base_eob_y: [[Vec<u16>; 4]; 5],
    pub coeff_br_lf_y: [Vec<u16>; 14],
    pub coeff_br_y: [Vec<u16>; 7],
    pub coeff_base_lf_uv: [[Vec<u16>; 2]; 12],
    pub coeff_base_lf_eob_uv: [Vec<u16>; 4],
    pub coeff_base_uv: [[Vec<u16>; 2]; 12],
    pub coeff_base_eob_uv: [Vec<u16>; 4],
    pub coeff_br_lf_uv: [Vec<u16>; 4],
    pub coeff_br_uv: [Vec<u16>; 4],
    pub do_split_part: [[Vec<u16>; 64]; 2],
    pub do_square_split: [[Vec<u16>; 8]; 2],
    pub rect_type: [[Vec<u16>; 64]; 2],
    pub do_ext_partition: [[Vec<u16>; 64]; 2],
    pub do_uneven_4way: [[Vec<u16>; 64]; 2],
    pub eob_multi_16: [Vec<u16>; 3],
    pub eob_multi_32: [Vec<u16>; 3],
    pub eob_multi_64: [Vec<u16>; 3],
    pub eob_multi_128: [Vec<u16>; 3],
    pub eob_multi_256: [Vec<u16>; 3],
    pub eob_multi_512: [Vec<u16>; 3],
    pub fsc_mode: [[Vec<u16>; 6]; 4],
    pub mrl_index: [Vec<u16>; 3],
    pub multi_line_mrl: [Vec<u16>; 3],
    pub txfm_do_partition: [[[Vec<u16>; 9]; 2]; 2],
    pub txfm_4way_partition_type: [[[Vec<u16>; 14]; 2]; 2],
    pub txfm_2or3_way_partition_type: [[[Vec<u16>; 2]; 2]; 2],
    pub intrabc: [Vec<u16>; 3],
    pub cdef_strength_index0: [Vec<u16>; 4],
    pub ccso: [[Vec<u16>; 4]; 3],
    pub intra_ext_tx: [Vec<u16>; 4],
    pub intra_ext_tx_reduced: Vec<u16>,
    pub intra_ext_tx_short_side: [Vec<u16>; 4],
    pub tx_ext_32_intra: Vec<u16>,
    pub stx: [Vec<u16>; 5],
    pub most_probable_stx_set: Vec<u16>,
    pub most_probable_stx_set_adst_adst: Vec<u16>,
    pub cctx_type: Vec<u16>,
    pub palette_y_mode: Vec<u16>,
    pub palette_y_size: Vec<u16>,
    pub identity_row_y: [Vec<u16>; 4],
    pub palette_y_color_index: [[Vec<u16>; 5]; 7],
}

impl Default for FrameCdfContext {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameCdfContext {
    pub fn new() -> Self {
        Self {
            do_split: make_cdf(&DO_SPLIT_ICDF_LEGACY, 0),
            y_mode_set: make_cdf(&Y_MODE_SET_ICDF, 93),
            y_mode_idx: [
                make_cdf(&Y_MODE_IDX_ICDF[0], 5),
                make_cdf(&Y_MODE_IDX_ICDF[1], 0),
                make_cdf(&Y_MODE_IDX_ICDF[2], 0),
            ],
            y_mode_idx_offset: [
                make_cdf(&Y_MODE_IDX_OFFSET_ICDF[0], 6),
                make_cdf(&Y_MODE_IDX_OFFSET_ICDF[1], 75),
                make_cdf(&Y_MODE_IDX_OFFSET_ICDF[2], 75),
            ],
            uv_mode: [
                make_cdf(&UV_MODE_ICDF[0], 31),
                make_cdf(&UV_MODE_ICDF[1], 30),
            ],
            cfl: [
                make_cdf(&CFL_ICDF[0], 30),
                make_cdf(&CFL_ICDF[1], 6),
                make_cdf(&CFL_ICDF[2], 0),
            ],
            cfl_index: make_cdf(&CFL_INDEX_ICDF, 30),
            cfl_sign: make_cdf(&CFL_SIGN_ICDF, 62),
            cfl_alpha: [
                make_cdf(&CFL_ALPHA_ICDF[0], 62),
                make_cdf(&CFL_ALPHA_ICDF[1], 62),
                make_cdf(&CFL_ALPHA_ICDF[2], 62),
                make_cdf(&CFL_ALPHA_ICDF[3], 62),
                make_cdf(&CFL_ALPHA_ICDF[4], 62),
                make_cdf(&CFL_ALPHA_ICDF[5], 62),
            ],
            txb_skip_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&TXB_SKIP_Y_ICDF[t][c], 0))
            }),
            txb_skip_u: std::array::from_fn(|i| make_cdf(&TXB_SKIP_U_CTX_ICDF[i], 0)),
            txb_skip_v: std::array::from_fn(|i| make_cdf(&TXB_SKIP_V_CTX_ICDF[i], 0)),
            txb_skip_v_u_nz: std::array::from_fn(|i| make_cdf(&TXB_SKIP_V_U_NONZERO_CTX_ICDF[i], 0)),
            dc_sign_y: std::array::from_fn(|i| make_cdf(&DC_SIGN_Y_ICDF[i], 0)),
            dc_sign_uv: make_cdf(&DC_SIGN_UV_ICDF, 0),
            eob_multi_1024: std::array::from_fn(|i| make_cdf(&EOB_MULTI_1024_ICDF[i], 0)),
            eob_extra: make_cdf(&EOB_EXTRA_ICDF, 0),
            coeff_base_lf_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| [
                    make_cdf(&COEFF_BASE_LF_Y_ICDF[t][c], 0),
                    make_cdf(&COEFF_BASE_LF_Y_ICDF[t][c], 0),
                ])
            }),
            coeff_base_lf_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_LF_EOB_Y_ICDF[t][c], 0))
            }),
            coeff_base_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| [
                    make_cdf(&COEFF_BASE_Y_ICDF[t][c], 0),
                    make_cdf(&COEFF_BASE_Y_ICDF[t][c], 0),
                ])
            }),
            coeff_base_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_EOB_Y_ICDF[t][c], 0))
            }),
            coeff_br_lf_y: std::array::from_fn(|i| make_cdf(&COEFF_BR_LF_Y_ICDF[i], 0)),
            coeff_br_y: std::array::from_fn(|i| make_cdf(&COEFF_BR_Y_ICDF[i], 0)),
            coeff_base_lf_uv: std::array::from_fn(|i| [
                make_cdf(&COEFF_BASE_LF_UV_ICDF[i], 0),
                make_cdf(&COEFF_BASE_LF_UV_ICDF[i], 0),
            ]),
            coeff_base_lf_eob_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_LF_EOB_UV_ICDF[i], 0)),
            coeff_base_uv: std::array::from_fn(|i| [
                make_cdf(&COEFF_BASE_UV_ICDF[i], 0),
                make_cdf(&COEFF_BASE_UV_ICDF[i], 0),
            ]),
            coeff_base_eob_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_EOB_UV_ICDF[i], 0)),
            coeff_br_lf_uv: std::array::from_fn(|i| make_cdf(&COEFF_BR_LF_UV_ICDF[i], 0)),
            coeff_br_uv: std::array::from_fn(|i| make_cdf(&COEFF_BR_UV_ICDF[i], 0)),
            do_split_part: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_SPLIT_PART_ICDF[p][c], DO_SPLIT_PART_PARA[p][c]))
            }),
            do_square_split: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_SQUARE_SPLIT_ICDF[p][c], DO_SQUARE_SPLIT_PARA[p][c]))
            }),
            rect_type: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&RECT_TYPE_ICDF[p][c], RECT_TYPE_PARA[p][c]))
            }),
            do_ext_partition: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_EXT_PARTITION_ICDF[p][c], DO_EXT_PARTITION_PARA[p][c]))
            }),
            do_uneven_4way: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_UNEVEN_4WAY_ICDF[p][c], DO_UNEVEN_4WAY_PARA[p][c]))
            }),
            eob_multi_16: std::array::from_fn(|i| make_cdf(&EOB_MULTI_16_ICDF[i], 0)),
            eob_multi_32: std::array::from_fn(|i| make_cdf(&EOB_MULTI_32_ICDF[i], 0)),
            eob_multi_64: std::array::from_fn(|i| make_cdf(&EOB_MULTI_64_ICDF[i], 0)),
            eob_multi_128: std::array::from_fn(|i| make_cdf(&EOB_MULTI_128_ICDF[i], 0)),
            eob_multi_256: std::array::from_fn(|i| make_cdf(&EOB_MULTI_256_ICDF[i], 0)),
            eob_multi_512: std::array::from_fn(|i| make_cdf(&EOB_MULTI_512_ICDF[i], 0)),
            fsc_mode: std::array::from_fn(|ctx| {
                std::array::from_fn(|bg| make_cdf(&FSC_MODE_ICDF[ctx][bg], FSC_MODE_PARA[ctx][bg]))
            }),
            mrl_index: std::array::from_fn(|i| make_cdf(&MRL_INDEX_ICDF[i], MRL_INDEX_PARA[i])),
            multi_line_mrl: std::array::from_fn(|i| make_cdf(&MULTI_LINE_MRL_ICDF[i], MULTI_LINE_MRL_PARA[i])),
            txfm_do_partition: std::array::from_fn(|fsc| {
                std::array::from_fn(|inter| {
                    std::array::from_fn(|g| make_cdf(&TXFM_DO_PARTITION_ICDF[fsc][inter][g], TXFM_DO_PARTITION_PARA[fsc][inter][g]))
                })
            }),
            txfm_4way_partition_type: std::array::from_fn(|fsc| {
                std::array::from_fn(|inter| {
                    std::array::from_fn(|g| make_cdf(&TXFM_4WAY_PARTITION_TYPE_ICDF[fsc][inter][g], TXFM_4WAY_PARTITION_TYPE_PARA[fsc][inter][g]))
                })
            }),
            txfm_2or3_way_partition_type: std::array::from_fn(|fsc| {
                std::array::from_fn(|inter| {
                    std::array::from_fn(|g| make_cdf(&TXFM_2OR3_WAY_PARTITION_TYPE_ICDF[fsc][inter][g], TXFM_2OR3_WAY_PARTITION_TYPE_PARA[fsc][inter][g]))
                })
            }),
            intrabc: std::array::from_fn(|i| make_cdf(&INTRABC_ICDF[i], INTRABC_PARA[i])),
            cdef_strength_index0: std::array::from_fn(|i| make_cdf(&CDEF_STRENGTH_INDEX0_ICDF[i], CDEF_STRENGTH_INDEX0_PARA[i])),
            ccso: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&CCSO_ICDF[p][c], CCSO_PARA[p][c]))
            }),
            intra_ext_tx: std::array::from_fn(|i| make_cdf(&INTRA_EXT_TX_ICDF[i], INTRA_EXT_TX_PARA[i])),
            intra_ext_tx_reduced: make_cdf(&INTRA_EXT_TX_REDUCED_ICDF, INTRA_EXT_TX_REDUCED_PARA),
            intra_ext_tx_short_side: std::array::from_fn(|i| make_cdf(&INTRA_EXT_TX_SHORT_SIDE_ICDF[i], INTRA_EXT_TX_SHORT_SIDE_PARA[i])),
            tx_ext_32_intra: make_cdf(&TX_EXT_32_INTRA_ICDF, TX_EXT_32_INTRA_PARA),
            stx: std::array::from_fn(|i| make_cdf(&STX_ICDF[i], STX_PARA[i])),
            most_probable_stx_set: make_cdf(&MOST_PROBABLE_STX_SET_ICDF, MOST_PROBABLE_STX_SET_PARA),
            most_probable_stx_set_adst_adst: make_cdf(&MOST_PROBABLE_STX_SET_ADST_ADST_ICDF, MOST_PROBABLE_STX_SET_ADST_ADST_PARA),
            cctx_type: make_cdf(&CCTX_TYPE_ICDF, CCTX_TYPE_PARA),
            palette_y_mode: make_cdf(&PALETTE_Y_MODE_ICDF, PALETTE_Y_MODE_PARA),
            palette_y_size: make_cdf(&PALETTE_Y_SIZE_ICDF, PALETTE_Y_SIZE_PARA),
            identity_row_y: std::array::from_fn(|i| make_cdf(&IDENTITY_ROW_Y_ICDF[i], IDENTITY_ROW_Y_PARA[i])),
            palette_y_color_index: std::array::from_fn(|ps| {
                std::array::from_fn(|ctx| {
                    let n_colors = PALETTE_COLORS_PER_SIZE[ps];
                    make_cdf(&PALETTE_Y_COLOR_INDEX_ICDF[ps][ctx][..n_colors], PALETTE_Y_COLOR_INDEX_PARA[ps][ctx])
                })
            }),
        }
    }

    pub fn new_with_qindex(qindex: u8) -> Self {
        let q = get_q_ctx(qindex);
        Self {
            do_split: make_cdf(&DO_SPLIT_ICDF_LEGACY, 0),
            y_mode_set: make_cdf(&Y_MODE_SET_ICDF, 93),
            y_mode_idx: [
                make_cdf(&Y_MODE_IDX_ICDF[0], 5),
                make_cdf(&Y_MODE_IDX_ICDF[1], 0),
                make_cdf(&Y_MODE_IDX_ICDF[2], 0),
            ],
            y_mode_idx_offset: [
                make_cdf(&Y_MODE_IDX_OFFSET_ICDF[0], 6),
                make_cdf(&Y_MODE_IDX_OFFSET_ICDF[1], 75),
                make_cdf(&Y_MODE_IDX_OFFSET_ICDF[2], 75),
            ],
            uv_mode: [
                make_cdf(&UV_MODE_ICDF[0], 31),
                make_cdf(&UV_MODE_ICDF[1], 30),
            ],
            cfl: [
                make_cdf(&CFL_ICDF[0], 30),
                make_cdf(&CFL_ICDF[1], 6),
                make_cdf(&CFL_ICDF[2], 0),
            ],
            cfl_index: make_cdf(&CFL_INDEX_ICDF, 30),
            cfl_sign: make_cdf(&CFL_SIGN_ICDF, 62),
            cfl_alpha: [
                make_cdf(&CFL_ALPHA_ICDF[0], 62),
                make_cdf(&CFL_ALPHA_ICDF[1], 62),
                make_cdf(&CFL_ALPHA_ICDF[2], 62),
                make_cdf(&CFL_ALPHA_ICDF[3], 62),
                make_cdf(&CFL_ALPHA_ICDF[4], 62),
                make_cdf(&CFL_ALPHA_ICDF[5], 62),
            ],
            txb_skip_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(
                    &TXB_SKIP_Y_QCTX_ICDF[q][t][c],
                    TXB_SKIP_Y_QCTX_PARA[q][t][c],
                ))
            }),
            txb_skip_u: std::array::from_fn(|i| make_cdf(
                &TXB_SKIP_Y_QCTX_ICDF[q][3][6 + i],
                TXB_SKIP_Y_QCTX_PARA[q][3][6 + i],
            )),
            txb_skip_v: std::array::from_fn(|i| make_cdf(
                &TXB_SKIP_V_QCTX_ICDF[q][i],
                TXB_SKIP_V_QCTX_PARA[q][i],
            )),
            txb_skip_v_u_nz: std::array::from_fn(|i| make_cdf(
                &TXB_SKIP_V_QCTX_ICDF[q][6 + i],
                TXB_SKIP_V_QCTX_PARA[q][6 + i],
            )),
            dc_sign_y: std::array::from_fn(|i| make_cdf(
                &DC_SIGN_Y_QCTX_ICDF[q][0][i],
                DC_SIGN_Y_QCTX_PARA[q][0][i],
            )),
            dc_sign_uv: make_cdf(&DC_SIGN_UV_QCTX_ICDF[q][0][0], DC_SIGN_UV_QCTX_PARA[q][0][0]),
            eob_multi_1024: std::array::from_fn(|i| make_cdf(
                &EOB_MULTI_1024_QCTX_ICDF[q][i],
                EOB_MULTI_1024_QCTX_PARA[q][i],
            )),
            eob_extra: make_cdf(&EOB_EXTRA_QCTX_ICDF[q], EOB_EXTRA_QCTX_PARA[q]),
            coeff_base_lf_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| std::array::from_fn(|qi| make_cdf(
                    &COEFF_BASE_LF_Y_QCTX_ICDF[q][t][c][qi],
                    COEFF_BASE_LF_Y_QCTX_PARA[q][t][c][qi],
                )))
            }),
            coeff_base_lf_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(
                    &COEFF_BASE_LF_EOB_Y_QCTX_ICDF[q][t][c],
                    COEFF_BASE_LF_EOB_Y_QCTX_PARA[q][t][c],
                ))
            }),
            coeff_base_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| std::array::from_fn(|qi| make_cdf(
                    &COEFF_BASE_Y_QCTX_ICDF[q][t][c][qi],
                    COEFF_BASE_Y_QCTX_PARA[q][t][c][qi],
                )))
            }),
            coeff_base_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(
                    &COEFF_BASE_EOB_Y_QCTX_ICDF[q][t][c],
                    COEFF_BASE_EOB_Y_QCTX_PARA[q][t][c],
                ))
            }),
            coeff_br_lf_y: std::array::from_fn(|i| make_cdf(
                &COEFF_BR_LF_Y_QCTX_ICDF[q][i],
                COEFF_BR_LF_Y_QCTX_PARA[q][i],
            )),
            coeff_br_y: std::array::from_fn(|i| make_cdf(
                &COEFF_BR_Y_QCTX_ICDF[q][i],
                COEFF_BR_Y_QCTX_PARA[q][i],
            )),
            coeff_base_lf_uv: std::array::from_fn(|i| std::array::from_fn(|qi| make_cdf(
                &COEFF_BASE_LF_UV_QCTX_ICDF[q][i][qi],
                COEFF_BASE_LF_UV_QCTX_PARA[q][i][qi],
            ))),
            coeff_base_lf_eob_uv: std::array::from_fn(|i| make_cdf(
                &COEFF_BASE_LF_EOB_UV_QCTX_ICDF[q][i],
                COEFF_BASE_LF_EOB_UV_QCTX_PARA[q][i],
            )),
            coeff_base_uv: std::array::from_fn(|i| std::array::from_fn(|qi| make_cdf(
                &COEFF_BASE_UV_QCTX_ICDF[q][i][qi],
                COEFF_BASE_UV_QCTX_PARA[q][i][qi],
            ))),
            coeff_base_eob_uv: std::array::from_fn(|i| make_cdf(
                &COEFF_BASE_EOB_UV_QCTX_ICDF[q][i],
                COEFF_BASE_EOB_UV_QCTX_PARA[q][i],
            )),
            coeff_br_lf_uv: std::array::from_fn(|i| make_cdf(
                &COEFF_BR_UV_QCTX_ICDF[q][i],
                COEFF_BR_UV_QCTX_PARA[q][i],
            )),
            coeff_br_uv: std::array::from_fn(|i| make_cdf(
                &COEFF_BR_UV_QCTX_ICDF[q][i],
                COEFF_BR_UV_QCTX_PARA[q][i],
            )),
            do_split_part: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_SPLIT_PART_ICDF[p][c], DO_SPLIT_PART_PARA[p][c]))
            }),
            do_square_split: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_SQUARE_SPLIT_ICDF[p][c], DO_SQUARE_SPLIT_PARA[p][c]))
            }),
            rect_type: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&RECT_TYPE_ICDF[p][c], RECT_TYPE_PARA[p][c]))
            }),
            do_ext_partition: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_EXT_PARTITION_ICDF[p][c], DO_EXT_PARTITION_PARA[p][c]))
            }),
            do_uneven_4way: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_UNEVEN_4WAY_ICDF[p][c], DO_UNEVEN_4WAY_PARA[p][c]))
            }),
            eob_multi_16: std::array::from_fn(|i| make_cdf(
                &EOB_MULTI_16_QCTX_ICDF[q][i],
                EOB_MULTI_16_QCTX_PARA[q][i],
            )),
            eob_multi_32: std::array::from_fn(|i| make_cdf(
                &EOB_MULTI_32_QCTX_ICDF[q][i],
                EOB_MULTI_32_QCTX_PARA[q][i],
            )),
            eob_multi_64: std::array::from_fn(|i| make_cdf(
                &EOB_MULTI_64_QCTX_ICDF[q][i],
                EOB_MULTI_64_QCTX_PARA[q][i],
            )),
            eob_multi_128: std::array::from_fn(|i| make_cdf(
                &EOB_MULTI_128_QCTX_ICDF[q][i],
                EOB_MULTI_128_QCTX_PARA[q][i],
            )),
            eob_multi_256: std::array::from_fn(|i| make_cdf(
                &EOB_MULTI_256_QCTX_ICDF[q][i],
                EOB_MULTI_256_QCTX_PARA[q][i],
            )),
            eob_multi_512: std::array::from_fn(|i| make_cdf(
                &EOB_MULTI_512_QCTX_ICDF[q][i],
                EOB_MULTI_512_QCTX_PARA[q][i],
            )),
            fsc_mode: std::array::from_fn(|ctx| {
                std::array::from_fn(|bg| make_cdf(&FSC_MODE_ICDF[ctx][bg], FSC_MODE_PARA[ctx][bg]))
            }),
            mrl_index: std::array::from_fn(|i| make_cdf(&MRL_INDEX_ICDF[i], MRL_INDEX_PARA[i])),
            multi_line_mrl: std::array::from_fn(|i| make_cdf(&MULTI_LINE_MRL_ICDF[i], MULTI_LINE_MRL_PARA[i])),
            txfm_do_partition: std::array::from_fn(|fsc| {
                std::array::from_fn(|inter| {
                    std::array::from_fn(|g| make_cdf(&TXFM_DO_PARTITION_ICDF[fsc][inter][g], TXFM_DO_PARTITION_PARA[fsc][inter][g]))
                })
            }),
            txfm_4way_partition_type: std::array::from_fn(|fsc| {
                std::array::from_fn(|inter| {
                    std::array::from_fn(|g| make_cdf(&TXFM_4WAY_PARTITION_TYPE_ICDF[fsc][inter][g], TXFM_4WAY_PARTITION_TYPE_PARA[fsc][inter][g]))
                })
            }),
            txfm_2or3_way_partition_type: std::array::from_fn(|fsc| {
                std::array::from_fn(|inter| {
                    std::array::from_fn(|g| make_cdf(&TXFM_2OR3_WAY_PARTITION_TYPE_ICDF[fsc][inter][g], TXFM_2OR3_WAY_PARTITION_TYPE_PARA[fsc][inter][g]))
                })
            }),
            intrabc: std::array::from_fn(|i| make_cdf(&INTRABC_ICDF[i], INTRABC_PARA[i])),
            cdef_strength_index0: std::array::from_fn(|i| make_cdf(&CDEF_STRENGTH_INDEX0_ICDF[i], CDEF_STRENGTH_INDEX0_PARA[i])),
            ccso: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&CCSO_ICDF[p][c], CCSO_PARA[p][c]))
            }),
            intra_ext_tx: std::array::from_fn(|i| make_cdf(&INTRA_EXT_TX_ICDF[i], INTRA_EXT_TX_PARA[i])),
            intra_ext_tx_reduced: make_cdf(&INTRA_EXT_TX_REDUCED_ICDF, INTRA_EXT_TX_REDUCED_PARA),
            intra_ext_tx_short_side: std::array::from_fn(|i| make_cdf(&INTRA_EXT_TX_SHORT_SIDE_ICDF[i], INTRA_EXT_TX_SHORT_SIDE_PARA[i])),
            tx_ext_32_intra: make_cdf(&TX_EXT_32_INTRA_ICDF, TX_EXT_32_INTRA_PARA),
            stx: std::array::from_fn(|i| make_cdf(&STX_ICDF[i], STX_PARA[i])),
            most_probable_stx_set: make_cdf(&MOST_PROBABLE_STX_SET_ICDF, MOST_PROBABLE_STX_SET_PARA),
            most_probable_stx_set_adst_adst: make_cdf(&MOST_PROBABLE_STX_SET_ADST_ADST_ICDF, MOST_PROBABLE_STX_SET_ADST_ADST_PARA),
            cctx_type: make_cdf(&CCTX_TYPE_ICDF, CCTX_TYPE_PARA),
            palette_y_mode: make_cdf(&PALETTE_Y_MODE_ICDF, PALETTE_Y_MODE_PARA),
            palette_y_size: make_cdf(&PALETTE_Y_SIZE_ICDF, PALETTE_Y_SIZE_PARA),
            identity_row_y: std::array::from_fn(|i| make_cdf(&IDENTITY_ROW_Y_ICDF[i], IDENTITY_ROW_Y_PARA[i])),
            palette_y_color_index: std::array::from_fn(|ps| {
                std::array::from_fn(|ctx| {
                    let n_colors = PALETTE_COLORS_PER_SIZE[ps];
                    make_cdf(&PALETTE_Y_COLOR_INDEX_ICDF[ps][ctx][..n_colors], PALETTE_Y_COLOR_INDEX_PARA[ps][ctx])
                })
            }),
        }
    }

    pub fn txb_skip_y_for_tx(&mut self, tx_size: TxSize, entropy_ctx: usize) -> &mut Vec<u16> {
        &mut self.txb_skip_y[tx_size.txs_ctx()][entropy_ctx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_initializes_with_correct_sizes() {
        let ctx = FrameCdfContext::new();
        assert_eq!(ctx.do_split.len(), 4);
        assert_eq!(ctx.y_mode_idx[0].len(), 10);
        assert_eq!(ctx.y_mode_idx.len(), 3);
        assert_eq!(ctx.y_mode_idx_offset.len(), 3);
        assert_eq!(ctx.y_mode_idx_offset[0].len(), 8);
        assert_eq!(ctx.uv_mode.len(), 2);
        assert_eq!(ctx.cfl.len(), 3);
        assert_eq!(ctx.cfl[0].len(), 4);
        assert_eq!(ctx.cfl_sign.len(), 10);
        assert_eq!(ctx.cfl_alpha.len(), 6);
        assert_eq!(ctx.do_split[0], 12276);
        assert_eq!(ctx.do_split[1], 0);
        assert_eq!(ctx.txb_skip_y.len(), 5);
        assert_eq!(ctx.txb_skip_y[0].len(), 10);
        assert_eq!(ctx.coeff_base_lf_y.len(), 5);
        assert_eq!(ctx.eob_multi_32.len(), 3);
        assert_eq!(ctx.eob_multi_128.len(), 3);
    }

    #[test]
    fn context_cdfs_are_mutable() {
        let mut ctx = FrameCdfContext::new();
        crate::entropy::update_cdf(&mut ctx.do_split, 0, 2);
        assert_eq!(ctx.do_split[2], 1);
        assert_ne!(ctx.do_split[0], 12276);
    }
}
