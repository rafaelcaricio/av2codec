use crate::entropy::DaalaCoder;

// Default ICDF tables for the simplest intra-only KEY_FRAME
// Config: enable_ext_partitions=0, segmentation=0, intrabc=0, enable_gdf=0,
// enable_cdef=0, enable_ccso=0, delta_q_present=0, enable_cfl_intra=0
// base_qindex <= 90 (coef CDF category 0)

// do_split CDF: PARTITION_STRUCTURE_0, context 12 (BLOCK_64X64, no neighbors)
// AVM_CDF2(20492)
const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];

// Y mode set CDF (no context dimension)
// AVM_CDF4(28863, 31022, 31724)
const Y_MODE_SET_ICDF: [u16; 4] = [3905, 1746, 1044, 0];

// Y mode index CDF, context 0 (no angular neighbors)
// AVM_CDF8(15175, 20075, 21728, 24098, 26405, 27655, 28860)
const Y_MODE_IDX_ICDF: [u16; 8] = [17593, 12693, 11040, 8670, 6363, 5113, 3908, 0];

// UV mode CDF, context 0 (Y mode is non-directional)
// AVM_CDF8(9363, 20957, 22865, 24753, 26411, 27983, 30428)
const UV_MODE_ICDF: [u16; 8] = [23405, 11811, 9903, 8015, 6357, 4785, 2340, 0];

// Coefficient txb_skip CDFs (coef category 0, pred_mode_ctx=0 intra)
// Y: TX_64X64 (txs_ctx=4, txb_skip_ctx=0) — AVM_CDF2(2800)
const TXB_SKIP_Y_ICDF: [u16; 2] = [29968, 0];

// U: TX_32X32 (txs_ctx=3, txb_skip_ctx=6 — hardcoded offset for U plane) — AVM_CDF2(1300)
const TXB_SKIP_U_ICDF: [u16; 2] = [31468, 0];

// V: v_txb_skip (ctx=0, U had eob=0) — AVM_CDF2(1439)
const TXB_SKIP_V_ICDF: [u16; 2] = [31329, 0];

// EOB flag CDF for eob_multi_size=6 (TX_64X64 and TX_32X32), 8 symbols
// Y (pl_ctx=0): AVM_CDF8(576, 1953, 8086, 14995, 20006, 24682, 25870)
const EOB_FLAG_1024_Y_ICDF: [u16; 8] = [32192, 30815, 24682, 17773, 12762, 8086, 6898, 0];
// UV (pl_ctx=2): AVM_CDF8(11873, 15379, 25835, 27918, 29093, 30164, 30782)
const EOB_FLAG_1024_UV_ICDF: [u16; 8] = [20895, 17389, 6933, 4850, 3675, 2604, 1986, 0];

// LF base coefficient at EOB for Y (txs_ctx=4, coeff_ctx=0)
// AVM_CDF5(298, 596, 894, 1192)
const COEFF_BASE_LF_EOB_Y_ICDF: [u16; 5] = [32470, 32172, 31874, 31576, 0];

// LF base coefficient at EOB for UV (coeff_ctx=0)
// AVM_CDF5(28950, 31443, 32009, 32257)
const COEFF_BASE_LF_EOB_UV_ICDF: [u16; 5] = [3818, 1325, 759, 511, 0];

// LF base range for Y (br_ctx=0)
// AVM_CDF4(7943, 14193, 20775)
const COEFF_BR_LF_Y_ICDF: [u16; 4] = [24825, 18575, 11993, 0];

const DC_SIGN_Y_ICDF: [u16; 2] = [16937, 0];

// V: v_txb_skip_cdf[6] (U had nonzero, adds V_TXB_SKIP_CONTEXT_OFFSET=6) — AVM_CDF2(7648)
const TXB_SKIP_V_U_NONZERO_ICDF: [u16; 2] = [25120, 0];

