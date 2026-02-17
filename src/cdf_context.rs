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
    pub coeff_base_lf_y: [[Vec<u16>; 33]; 5],
    pub coeff_base_lf_eob_y: [[Vec<u16>; 4]; 5],
    pub coeff_base_y: [[Vec<u16>; 20]; 5],
    pub coeff_base_eob_y: [[Vec<u16>; 4]; 5],
    pub coeff_br_lf_y: [Vec<u16>; 14],
    pub coeff_br_y: [Vec<u16>; 7],
    pub coeff_base_lf_uv: [Vec<u16>; 12],
    pub coeff_base_lf_eob_uv: [Vec<u16>; 4],
    pub coeff_base_uv: [Vec<u16>; 12],
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
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_LF_Y_ICDF[t][c], 0))
            }),
            coeff_base_lf_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_LF_EOB_Y_ICDF[t][c], 0))
            }),
            coeff_base_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_Y_ICDF[t][c], 0))
            }),
            coeff_base_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_EOB_Y_ICDF[t][c], 0))
            }),
            coeff_br_lf_y: std::array::from_fn(|i| make_cdf(&COEFF_BR_LF_Y_ICDF[i], 0)),
            coeff_br_y: std::array::from_fn(|i| make_cdf(&COEFF_BR_Y_ICDF[i], 0)),
            coeff_base_lf_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_LF_UV_ICDF[i], 0)),
            coeff_base_lf_eob_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_LF_EOB_UV_ICDF[i], 0)),
            coeff_base_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_UV_ICDF[i], 0)),
            coeff_base_eob_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_EOB_UV_ICDF[i], 0)),
            coeff_br_lf_uv: std::array::from_fn(|i| make_cdf(&COEFF_BR_LF_UV_ICDF[i], 0)),
            coeff_br_uv: std::array::from_fn(|i| make_cdf(&COEFF_BR_UV_ICDF[i], 0)),
            do_split_part: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_SPLIT_PART_ICDF[p][c], 0))
            }),
            do_square_split: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_SQUARE_SPLIT_ICDF[p][c], 0))
            }),
            rect_type: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&RECT_TYPE_ICDF[p][c], 0))
            }),
            do_ext_partition: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_EXT_PARTITION_ICDF[p][c], 0))
            }),
            do_uneven_4way: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_UNEVEN_4WAY_ICDF[p][c], 0))
            }),
            eob_multi_16: std::array::from_fn(|i| make_cdf(&EOB_MULTI_16_ICDF[i], 0)),
            eob_multi_32: std::array::from_fn(|i| make_cdf(&EOB_MULTI_32_ICDF[i], 0)),
            eob_multi_64: std::array::from_fn(|i| make_cdf(&EOB_MULTI_64_ICDF[i], 0)),
            eob_multi_128: std::array::from_fn(|i| make_cdf(&EOB_MULTI_128_ICDF[i], 0)),
            eob_multi_256: std::array::from_fn(|i| make_cdf(&EOB_MULTI_256_ICDF[i], 0)),
            eob_multi_512: std::array::from_fn(|i| make_cdf(&EOB_MULTI_512_ICDF[i], 0)),
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
                std::array::from_fn(|c| make_cdf(
                    &COEFF_BASE_LF_Y_QCTX_ICDF[q][t][c],
                    COEFF_BASE_LF_Y_QCTX_PARA[q][t][c],
                ))
            }),
            coeff_base_lf_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(
                    &COEFF_BASE_LF_EOB_Y_QCTX_ICDF[q][t][c],
                    COEFF_BASE_LF_EOB_Y_QCTX_PARA[q][t][c],
                ))
            }),
            coeff_base_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(
                    &COEFF_BASE_Y_QCTX_ICDF[q][t][c],
                    COEFF_BASE_Y_QCTX_PARA[q][t][c],
                ))
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
            coeff_base_lf_uv: std::array::from_fn(|i| make_cdf(
                &COEFF_BASE_LF_UV_QCTX_ICDF[q][i],
                COEFF_BASE_LF_UV_QCTX_PARA[q][i],
            )),
            coeff_base_lf_eob_uv: std::array::from_fn(|i| make_cdf(
                &COEFF_BASE_LF_EOB_UV_QCTX_ICDF[q][i],
                COEFF_BASE_LF_EOB_UV_QCTX_PARA[q][i],
            )),
            coeff_base_uv: std::array::from_fn(|i| make_cdf(
                &COEFF_BASE_UV_QCTX_ICDF[q][i],
                COEFF_BASE_UV_QCTX_PARA[q][i],
            )),
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
                std::array::from_fn(|c| make_cdf(&DO_SPLIT_PART_ICDF[p][c], 0))
            }),
            do_square_split: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_SQUARE_SPLIT_ICDF[p][c], 0))
            }),
            rect_type: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&RECT_TYPE_ICDF[p][c], 0))
            }),
            do_ext_partition: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_EXT_PARTITION_ICDF[p][c], 0))
            }),
            do_uneven_4way: std::array::from_fn(|p| {
                std::array::from_fn(|c| make_cdf(&DO_UNEVEN_4WAY_ICDF[p][c], 0))
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
