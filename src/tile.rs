use crate::cdf_tables::*;
use crate::entropy::DaalaCoder;
use crate::scan::DEFAULT_SCAN_32X32;

const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];
const Y_MODE_SET_ICDF: [u16; 4] = [3905, 1746, 1044, 0];
const Y_MODE_IDX_ICDF: [u16; 8] = [17593, 12693, 11040, 8670, 6363, 5113, 3908, 0];
const UV_MODE_ICDF: [u16; 8] = [23405, 11811, 9903, 8015, 6357, 4785, 2340, 0];
const TXB_SKIP_Y_ICDF: [u16; 2] = [29968, 0];
const TXB_SKIP_U_ICDF: [u16; 2] = [31468, 0];
const TXB_SKIP_V_ICDF: [u16; 2] = [31329, 0];
const TXB_SKIP_V_U_NONZERO_ICDF: [u16; 2] = [25120, 0];

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

fn get_padded_idx(idx: usize, bwl: u32) -> usize {
    idx + ((idx >> bwl) << TX_PAD_HOR_LOG2)
}

fn is_lf_2d(row: usize, col: usize, plane: usize) -> bool {
    if plane == 0 {
        (row + col) < LF_2D_LIM
    } else {
        (row + col) < LF_2D_LIM_UV
    }
}

fn get_lower_levels_ctx_eob(bwl: u32, height: usize, scan_idx: usize) -> usize {
    if scan_idx == 0 {
        return 0;
    }
    if scan_idx <= (height << bwl) / 8 {
        return 1;
    }
    if scan_idx <= (height << bwl) / 4 {
        return 2;
    }
    3
}

fn get_lower_levels_ctx_lf_2d(levels: &[u8], coeff_idx: usize, bwl: u32) -> usize {
    let pos = get_padded_idx(coeff_idx, bwl);
    let stride = (1 << bwl) + TX_PAD_HOR;
    let mag = (levels[pos + 1] as usize).min(5)
        + (levels[pos + stride] as usize).min(5)
        + (levels[pos + stride + 1] as usize).min(5)
        + (levels[pos + 2] as usize).min(5)
        + (levels[pos + 2 * stride] as usize).min(5);
    let ctx = (mag + 1) >> 1;
    let row = coeff_idx >> bwl;
    let col = coeff_idx - (row << bwl);
    if coeff_idx == 0 {
        return ctx.min(8);
    }
    if row + col < 2 {
        return ctx.min(6) + 9;
    }
    ctx.min(4) + 16
}

fn get_lower_levels_ctx_lf_2d_chroma(
    levels: &[u8],
    coeff_idx: usize,
    bwl: u32,
    plane: usize,
) -> usize {
    let pos = get_padded_idx(coeff_idx, bwl);
    let stride = (1 << bwl) + TX_PAD_HOR;
    let mag = (levels[pos + 1] as usize).min(5)
        + (levels[pos + stride] as usize).min(5)
        + (levels[pos + stride + 1] as usize).min(5);
    let ctx = ((mag + 1) >> 1).min(3);
    if plane == 1 { ctx } else { ctx + 4 }
}

fn get_lower_levels_ctx_2d(levels: &[u8], coeff_idx: usize, bwl: u32, plane: usize) -> usize {
    let pos = get_padded_idx(coeff_idx, bwl);
    let stride = (1 << bwl) + TX_PAD_HOR;
    let mag = (levels[pos + 1] as usize).min(3)
        + (levels[pos + stride] as usize).min(3)
        + (levels[pos + stride + 1] as usize).min(3)
        + (levels[pos + 2] as usize).min(3)
        + (levels[pos + 2 * stride] as usize).min(3);
    let ctx = ((mag + 1) >> 1).min(4);
    if plane > 0 {
        return ctx;
    }
    let row = coeff_idx >> bwl;
    let col = coeff_idx - (row << bwl);
    if row + col < 6 {
        return ctx;
    }
    if row + col < 8 {
        return ctx + 5;
    }
    ctx + 10
}