const LF_NUM_BASE_LEVELS: usize = 4;
const LF_BASE_SYMBOLS: usize = 6;
const BR_CDF_SIZE: usize = 4;
const COEFF_BASE_RANGE: usize = 3;
const Y_LF_HR_MAX: usize = COEFF_BASE_RANGE + LF_NUM_BASE_LEVELS;
const UV_LF_HR_MAX: usize = LF_NUM_BASE_LEVELS;
const ADAPTIVE_TABLE: [usize; 5] = [4, 8, 16, 32, 64];

fn get_adaptive_param(ctx: usize) -> u32 {
    let mut m = 0;
    while m < ADAPTIVE_TABLE.len() && ctx >= ADAPTIVE_TABLE[m] {
        m += 1;
    }
    (m + 1) as u32
}

fn encode_exp_golomb(ec: &mut DaalaCoder, mut x: u32, mut k: u32) {
    while x >= (1 << k) {
        ec.encode_literal(0, 1);
        x -= 1 << k;
        k += 1;
    }
    ec.encode_literal(1, 1);
    if k > 0 {
        ec.encode_literal(x, k);
    }
}

fn encode_truncated_rice(ec: &mut DaalaCoder, level: u32, m: u32, k: u32, cmax: u32) {
    let q = level >> m;
    if q >= cmax {
        ec.encode_literal(0, cmax);
        encode_exp_golomb(ec, level - (cmax << m), k);
    } else {
        let mask = (1u32 << m) - 1;
        if q > 0 {
            ec.encode_literal(0, q);
        }
        ec.encode_literal(1, 1);
        if m > 0 {
            ec.encode_literal(level & mask, m);
        }
    }
}

fn encode_adaptive_hr(ec: &mut DaalaCoder, hr: usize, hr_ctx: usize) {
    let m = get_adaptive_param(hr_ctx);
    let k = m + 1;
    let cmax = (m + 4).min(6);
    encode_truncated_rice(ec, hr as u32, m, k, cmax);
}

fn encode_high_range(ec: &mut DaalaCoder, abs_level: usize, max: usize) {
    if abs_level > max {
        let hr = abs_level - max - 1;
        encode_adaptive_hr(ec, hr, 0);
    }
}

fn encode_plane_coeffs(ec: &mut DaalaCoder, dc_level: i32, is_y: bool) {
    let abs_level = dc_level.unsigned_abs() as usize;
    let sign = if dc_level < 0 { 1u32 } else { 0u32 };

    let txb_skip_icdf = if is_y {
        &TXB_SKIP_Y_ICDF
    } else {
        &TXB_SKIP_U_ICDF
    };

    if abs_level == 0 {
        ec.encode_symbol(1, txb_skip_icdf);
        return;
    }

    ec.encode_symbol(0, txb_skip_icdf);

    let eob_icdf: &[u16] = if is_y {
        &EOB_FLAG_1024_Y_ICDF
    } else {
        &EOB_FLAG_1024_UV_ICDF
    };
    ec.encode_symbol(0, eob_icdf);

    let base_eob_icdf = if is_y {
        &COEFF_BASE_LF_EOB_Y_ICDF
    } else {
        &COEFF_BASE_LF_EOB_UV_ICDF
    };
    let base_symbol = abs_level.min(LF_BASE_SYMBOLS - 1) - 1;
    ec.encode_symbol(base_symbol, base_eob_icdf);

    if is_y {
        if abs_level > LF_NUM_BASE_LEVELS {
            let remainder = abs_level - 1 - LF_NUM_BASE_LEVELS;
            let sym = remainder.min(BR_CDF_SIZE - 1);
            ec.encode_symbol(sym, &COEFF_BR_LF_Y_ICDF);
        }
        ec.encode_symbol(sign as usize, &DC_SIGN_Y_ICDF);
        encode_high_range(ec, abs_level, Y_LF_HR_MAX);
    } else {
        ec.encode_literal(sign, 1);
        encode_high_range(ec, abs_level, UV_LF_HR_MAX);
    }
}

