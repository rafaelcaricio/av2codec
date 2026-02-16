use crate::cdf_tables::*;

fn make_cdf(icdf: &[u16], para: u16) -> Vec<u16> {
    let mut cdf = icdf.to_vec();
    cdf.push(0);
    cdf.push(para);
    cdf
}

const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];
const Y_MODE_SET_ICDF: [u16; 4] = [3905, 1746, 1044, 0];
const Y_MODE_IDX_ICDF: [u16; 8] = [17593, 12693, 11040, 8670, 6363, 5113, 3908, 0];
const UV_MODE_ICDF: [u16; 8] = [23405, 11811, 9903, 8015, 6357, 4785, 2340, 0];
const TXB_SKIP_Y_ICDF: [u16; 2] = [29968, 0];

pub struct FrameCdfContext {
    pub do_split: Vec<u16>,
    pub y_mode_set: Vec<u16>,
    pub y_mode_idx: Vec<u16>,
    pub uv_mode: Vec<u16>,
    pub txb_skip_y: Vec<u16>,
    pub txb_skip_u: [Vec<u16>; 3],
    pub txb_skip_v: [Vec<u16>; 3],
    pub txb_skip_v_u_nz: [Vec<u16>; 3],
    pub dc_sign_y: [Vec<u16>; 3],
    pub dc_sign_uv: Vec<u16>,
    pub eob_multi_1024: [Vec<u16>; 3],
    pub eob_extra: Vec<u16>,
    pub coeff_base_lf_y: [[Vec<u16>; 33]; 2],
    pub coeff_base_lf_eob_y: [[Vec<u16>; 4]; 2],
    pub coeff_base_y: [[Vec<u16>; 20]; 2],
    pub coeff_base_eob_y: [[Vec<u16>; 4]; 2],
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
    pub eob_multi_64: [Vec<u16>; 3],
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
            do_split: make_cdf(&DO_SPLIT_ICDF, 0),
            y_mode_set: make_cdf(&Y_MODE_SET_ICDF, 0),
            y_mode_idx: make_cdf(&Y_MODE_IDX_ICDF, 0),
            uv_mode: make_cdf(&UV_MODE_ICDF, 0),
            txb_skip_y: make_cdf(&TXB_SKIP_Y_ICDF, 0),
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
            eob_multi_64: std::array::from_fn(|i| make_cdf(&EOB_MULTI_64_ICDF[i], 0)),
            eob_multi_256: std::array::from_fn(|i| make_cdf(&EOB_MULTI_256_ICDF[i], 0)),
            eob_multi_512: std::array::from_fn(|i| make_cdf(&EOB_MULTI_512_ICDF[i], 0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_initializes_with_correct_sizes() {
        let ctx = FrameCdfContext::new();
        assert_eq!(ctx.do_split.len(), 4);
        assert_eq!(ctx.y_mode_idx.len(), 10);
        assert_eq!(ctx.do_split[2], 0);
        assert_eq!(ctx.y_mode_idx[8], 0);
        assert_eq!(ctx.do_split[0], 12276);
        assert_eq!(ctx.do_split[1], 0);
    }

    #[test]
    fn context_cdfs_are_mutable() {
        let mut ctx = FrameCdfContext::new();
        crate::entropy::update_cdf(&mut ctx.do_split, 0, 2);
        assert_eq!(ctx.do_split[2], 1);
        assert_ne!(ctx.do_split[0], 12276);
    }
}