fn get_lower_levels_ctx_2d_chroma(
    levels: &[u8],
    coeff_idx: usize,
    bwl: u32,
    plane: usize,
) -> usize {
    let pos = get_padded_idx(coeff_idx, bwl);
    let stride = (1 << bwl) + TX_PAD_HOR;
    let mag = (levels[pos + 1] as usize).min(3)
        + (levels[pos + stride] as usize).min(3)
        + (levels[pos + stride + 1] as usize).min(3);
    let ctx = ((mag + 1) >> 1).min(3);
    if plane == 1 { ctx } else { ctx + 4 }
}

fn get_br_ctx_lf_eob_2d(coeff_idx: usize) -> usize {
    if coeff_idx == 0 { 0 } else { 7 }
}

fn get_br_lf_ctx_2d(levels: &[u8], coeff_idx: usize, bwl: u32) -> usize {
    let row = coeff_idx >> bwl;
    let col = coeff_idx - (row << bwl);
    let stride = (1 << bwl) + TX_PAD_HOR;
    let pos = row * stride + col;
    let mag = (levels[pos + 1] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride + 1] as usize).min(MAX_VAL_BR_CTX as usize);
    ((mag + 1) >> 1).min(6) + 7
}

fn get_br_lf_ctx_dc_2d(levels: &[u8], coeff_idx: usize, bwl: u32) -> usize {
    let stride = (1 << bwl) + TX_PAD_HOR;
    let pos = (coeff_idx >> bwl) * stride + (coeff_idx - ((coeff_idx >> bwl) << bwl));
    let mag = (levels[pos + 1] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride + 1] as usize).min(MAX_VAL_BR_CTX as usize);
    ((mag + 1) >> 1).min(6)
}

fn get_br_ctx_2d(levels: &[u8], coeff_idx: usize, bwl: u32) -> usize {
    let row = coeff_idx >> bwl;
    let col = coeff_idx - (row << bwl);
    let stride = (1 << bwl) + TX_PAD_HOR;
    let pos = row * stride + col;
    let mag = (levels[pos + 1] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride + 1] as usize).min(MAX_VAL_BR_CTX as usize);
    ((mag + 1) >> 1).min(6)
}

fn get_br_ctx_2d_chroma(levels: &[u8], coeff_idx: usize, bwl: u32) -> usize {
    let row = coeff_idx >> bwl;
    let col = coeff_idx - (row << bwl);
    let stride = (1 << bwl) + TX_PAD_HOR;
    let pos = row * stride + col;
    let mag = (levels[pos + 1] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride] as usize).min(MAX_VAL_BR_CTX as usize)
        + (levels[pos + stride + 1] as usize).min(MAX_VAL_BR_CTX as usize);
    ((mag + 1) >> 1).min(3)
}

fn encode_low_range(ec: &mut DaalaCoder, level: usize, base_levels: usize, cdf: &[u16]) {
    if level > base_levels {
        let remainder = level - 1 - base_levels;
        let sym = remainder.min(BR_CDF_SIZE - 1);
        ec.encode_symbol(sym, cdf);
    }
}

fn encode_eob(ec: &mut DaalaCoder, eob: u16, pl_ctx: usize) {
    let eob_icdf = &EOB_MULTI_1024_ICDF[pl_ctx];

    let mut eob_pt = 0usize;
    for i in (0..EOB_GROUP_START.len()).rev() {
        if eob >= EOB_GROUP_START[i] {
            if i == 0 && eob == 0 {
                continue;
            }
            eob_pt = i;
            break;
        }
    }

    let eob_token = eob_pt - 1;
    if eob_token < EOB_PT_INDEX_COUNT - 1 {
        ec.encode_symbol(eob_token, eob_icdf);
    } else {
        ec.encode_symbol(EOB_PT_INDEX_COUNT - 1, eob_icdf);
        let extra_token = eob_token - (EOB_PT_INDEX_COUNT - 1);
        ec.encode_literal(extra_token as u32, 2);
    }

    let eob_offset_bits = EOB_OFFSET_BITS[eob_pt] as u32;
    if eob_offset_bits > 0 {
        let extra = eob - EOB_GROUP_START[eob_pt];
        let msb = (extra >> (eob_offset_bits - 1)) & 1;
        ec.encode_symbol(msb as usize, &EOB_EXTRA_ICDF);
        if eob_offset_bits > 1 {
            let rest = extra & ((1 << (eob_offset_bits - 1)) - 1);
            ec.encode_literal(rest as u32, eob_offset_bits - 1);
        }
    }
}