fn encode_plane_coeffs_v(ec: &mut DaalaCoder, dc_level: i32, u_was_zero: bool) {
    let abs_level = dc_level.unsigned_abs() as usize;
    let sign = if dc_level < 0 { 1u32 } else { 0u32 };

    let txb_skip_icdf = if u_was_zero {
        &TXB_SKIP_V_ICDF
    } else {
        &TXB_SKIP_V_U_NONZERO_ICDF
    };

    if abs_level == 0 {
        ec.encode_symbol(1, txb_skip_icdf);
        return;
    }

    ec.encode_symbol(0, txb_skip_icdf);
    ec.encode_symbol(0, &EOB_FLAG_1024_UV_ICDF);

    let base_symbol = abs_level.min(LF_BASE_SYMBOLS - 1) - 1;
    ec.encode_symbol(base_symbol, &COEFF_BASE_LF_EOB_UV_ICDF);

    ec.encode_literal(sign, 1);
    encode_high_range(ec, abs_level, UV_LF_HR_MAX);
}

pub fn encode_tile_data_with_dc(y_dc_level: i32, u_dc_level: i32, v_dc_level: i32) -> Vec<u8> {
    let mut ec = DaalaCoder::new();

    ec.encode_symbol(0, &DO_SPLIT_ICDF);
    ec.encode_symbol(0, &Y_MODE_SET_ICDF);
    ec.encode_symbol(0, &Y_MODE_IDX_ICDF);
    ec.encode_symbol(0, &UV_MODE_ICDF);

    encode_plane_coeffs(&mut ec, y_dc_level, true);
    encode_plane_coeffs(&mut ec, u_dc_level, false);
    encode_plane_coeffs_v(&mut ec, v_dc_level, u_dc_level == 0);

    ec.done()
}

pub fn encode_tile_data() -> Vec<u8> {
    let mut ec = DaalaCoder::new();

    // Partition: do_split = 0 (PARTITION_NONE for 64x64 SB)
    ec.encode_symbol(0, &DO_SPLIT_ICDF);

    // Intra luma mode: set 0 (DC_PRED is in first mode set)
    ec.encode_symbol(0, &Y_MODE_SET_ICDF);

    // Intra luma mode: index 0 within set (DC_PRED)
    ec.encode_symbol(0, &Y_MODE_IDX_ICDF);

    // Intra chroma mode: index 0 (DC_PRED, no CFL since enable_cfl_intra=0)
    ec.encode_symbol(0, &UV_MODE_ICDF);

    // Coefficients: all zero (eob==0 encoded as symbol 1)
    // Y plane (1 TU of TX_64X64)
    ec.encode_symbol(1, &TXB_SKIP_Y_ICDF);

    // U plane (1 TU of TX_32X32)
    ec.encode_symbol(1, &TXB_SKIP_U_ICDF);

    // V plane (1 TU of TX_32X32)
    ec.encode_symbol(1, &TXB_SKIP_V_ICDF);

    ec.done()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_data_is_nonempty() {
        let data = encode_tile_data();
        assert!(!data.is_empty());
    }

    #[test]
    fn tile_data_reasonable_size() {
        let data = encode_tile_data();
        assert!(
            data.len() <= 20,
            "tile data too large: {} bytes",
            data.len()
        );
    }

    #[test]
    fn tile_data_with_dc_coefficient() {
        let data = encode_tile_data_with_dc(5, 0, 0);
        assert!(!data.is_empty());
        let zero_data = encode_tile_data_with_dc(0, 0, 0);
        assert_ne!(data, zero_data);
    }

    #[test]
    fn tile_data_dc_zero_matches_original() {
        let original = encode_tile_data();
        let with_dc = encode_tile_data_with_dc(0, 0, 0);
        assert_eq!(original, with_dc);
    }

    #[test]
    fn tile_data_negative_dc() {
        let pos = encode_tile_data_with_dc(5, 0, 0);
        let neg = encode_tile_data_with_dc(-5, 0, 0);
        assert_ne!(pos, neg);
    }
}