fn encode_high_range_full(
    ec: &mut DaalaCoder,
    level: usize,
    lf: bool,
    plane: usize,
    hr_avg: &mut usize,
) {
    let max_br = if lf {
        if plane == 0 {
            LF_MAX_BASE_BR_RANGE
        } else {
            LF_NUM_BASE_LEVELS + 1
        }
    } else {
        MAX_BASE_BR_RANGE
    };
    if level >= max_br {
        let hr = level - max_br;
        encode_adaptive_hr(ec, hr, *hr_avg);
        *hr_avg = (*hr_avg + hr) >> 1;
    }
}

struct PlaneCoeffs<'a> {
    levels: &'a [i32],
    eob: u16,
}

struct PlaneEncodeCtx<'a> {
    plane: usize,
    txs_ctx_idx: usize,
    bwl: u32,
    height: usize,
    txb_skip_icdf: &'a [u16],
    dc_sign_ctx: usize,
}

fn encode_plane_full(ec: &mut DaalaCoder, coeffs: &PlaneCoeffs, ctx: &PlaneEncodeCtx) {
    let plane = ctx.plane;
    let txs_ctx_idx = ctx.txs_ctx_idx;
    let bwl = ctx.bwl;
    let height = ctx.height;
    let txb_skip_icdf = ctx.txb_skip_icdf;
    let dc_sign_ctx = ctx.dc_sign_ctx;
    let eob = coeffs.eob;

    if eob == 0 {
        ec.encode_symbol(1, txb_skip_icdf);
        return;
    }

    ec.encode_symbol(0, txb_skip_icdf);

    let pl_ctx = if plane == 0 { 0 } else { 2 };
    encode_eob(ec, eob, pl_ctx);

    let width = 1usize << bwl;
    let padded_stride = width + TX_PAD_HOR;
    let padded_height = height + 2;
    let mut levels_buf = vec![0u8; padded_stride * padded_height];

    let eob_scan_idx = (eob - 1) as usize;
    let eob_pos = DEFAULT_SCAN_32X32[eob_scan_idx] as usize;
    let eob_row = eob_pos >> bwl;
    let eob_col = eob_pos - (eob_row << bwl);
    let eob_lf = is_lf_2d(eob_row, eob_col, plane);
    let eob_abs_level = coeffs.levels[eob_scan_idx].unsigned_abs() as usize;

    let coeff_ctx = get_lower_levels_ctx_eob(bwl, height, eob_scan_idx);

    if plane == 0 {
        if eob_lf {
            let cdf = &COEFF_BASE_LF_EOB_Y_ICDF[txs_ctx_idx][coeff_ctx];
            let sym = eob_abs_level.min(LF_BASE_SYMBOLS - 1) - 1;
            ec.encode_symbol(sym, cdf);
            if eob_abs_level > LF_NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_lf_eob_2d(eob_pos);
                encode_low_range(
                    ec,
                    eob_abs_level,
                    LF_NUM_BASE_LEVELS,
                    &COEFF_BR_LF_Y_ICDF[br_ctx],
                );
            }
        } else {
            let cdf = &COEFF_BASE_EOB_Y_ICDF[txs_ctx_idx][coeff_ctx];
            let sym = eob_abs_level.min(NUM_BASE_LEVELS + 1) - 1;
            ec.encode_symbol(sym, cdf);
            if eob_abs_level > NUM_BASE_LEVELS {
                encode_low_range(ec, eob_abs_level, NUM_BASE_LEVELS, &COEFF_BR_Y_ICDF[0]);
            }
        }
    } else if eob_lf {
        let cdf = &COEFF_BASE_LF_EOB_UV_ICDF[coeff_ctx];
        let sym = eob_abs_level.min(LF_BASE_SYMBOLS - 1) - 1;
        ec.encode_symbol(sym, cdf);
    } else {
        let cdf = &COEFF_BASE_EOB_UV_ICDF[coeff_ctx];
        let sym = eob_abs_level.min(NUM_BASE_LEVELS + 1) - 1;
        ec.encode_symbol(sym, cdf);
        if eob_abs_level > NUM_BASE_LEVELS {
            encode_low_range(ec, eob_abs_level, NUM_BASE_LEVELS, &COEFF_BR_UV_ICDF[0]);
        }
    }

    levels_buf[get_padded_idx(eob_pos, bwl)] = eob_abs_level as u8;

    if eob > 1 {
        for c in (1..eob_scan_idx).rev() {
            let pos = DEFAULT_SCAN_32X32[c] as usize;
            let abs_level = coeffs.levels[c].unsigned_abs() as usize;
            let row = pos >> bwl;
            let col = pos - (row << bwl);
            let lf = is_lf_2d(row, col, plane);

            if plane == 0 {
                if lf {
                    let ctx = get_lower_levels_ctx_lf_2d(&levels_buf, pos, bwl);
                    ec.encode_symbol(
                        abs_level.min(LF_BASE_SYMBOLS - 1),
                        &COEFF_BASE_LF_Y_ICDF[txs_ctx_idx][ctx],
                    );
                    if abs_level > LF_NUM_BASE_LEVELS {
                        let br_ctx = get_br_lf_ctx_2d(&levels_buf, pos, bwl);
                        encode_low_range(
                            ec,
                            abs_level,
                            LF_NUM_BASE_LEVELS,
                            &COEFF_BR_LF_Y_ICDF[br_ctx],
                        );
                    }
                } else {
                    let ctx = get_lower_levels_ctx_2d(&levels_buf, pos, bwl, plane);
                    ec.encode_symbol(
                        abs_level.min(NUM_BASE_LEVELS + 1),
                        &COEFF_BASE_Y_ICDF[txs_ctx_idx][ctx],
                    );
                    if abs_level > NUM_BASE_LEVELS {
                        let br_ctx = get_br_ctx_2d(&levels_buf, pos, bwl);
                        encode_low_range(ec, abs_level, NUM_BASE_LEVELS, &COEFF_BR_Y_ICDF[br_ctx]);
                    }
                }
            } else if lf {
                let ctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, pos, bwl, plane);
                ec.encode_symbol(
                    abs_level.min(LF_BASE_SYMBOLS - 1),
                    &COEFF_BASE_LF_UV_ICDF[ctx],
                );
            } else {
                let ctx = get_lower_levels_ctx_2d_chroma(&levels_buf, pos, bwl, plane);
                ec.encode_symbol(abs_level.min(NUM_BASE_LEVELS + 1), &COEFF_BASE_UV_ICDF[ctx]);
                if abs_level > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d_chroma(&levels_buf, pos, bwl);
                    encode_low_range(ec, abs_level, NUM_BASE_LEVELS, &COEFF_BR_UV_ICDF[br_ctx]);
                }
            }

            levels_buf[get_padded_idx(pos, bwl)] = abs_level as u8;
        }

        let dc_pos = DEFAULT_SCAN_32X32[0] as usize;
        let dc_abs_level = coeffs.levels[0].unsigned_abs() as usize;
        let dc_row = dc_pos >> bwl;
        let dc_col = dc_pos - (dc_row << bwl);
        let dc_lf = is_lf_2d(dc_row, dc_col, plane);

        if plane == 0 {
            if dc_lf {
                let ctx = get_lower_levels_ctx_lf_2d(&levels_buf, dc_pos, bwl);
                ec.encode_symbol(
                    dc_abs_level.min(LF_BASE_SYMBOLS - 1),
                    &COEFF_BASE_LF_Y_ICDF[txs_ctx_idx][ctx],
                );
                if dc_abs_level > LF_NUM_BASE_LEVELS {
                    let br_ctx = get_br_lf_ctx_dc_2d(&levels_buf, dc_pos, bwl);
                    encode_low_range(
                        ec,
                        dc_abs_level,
                        LF_NUM_BASE_LEVELS,
                        &COEFF_BR_LF_Y_ICDF[br_ctx],
                    );
                }
            } else {
                let ctx = get_lower_levels_ctx_2d(&levels_buf, dc_pos, bwl, plane);
                ec.encode_symbol(
                    dc_abs_level.min(NUM_BASE_LEVELS + 1),
                    &COEFF_BASE_Y_ICDF[txs_ctx_idx][ctx],
                );
                if dc_abs_level > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d(&levels_buf, dc_pos, bwl);
                    encode_low_range(ec, dc_abs_level, NUM_BASE_LEVELS, &COEFF_BR_Y_ICDF[br_ctx]);
                }
            }
        } else if dc_lf {
            let ctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            ec.encode_symbol(
                dc_abs_level.min(LF_BASE_SYMBOLS - 1),
                &COEFF_BASE_LF_UV_ICDF[ctx],
            );
        } else {
            let ctx = get_lower_levels_ctx_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            ec.encode_symbol(
                dc_abs_level.min(NUM_BASE_LEVELS + 1),
                &COEFF_BASE_UV_ICDF[ctx],
            );
            if dc_abs_level > NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_2d_chroma(&levels_buf, dc_pos, bwl);
                encode_low_range(ec, dc_abs_level, NUM_BASE_LEVELS, &COEFF_BR_UV_ICDF[br_ctx]);
            }
        }
        levels_buf[get_padded_idx(dc_pos, bwl)] = dc_abs_level as u8;
    }

    let mut hr_avg = 0usize;
    for c in (0..eob as usize).rev() {
        let pos = DEFAULT_SCAN_32X32[c] as usize;
        let level = coeffs.levels[c];
        let abs_level = level.unsigned_abs() as usize;

        if abs_level == 0 {
            continue;
        }

        let row = pos >> bwl;
        let col = pos - (row << bwl);
        let is_dc_2d = c == 0;

        if is_dc_2d {
            if plane == 0 {
                ec.encode_symbol(if level < 0 { 1 } else { 0 }, &DC_SIGN_Y_ICDF[dc_sign_ctx]);
            } else {
                ec.encode_literal(if level < 0 { 1 } else { 0 }, 1);
            }
        } else {
            ec.encode_literal(if level < 0 { 1 } else { 0 }, 1);
        }

        let lf = is_lf_2d(row, col, plane);
        encode_high_range_full(ec, abs_level, lf, plane, &mut hr_avg);
    }
}

pub fn encode_tile_data_with_coeffs(
    y_levels: &[i32],
    y_eob: u16,
    u_levels: &[i32],
    u_eob: u16,
    v_levels: &[i32],
    v_eob: u16,
) -> Vec<u8> {
    let mut ec = DaalaCoder::new();

    ec.encode_symbol(0, &DO_SPLIT_ICDF);
    ec.encode_symbol(0, &Y_MODE_SET_ICDF);
    ec.encode_symbol(0, &Y_MODE_IDX_ICDF);
    ec.encode_symbol(0, &UV_MODE_ICDF);

    let y_coeffs = PlaneCoeffs {
        levels: y_levels,
        eob: y_eob,
    };
    encode_plane_full(&mut ec, &y_coeffs, &PlaneEncodeCtx {
        plane: 0, txs_ctx_idx: 1, bwl: 5, height: 32,
        txb_skip_icdf: &TXB_SKIP_Y_ICDF, dc_sign_ctx: 0,
    });

    let u_coeffs = PlaneCoeffs {
        levels: u_levels,
        eob: u_eob,
    };
    encode_plane_full(&mut ec, &u_coeffs, &PlaneEncodeCtx {
        plane: 1, txs_ctx_idx: 0, bwl: 5, height: 32,
        txb_skip_icdf: &TXB_SKIP_U_ICDF, dc_sign_ctx: 0,
    });

    let v_skip_icdf = if u_eob > 0 {
        &TXB_SKIP_V_U_NONZERO_ICDF
    } else {
        &TXB_SKIP_V_ICDF
    };
    let v_coeffs = PlaneCoeffs {
        levels: v_levels,
        eob: v_eob,
    };
    encode_plane_full(&mut ec, &v_coeffs, &PlaneEncodeCtx {
        plane: 2, txs_ctx_idx: 0, bwl: 5, height: 32,
        txb_skip_icdf: v_skip_icdf, dc_sign_ctx: 0,
    });

    ec.done()
}

pub fn encode_tile_data_with_dc(y_dc_level: i32, u_dc_level: i32, v_dc_level: i32) -> Vec<u8> {
    let mut y_levels = [0i32; 1024];
    let mut u_levels = [0i32; 1024];
    let mut v_levels = [0i32; 1024];
    y_levels[0] = y_dc_level;
    u_levels[0] = u_dc_level;
    v_levels[0] = v_dc_level;
    let y_eob = if y_dc_level != 0 { 1u16 } else { 0u16 };
    let u_eob = if u_dc_level != 0 { 1u16 } else { 0u16 };
    let v_eob = if v_dc_level != 0 { 1u16 } else { 0u16 };
    encode_tile_data_with_coeffs(&y_levels, y_eob, &u_levels, u_eob, &v_levels, v_eob)
}

pub fn encode_tile_data() -> Vec<u8> {
    encode_tile_data_with_dc(0, 0, 0)
}

const COEFF_CONTEXT_BITS: u32 = 3;
const COEFF_CONTEXT_MASK: u8 = (1 << COEFF_CONTEXT_BITS) - 1;

pub struct SbCoeffs {
    pub y_levels: [i32; 1024],
    pub y_eob: u16,
    pub u_levels: [i32; 1024],
    pub u_eob: u16,
    pub v_levels: [i32; 1024],
    pub v_eob: u16,
}

fn compute_cul_level(levels: &[i32], eob: u16) -> u8 {
    if eob == 0 {
        return 0;
    }
    let mut sum = 0u32;
    for item in levels.iter().take(eob as usize) {
        sum += item.unsigned_abs();
    }
    let mut cul = sum.min(COEFF_CONTEXT_MASK as u32) as u8;
    let dc = levels[0];
    if dc < 0 {
        cul |= 1 << COEFF_CONTEXT_BITS;
    } else if dc > 0 {
        cul += 2 << COEFF_CONTEXT_BITS;
    }
    cul
}

fn compute_entropy_ctx(above: &[u8], left: &[u8]) -> usize {
    let above_nz = above.iter().any(|&x| x != 0) as usize;
    let left_nz = left.iter().any(|&x| x != 0) as usize;
    above_nz + left_nz
}

fn compute_dc_sign_ctx(above: &[u8], left: &[u8]) -> usize {
    const SIGNS: [i32; 3] = [0, -1, 1];
    let mut dc_sign = 0i32;
    for &a in above {
        let sign = (a >> COEFF_CONTEXT_BITS) as usize;
        dc_sign += SIGNS[sign.min(2)];
    }
    for &l in left {
        let sign = (l >> COEFF_CONTEXT_BITS) as usize;
        dc_sign += SIGNS[sign.min(2)];
    }
    if dc_sign < 0 {
        1
    } else if dc_sign > 0 {
        2
    } else {
        0
    }
}

pub fn encode_tile_data_multi_sb(sb_cols: usize, sb_rows: usize, sb_grid: &[SbCoeffs]) -> Vec<u8> {
    let mut ec = DaalaCoder::new();

    let mut above_y = vec![0u8; sb_cols * 16];
    let mut above_u = vec![0u8; sb_cols * 8];
    let mut above_v = vec![0u8; sb_cols * 8];

    for sb_row in 0..sb_rows {
        let mut left_y = [0u8; 16];
        let mut left_u = [0u8; 8];
        let mut left_v = [0u8; 8];

        for sb_col in 0..sb_cols {
            let sb = &sb_grid[sb_row * sb_cols + sb_col];

            ec.encode_symbol(0, &DO_SPLIT_ICDF);
            ec.encode_symbol(0, &Y_MODE_SET_ICDF);
            ec.encode_symbol(0, &Y_MODE_IDX_ICDF);
            ec.encode_symbol(0, &UV_MODE_ICDF);

            let y_above = &above_y[sb_col * 16..(sb_col + 1) * 16];
            let y_dc_sign_ctx = compute_dc_sign_ctx(y_above, &left_y);
            let y_coeffs = PlaneCoeffs {
                levels: &sb.y_levels,
                eob: sb.y_eob,
            };
            encode_plane_full(&mut ec, &y_coeffs, &PlaneEncodeCtx {
                plane: 0, txs_ctx_idx: 1, bwl: 5, height: 32,
                txb_skip_icdf: &TXB_SKIP_Y_ICDF, dc_sign_ctx: y_dc_sign_ctx,
            });

            let y_cul = compute_cul_level(&sb.y_levels, sb.y_eob);
            for v in &mut above_y[sb_col * 16..(sb_col + 1) * 16] {
                *v = y_cul;
            }
            left_y.fill(y_cul);

            let u_above = &above_u[sb_col * 8..(sb_col + 1) * 8];
            let u_entropy_ctx = compute_entropy_ctx(u_above, &left_u);
            let u_coeffs = PlaneCoeffs {
                levels: &sb.u_levels,
                eob: sb.u_eob,
            };
            encode_plane_full(&mut ec, &u_coeffs, &PlaneEncodeCtx {
                plane: 1, txs_ctx_idx: 0, bwl: 5, height: 32,
                txb_skip_icdf: &TXB_SKIP_U_CTX_ICDF[u_entropy_ctx], dc_sign_ctx: 0,
            });

            let u_cul = compute_cul_level(&sb.u_levels, sb.u_eob);
            for v in &mut above_u[sb_col * 8..(sb_col + 1) * 8] {
                *v = u_cul;
            }
            left_u.fill(u_cul);

            let v_above = &above_v[sb_col * 8..(sb_col + 1) * 8];
            let v_entropy_ctx = compute_entropy_ctx(v_above, &left_v);
            let v_skip_icdf = if sb.u_eob > 0 {
                &TXB_SKIP_V_U_NONZERO_CTX_ICDF[v_entropy_ctx]
            } else {
                &TXB_SKIP_V_CTX_ICDF[v_entropy_ctx]
            };
            let v_coeffs = PlaneCoeffs {
                levels: &sb.v_levels,
                eob: sb.v_eob,
            };
            encode_plane_full(&mut ec, &v_coeffs, &PlaneEncodeCtx {
                plane: 2, txs_ctx_idx: 0, bwl: 5, height: 32,
                txb_skip_icdf: v_skip_icdf, dc_sign_ctx: 0,
            });

            let v_cul = compute_cul_level(&sb.v_levels, sb.v_eob);
            for v in &mut above_v[sb_col * 8..(sb_col + 1) * 8] {
                *v = v_cul;
            }
            left_v.fill(v_cul);
        }
    }

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

    #[test]
    fn tile_data_with_ac_coefficients() {
        let mut levels = [0i32; 1024];
        levels[0] = 100;
        levels[1] = -20;
        levels[2] = 10;
        let data = encode_tile_data_with_coeffs(&levels, 3, &[0i32; 1024], 0, &[0i32; 1024], 0);
        assert!(!data.is_empty());
    }

    #[test]
    fn tile_data_eob_1_produces_data() {
        let mut levels = [0i32; 1024];
        levels[0] = 50;
        let data = encode_tile_data_with_coeffs(&levels, 1, &[0i32; 1024], 0, &[0i32; 1024], 0);
        assert!(!data.is_empty());
    }

    #[test]
    fn encode_eob_position_1() {
        let mut ec = DaalaCoder::new();
        encode_eob(&mut ec, 1, 0);
        let data = ec.done();
        assert!(!data.is_empty());
    }

    #[test]
    fn encode_eob_various_positions() {
        for eob in [1, 2, 3, 5, 9, 17, 33, 100] {
            let mut ec = DaalaCoder::new();
            encode_eob(&mut ec, eob, 0);
            let data = ec.done();
            assert!(!data.is_empty(), "eob={} produced empty data", eob);
        }
    }

    #[test]
    fn context_padded_idx_dc() {
        assert_eq!(get_padded_idx(0, 5), 0);
    }

    #[test]
    fn context_padded_idx_second_row() {
        assert_eq!(get_padded_idx(32, 5), 32 + 4);
    }

    #[test]
    fn context_eob_scan_0() {
        assert_eq!(get_lower_levels_ctx_eob(5, 32, 0), 0);
    }

    #[test]
    fn is_lf_dc_is_lf() {
        assert!(is_lf_2d(0, 0, 0));
        assert!(is_lf_2d(0, 0, 1));
    }

    #[test]
    fn compute_cul_level_zero() {
        let levels = [0i32; 1024];
        assert_eq!(compute_cul_level(&levels, 0), 0);
    }

    #[test]
    fn compute_cul_level_positive_dc() {
        let mut levels = [0i32; 1024];
        levels[0] = 5;
        let cul = compute_cul_level(&levels, 1);
        assert_eq!(cul & COEFF_CONTEXT_MASK, 5);
        assert_eq!(cul >> COEFF_CONTEXT_BITS, 2);
    }

    #[test]
    fn compute_cul_level_negative_dc() {
        let mut levels = [0i32; 1024];
        levels[0] = -3;
        let cul = compute_cul_level(&levels, 1);
        assert_eq!(cul & COEFF_CONTEXT_MASK, 3);
        assert_eq!(cul >> COEFF_CONTEXT_BITS, 1);
    }

    #[test]
    fn compute_cul_level_clamped() {
        let mut levels = [0i32; 1024];
        levels[0] = 100;
        let cul = compute_cul_level(&levels, 1);
        assert_eq!(cul & COEFF_CONTEXT_MASK, 7);
    }

    #[test]
    fn entropy_ctx_both_zero() {
        assert_eq!(compute_entropy_ctx(&[0, 0, 0, 0], &[0, 0, 0, 0]), 0);
    }

    #[test]
    fn entropy_ctx_above_nonzero() {
        assert_eq!(compute_entropy_ctx(&[1, 0, 0, 0], &[0, 0, 0, 0]), 1);
    }

    #[test]
    fn entropy_ctx_both_nonzero() {
        assert_eq!(compute_entropy_ctx(&[1, 0, 0, 0], &[0, 0, 1, 0]), 2);
    }

    #[test]
    fn dc_sign_ctx_all_zero() {
        assert_eq!(compute_dc_sign_ctx(&[0; 16], &[0; 16]), 0);
    }

    #[test]
    fn dc_sign_ctx_positive() {
        let mut above = [0u8; 16];
        above.fill(2 << COEFF_CONTEXT_BITS | 1);
        assert_eq!(compute_dc_sign_ctx(&above, &[0; 16]), 2);
    }

    #[test]
    fn dc_sign_ctx_negative() {
        let mut left = [0u8; 16];
        left.fill(1 << COEFF_CONTEXT_BITS | 1);
        assert_eq!(compute_dc_sign_ctx(&[0; 16], &left), 1);
    }

    #[test]
    fn multi_sb_1x1_matches_single_sb() {
        let mut sb = SbCoeffs {
            y_levels: [0i32; 1024],
            y_eob: 0,
            u_levels: [0i32; 1024],
            u_eob: 0,
            v_levels: [0i32; 1024],
            v_eob: 0,
        };
        sb.y_levels[0] = 50;
        sb.y_eob = 1;
        sb.u_levels[0] = -10;
        sb.u_eob = 1;

        let single = encode_tile_data_with_coeffs(
            &sb.y_levels,
            sb.y_eob,
            &sb.u_levels,
            sb.u_eob,
            &sb.v_levels,
            sb.v_eob,
        );
        let multi = encode_tile_data_multi_sb(1, 1, &[sb]);
        assert_eq!(single, multi, "1x1 multi-SB should match single-SB encoding");
    }

    #[test]
    fn multi_sb_2x2_nonempty() {
        let sbs: Vec<SbCoeffs> = (0..4)
            .map(|_| SbCoeffs {
                y_levels: [0i32; 1024],
                y_eob: 0,
                u_levels: [0i32; 1024],
                u_eob: 0,
                v_levels: [0i32; 1024],
                v_eob: 0,
            })
            .collect();
        let data = encode_tile_data_multi_sb(2, 2, &sbs);
        assert!(!data.is_empty());
    }

    #[test]
    fn multi_sb_2x2_with_coeffs() {
        let sbs: Vec<SbCoeffs> = (0..4)
            .map(|i| {
                let mut sb = SbCoeffs {
                    y_levels: [0i32; 1024],
                    y_eob: 0,
                    u_levels: [0i32; 1024],
                    u_eob: 0,
                    v_levels: [0i32; 1024],
                    v_eob: 0,
                };
                sb.y_levels[0] = (i + 1) * 10;
                sb.y_eob = 1;
                sb.u_levels[0] = (i + 1) * 5;
                sb.u_eob = 1;
                sb.v_levels[0] = (i + 1) * 3;
                sb.v_eob = 1;
                sb
            })
            .collect();
        let data = encode_tile_data_multi_sb(2, 2, &sbs);
        assert!(!data.is_empty());
    }
}
