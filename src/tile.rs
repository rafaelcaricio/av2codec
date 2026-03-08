use crate::cdf_context::FrameCdfContext;
use crate::cdf_tables::*;
use crate::entropy::DaalaCoder;

fn scan_for_num_coeffs(num_coeffs: usize) -> &'static [u16] {
    match num_coeffs {
        16 => &crate::scan::DEFAULT_SCAN_4X4,
        64 => &crate::scan::DEFAULT_SCAN_8X8,
        256 => &crate::scan::DEFAULT_SCAN_16X16,
        1024 => &crate::scan::DEFAULT_SCAN_32X32,
        _ => panic!("unsupported num_coeffs: {}", num_coeffs),
    }
}

fn encode_eob_var_size(
    ec: &mut DaalaCoder,
    eob: u16,
    num_coeffs: usize,
    pl_ctx: usize,
    mut cdf_ctx: Option<&mut FrameCdfContext>,
) {
    let mut eob_pt = 0usize;
    for i in (0..EOB_GROUP_START.len()).rev() {
        if eob >= EOB_GROUP_START[i] {
            if i == 0 && eob == 0 { continue; }
            eob_pt = i;
            break;
        }
    }

    let eob_token = eob_pt - 1;

    let extra_bits: u32 = match num_coeffs {
        16 => {
            if let Some(ref mut ctx) = cdf_ctx {
                ec.encode_symbol_mut(eob_token, &mut ctx.eob_multi_16[pl_ctx]);
            } else {
                ec.encode_symbol(eob_token, &EOB_MULTI_16_ICDF[pl_ctx]);
            }
            0
        }
        64 => {
            if let Some(ref mut ctx) = cdf_ctx {
                ec.encode_symbol_mut(eob_token, &mut ctx.eob_multi_64[pl_ctx]);
            } else {
                ec.encode_symbol(eob_token, &EOB_MULTI_64_ICDF[pl_ctx]);
            }
            0
        }
        256 => {
            if eob_token < EOB_PT_INDEX_COUNT - 1 {
                if let Some(ref mut ctx) = cdf_ctx {
                    ec.encode_symbol_mut(eob_token, &mut ctx.eob_multi_256[pl_ctx]);
                } else {
                    ec.encode_symbol(eob_token, &EOB_MULTI_256_ICDF[pl_ctx]);
                }
            } else {
                if let Some(ref mut ctx) = cdf_ctx {
                    ec.encode_symbol_mut(EOB_PT_INDEX_COUNT - 1, &mut ctx.eob_multi_256[pl_ctx]);
                } else {
                    ec.encode_symbol(EOB_PT_INDEX_COUNT - 1, &EOB_MULTI_256_ICDF[pl_ctx]);
                }
                let extra_token = eob_token - (EOB_PT_INDEX_COUNT - 1);
                ec.encode_literal(extra_token as u32, 1);
            }
            0
        }
        1024 => {
            if eob_token < EOB_PT_INDEX_COUNT - 1 {
                if let Some(ref mut ctx) = cdf_ctx {
                    ec.encode_symbol_mut(eob_token, &mut ctx.eob_multi_1024[pl_ctx]);
                } else {
                    ec.encode_symbol(eob_token, &EOB_MULTI_1024_ICDF[pl_ctx]);
                }
            } else {
                if let Some(ref mut ctx) = cdf_ctx {
                    ec.encode_symbol_mut(EOB_PT_INDEX_COUNT - 1, &mut ctx.eob_multi_1024[pl_ctx]);
                } else {
                    ec.encode_symbol(EOB_PT_INDEX_COUNT - 1, &EOB_MULTI_1024_ICDF[pl_ctx]);
                }
                let extra_token = eob_token - (EOB_PT_INDEX_COUNT - 1);
                ec.encode_literal(extra_token as u32, 2);
            }
            0
        }
        _ => panic!("unsupported num_coeffs for EOB: {}", num_coeffs),
    };
    let _ = extra_bits;

    let eob_offset_bits = EOB_OFFSET_BITS[eob_pt] as u32;
    if eob_offset_bits > 0 {
        let extra = eob - EOB_GROUP_START[eob_pt];
        let msb = (extra >> (eob_offset_bits - 1)) & 1;
        if let Some(ref mut ctx) = cdf_ctx {
            ec.encode_symbol_mut(msb as usize, &mut ctx.eob_extra);
        } else {
            ec.encode_symbol(msb as usize, &EOB_EXTRA_ICDF);
        }
        if eob_offset_bits > 1 {
            let rest = extra & ((1 << (eob_offset_bits - 1)) - 1);
            ec.encode_literal(rest as u32, eob_offset_bits - 1);
        }
    }
}

const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];
const Y_MODE_SET_ICDF: [u16; 4] = [3905, 1746, 1044, 0];
const Y_MODE_IDX_ICDF: [u16; 8] = [17593, 12693, 11040, 8670, 6363, 5113, 3908, 0];
const UV_MODE_ICDF: [u16; 8] = [23405, 11811, 9903, 8015, 6357, 4785, 2340, 0];
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

fn encode_low_range(
    ec: &mut DaalaCoder,
    level: usize,
    base_levels: usize,
    cdf: &[u16],
    mutable_cdf: Option<&mut Vec<u16>>,
) {
    if level > base_levels {
        let remainder = level - 1 - base_levels;
        let sym = remainder.min(BR_CDF_SIZE - 1);
        if let Some(mcdf) = mutable_cdf {
            ec.encode_symbol_mut(sym, mcdf);
        } else {
            ec.encode_symbol(sym, cdf);
        }
    }
}

fn encode_eob(
    ec: &mut DaalaCoder,
    eob: u16,
    pl_ctx: usize,
    mut cdf_ctx: Option<&mut FrameCdfContext>,
) {
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
        if let Some(ref mut ctx) = cdf_ctx {
            ec.encode_symbol_mut(eob_token, &mut ctx.eob_multi_1024[pl_ctx]);
        } else {
            ec.encode_symbol(eob_token, eob_icdf);
        }
    } else {
        if let Some(ref mut ctx) = cdf_ctx {
            ec.encode_symbol_mut(EOB_PT_INDEX_COUNT - 1, &mut ctx.eob_multi_1024[pl_ctx]);
        } else {
            ec.encode_symbol(EOB_PT_INDEX_COUNT - 1, eob_icdf);
        }
        let extra_token = eob_token - (EOB_PT_INDEX_COUNT - 1);
        ec.encode_literal(extra_token as u32, 2);
    }

    let eob_offset_bits = EOB_OFFSET_BITS[eob_pt] as u32;
    if eob_offset_bits > 0 {
        let extra = eob - EOB_GROUP_START[eob_pt];
        let msb = (extra >> (eob_offset_bits - 1)) & 1;
        if let Some(ref mut ctx) = cdf_ctx {
            ec.encode_symbol_mut(msb as usize, &mut ctx.eob_extra);
        } else {
            ec.encode_symbol(msb as usize, &EOB_EXTRA_ICDF);
        }
        if eob_offset_bits > 1 {
            let rest = extra & ((1 << (eob_offset_bits - 1)) - 1);
            ec.encode_literal(rest as u32, eob_offset_bits - 1);
        }
    }
}

fn stored_level(abs_level: usize, lf: bool, plane: usize) -> u8 {
    let max = if lf {
        if plane == 0 {
            LF_MAX_BASE_BR_RANGE
        } else {
            LF_BASE_SYMBOLS - 1
        }
    } else {
        MAX_BASE_BR_RANGE
    };
    abs_level.min(max) as u8
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
    num_coeffs: usize,
    txb_skip_icdf: &'a [u16],
    dc_sign_ctx: usize,
    entropy_ctx: usize,
    u_has_coeffs: bool,
}

fn encode_plane_full(
    ec: &mut DaalaCoder,
    coeffs: &PlaneCoeffs,
    ctx: &PlaneEncodeCtx,
    mut cdf_ctx: Option<&mut FrameCdfContext>,
) {
    let plane = ctx.plane;
    let txs_ctx_idx = ctx.txs_ctx_idx;
    let bwl = ctx.bwl;
    let height = ctx.height;
    let txb_skip_icdf = ctx.txb_skip_icdf;
    let dc_sign_ctx = ctx.dc_sign_ctx;
    let eob = coeffs.eob;

    let skip_sym = if eob == 0 { 1 } else { 0 };
    if let Some(ref mut cdf) = cdf_ctx {
        let txb_skip_cdf = match plane {
            0 => &mut cdf.txb_skip_y[txs_ctx_idx][ctx.entropy_ctx],
            1 => &mut cdf.txb_skip_u[ctx.entropy_ctx],
            _ => {
                if ctx.u_has_coeffs {
                    &mut cdf.txb_skip_v_u_nz[ctx.entropy_ctx]
                } else {
                    &mut cdf.txb_skip_v[ctx.entropy_ctx]
                }
            }
        };
        ec.encode_symbol_mut(skip_sym, txb_skip_cdf);
    } else {
        ec.encode_symbol(skip_sym, txb_skip_icdf);
    }

    if eob == 0 {
        return;
    }

    let pl_ctx = if plane == 0 { 0 } else { 2 };
    let num_coeffs = ctx.num_coeffs;
    encode_eob_var_size(ec, eob, num_coeffs, pl_ctx, cdf_ctx.as_deref_mut());

    let scan = scan_for_num_coeffs(num_coeffs);
    let width = 1usize << bwl;
    let padded_stride = width + TX_PAD_HOR;
    let padded_height = height + 2;
    let mut levels_buf = vec![0u8; padded_stride * padded_height];

    let eob_scan_idx = (eob - 1) as usize;
    let eob_pos = scan[eob_scan_idx] as usize;
    let eob_row = eob_pos >> bwl;
    let eob_col = eob_pos - (eob_row << bwl);
    let eob_lf = is_lf_2d(eob_row, eob_col, plane);
    let eob_abs_level = coeffs.levels[eob_scan_idx].unsigned_abs() as usize;

    let coeff_ctx = get_lower_levels_ctx_eob(bwl, height, eob_scan_idx);

    if plane == 0 {
        if eob_lf {
            let sym = eob_abs_level.min(LF_BASE_SYMBOLS - 1) - 1;
            if let Some(ref mut cdf) = cdf_ctx {
                ec.encode_symbol_mut(sym, &mut cdf.coeff_base_lf_eob_y[txs_ctx_idx][coeff_ctx]);
            } else {
                ec.encode_symbol(sym, &COEFF_BASE_LF_EOB_Y_ICDF[txs_ctx_idx][coeff_ctx]);
            }
            if eob_abs_level > LF_NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_lf_eob_2d(eob_pos);
                encode_low_range(
                    ec,
                    eob_abs_level,
                    LF_NUM_BASE_LEVELS,
                    &COEFF_BR_LF_Y_ICDF[br_ctx],
                    cdf_ctx.as_deref_mut().map(|c| &mut c.coeff_br_lf_y[br_ctx]),
                );
            }
        } else {
            let sym = eob_abs_level.min(NUM_BASE_LEVELS + 1) - 1;
            if let Some(ref mut cdf) = cdf_ctx {
                ec.encode_symbol_mut(sym, &mut cdf.coeff_base_eob_y[txs_ctx_idx][coeff_ctx]);
            } else {
                ec.encode_symbol(sym, &COEFF_BASE_EOB_Y_ICDF[txs_ctx_idx][coeff_ctx]);
            }
            if eob_abs_level > NUM_BASE_LEVELS {
                encode_low_range(
                    ec,
                    eob_abs_level,
                    NUM_BASE_LEVELS,
                    &COEFF_BR_Y_ICDF[0],
                    cdf_ctx.as_deref_mut().map(|c| &mut c.coeff_br_y[0]),
                );
            }
        }
    } else if eob_lf {
        let sym = eob_abs_level.min(LF_BASE_SYMBOLS - 1) - 1;
        if let Some(ref mut cdf) = cdf_ctx {
            ec.encode_symbol_mut(sym, &mut cdf.coeff_base_lf_eob_uv[coeff_ctx]);
        } else {
            ec.encode_symbol(sym, &COEFF_BASE_LF_EOB_UV_ICDF[coeff_ctx]);
        }
    } else {
        let sym = eob_abs_level.min(NUM_BASE_LEVELS + 1) - 1;
        if let Some(ref mut cdf) = cdf_ctx {
            ec.encode_symbol_mut(sym, &mut cdf.coeff_base_eob_uv[coeff_ctx]);
        } else {
            ec.encode_symbol(sym, &COEFF_BASE_EOB_UV_ICDF[coeff_ctx]);
        }
        if eob_abs_level > NUM_BASE_LEVELS {
            encode_low_range(
                ec,
                eob_abs_level,
                NUM_BASE_LEVELS,
                &COEFF_BR_UV_ICDF[0],
                cdf_ctx.as_deref_mut().map(|c| &mut c.coeff_br_uv[0]),
            );
        }
    }

    levels_buf[get_padded_idx(eob_pos, bwl)] = stored_level(eob_abs_level, eob_lf, plane);

    if eob > 1 {
        for c in (1..eob_scan_idx).rev() {
            let pos = scan[c] as usize;
            let abs_level = coeffs.levels[c].unsigned_abs() as usize;
            let row = pos >> bwl;
            let col = pos - (row << bwl);
            let lf = is_lf_2d(row, col, plane);

            if plane == 0 {
                if lf {
                    let lctx = get_lower_levels_ctx_lf_2d(&levels_buf, pos, bwl);
                    if let Some(ref mut cdf) = cdf_ctx {
                        ec.encode_symbol_mut(
                            abs_level.min(LF_BASE_SYMBOLS - 1),
                            &mut cdf.coeff_base_lf_y[txs_ctx_idx][lctx][0],
                        );
                    } else {
                        ec.encode_symbol(
                            abs_level.min(LF_BASE_SYMBOLS - 1),
                            &COEFF_BASE_LF_Y_ICDF[txs_ctx_idx][lctx],
                        );
                    }
                    if abs_level > LF_NUM_BASE_LEVELS {
                        let br_ctx = get_br_lf_ctx_2d(&levels_buf, pos, bwl);
                        encode_low_range(
                            ec,
                            abs_level,
                            LF_NUM_BASE_LEVELS,
                            &COEFF_BR_LF_Y_ICDF[br_ctx],
                            cdf_ctx
                                .as_deref_mut()
                                .map(|cd| &mut cd.coeff_br_lf_y[br_ctx]),
                        );
                    }
                } else {
                    let lctx = get_lower_levels_ctx_2d(&levels_buf, pos, bwl, plane);
                    if let Some(ref mut cdf) = cdf_ctx {
                        ec.encode_symbol_mut(
                            abs_level.min(NUM_BASE_LEVELS + 1),
                            &mut cdf.coeff_base_y[txs_ctx_idx][lctx][0],
                        );
                    } else {
                        ec.encode_symbol(
                            abs_level.min(NUM_BASE_LEVELS + 1),
                            &COEFF_BASE_Y_ICDF[txs_ctx_idx][lctx],
                        );
                    }
                    if abs_level > NUM_BASE_LEVELS {
                        let br_ctx = get_br_ctx_2d(&levels_buf, pos, bwl);
                        encode_low_range(
                            ec,
                            abs_level,
                            NUM_BASE_LEVELS,
                            &COEFF_BR_Y_ICDF[br_ctx],
                            cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_y[br_ctx]),
                        );
                    }
                }
            } else if lf {
                let lctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, pos, bwl, plane);
                if let Some(ref mut cdf) = cdf_ctx {
                    ec.encode_symbol_mut(
                        abs_level.min(LF_BASE_SYMBOLS - 1),
                        &mut cdf.coeff_base_lf_uv[lctx][0],
                    );
                } else {
                    ec.encode_symbol(
                        abs_level.min(LF_BASE_SYMBOLS - 1),
                        &COEFF_BASE_LF_UV_ICDF[lctx],
                    );
                }
            } else {
                let lctx = get_lower_levels_ctx_2d_chroma(&levels_buf, pos, bwl, plane);
                if let Some(ref mut cdf) = cdf_ctx {
                    ec.encode_symbol_mut(
                        abs_level.min(NUM_BASE_LEVELS + 1),
                        &mut cdf.coeff_base_uv[lctx][0],
                    );
                } else {
                    ec.encode_symbol(
                        abs_level.min(NUM_BASE_LEVELS + 1),
                        &COEFF_BASE_UV_ICDF[lctx],
                    );
                }
                if abs_level > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d_chroma(&levels_buf, pos, bwl);
                    encode_low_range(
                        ec,
                        abs_level,
                        NUM_BASE_LEVELS,
                        &COEFF_BR_UV_ICDF[br_ctx],
                        cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_uv[br_ctx]),
                    );
                }
            }

            levels_buf[get_padded_idx(pos, bwl)] = stored_level(abs_level, lf, plane);
        }

        let dc_pos = scan[0] as usize;
        let dc_abs_level = coeffs.levels[0].unsigned_abs() as usize;
        let dc_row = dc_pos >> bwl;
        let dc_col = dc_pos - (dc_row << bwl);
        let dc_lf = is_lf_2d(dc_row, dc_col, plane);

        if plane == 0 {
            if dc_lf {
                let lctx = get_lower_levels_ctx_lf_2d(&levels_buf, dc_pos, bwl);
                if let Some(ref mut cdf) = cdf_ctx {
                    ec.encode_symbol_mut(
                        dc_abs_level.min(LF_BASE_SYMBOLS - 1),
                        &mut cdf.coeff_base_lf_y[txs_ctx_idx][lctx][0],
                    );
                } else {
                    ec.encode_symbol(
                        dc_abs_level.min(LF_BASE_SYMBOLS - 1),
                        &COEFF_BASE_LF_Y_ICDF[txs_ctx_idx][lctx],
                    );
                }
                if dc_abs_level > LF_NUM_BASE_LEVELS {
                    let br_ctx = get_br_lf_ctx_dc_2d(&levels_buf, dc_pos, bwl);
                    encode_low_range(
                        ec,
                        dc_abs_level,
                        LF_NUM_BASE_LEVELS,
                        &COEFF_BR_LF_Y_ICDF[br_ctx],
                        cdf_ctx
                            .as_deref_mut()
                            .map(|cd| &mut cd.coeff_br_lf_y[br_ctx]),
                    );
                }
            } else {
                let lctx = get_lower_levels_ctx_2d(&levels_buf, dc_pos, bwl, plane);
                if let Some(ref mut cdf) = cdf_ctx {
                    ec.encode_symbol_mut(
                        dc_abs_level.min(NUM_BASE_LEVELS + 1),
                        &mut cdf.coeff_base_y[txs_ctx_idx][lctx][0],
                    );
                } else {
                    ec.encode_symbol(
                        dc_abs_level.min(NUM_BASE_LEVELS + 1),
                        &COEFF_BASE_Y_ICDF[txs_ctx_idx][lctx],
                    );
                }
                if dc_abs_level > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d(&levels_buf, dc_pos, bwl);
                    encode_low_range(
                        ec,
                        dc_abs_level,
                        NUM_BASE_LEVELS,
                        &COEFF_BR_Y_ICDF[br_ctx],
                        cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_y[br_ctx]),
                    );
                }
            }
        } else if dc_lf {
            let lctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            if let Some(ref mut cdf) = cdf_ctx {
                ec.encode_symbol_mut(
                    dc_abs_level.min(LF_BASE_SYMBOLS - 1),
                    &mut cdf.coeff_base_lf_uv[lctx][0],
                );
            } else {
                ec.encode_symbol(
                    dc_abs_level.min(LF_BASE_SYMBOLS - 1),
                    &COEFF_BASE_LF_UV_ICDF[lctx],
                );
            }
        } else {
            let lctx = get_lower_levels_ctx_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            if let Some(ref mut cdf) = cdf_ctx {
                ec.encode_symbol_mut(
                    dc_abs_level.min(NUM_BASE_LEVELS + 1),
                    &mut cdf.coeff_base_uv[lctx][0],
                );
            } else {
                ec.encode_symbol(
                    dc_abs_level.min(NUM_BASE_LEVELS + 1),
                    &COEFF_BASE_UV_ICDF[lctx],
                );
            }
            if dc_abs_level > NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_2d_chroma(&levels_buf, dc_pos, bwl);
                encode_low_range(
                    ec,
                    dc_abs_level,
                    NUM_BASE_LEVELS,
                    &COEFF_BR_UV_ICDF[br_ctx],
                    cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_uv[br_ctx]),
                );
            }
        }
        levels_buf[get_padded_idx(dc_pos, bwl)] = stored_level(dc_abs_level, dc_lf, plane);
    }

    let mut hr_avg = 0usize;
    for c in (0..eob as usize).rev() {
        let pos = scan[c] as usize;
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
                if let Some(ref mut cdf) = cdf_ctx {
                    ec.encode_symbol_mut(
                        if level < 0 { 1 } else { 0 },
                        &mut cdf.dc_sign_y[dc_sign_ctx],
                    );
                } else {
                    ec.encode_symbol(if level < 0 { 1 } else { 0 }, &DC_SIGN_Y_ICDF[dc_sign_ctx]);
                }
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

#[allow(clippy::too_many_arguments)]
pub fn encode_tile_data_with_coeffs(
    y_levels: &[i32],
    y_eob: u16,
    u_levels: &[i32],
    u_eob: u16,
    v_levels: &[i32],
    v_eob: u16,
    y_mode: u8,
    uv_mode: u8,
) -> Vec<u8> {
    let mut ec = DaalaCoder::new();

    ec.encode_symbol(0, &DO_SPLIT_ICDF);
    ec.encode_symbol(0, &Y_MODE_SET_ICDF);
    ec.encode_symbol(y_mode as usize, &Y_MODE_IDX_ICDF);
    ec.encode_symbol(uv_mode as usize, &UV_MODE_ICDF);

    let y_coeffs = PlaneCoeffs {
        levels: y_levels,
        eob: y_eob,
    };
    encode_plane_full(
        &mut ec,
        &y_coeffs,
        &PlaneEncodeCtx {
            plane: 0,
            txs_ctx_idx: 4,
            bwl: 5,
            height: 32,
            num_coeffs: 1024,
            txb_skip_icdf: &TXB_SKIP_Y_ICDF[4][0],
            dc_sign_ctx: 0,
            entropy_ctx: 0,
            u_has_coeffs: false,
        },
        None,
    );

    let u_coeffs = PlaneCoeffs {
        levels: u_levels,
        eob: u_eob,
    };
    encode_plane_full(
        &mut ec,
        &u_coeffs,
        &PlaneEncodeCtx {
            plane: 1,
            txs_ctx_idx: 3,
            bwl: 5,
            height: 32,
            num_coeffs: 1024,
            txb_skip_icdf: &TXB_SKIP_U_ICDF,
            dc_sign_ctx: 0,
            entropy_ctx: 0,
            u_has_coeffs: false,
        },
        None,
    );

    let v_skip_icdf = if u_eob > 0 {
        &TXB_SKIP_V_U_NONZERO_ICDF
    } else {
        &TXB_SKIP_V_ICDF
    };
    let v_coeffs = PlaneCoeffs {
        levels: v_levels,
        eob: v_eob,
    };
    encode_plane_full(
        &mut ec,
        &v_coeffs,
        &PlaneEncodeCtx {
            plane: 2,
            txs_ctx_idx: 3,
            bwl: 5,
            height: 32,
            num_coeffs: 1024,
            txb_skip_icdf: v_skip_icdf,
            dc_sign_ctx: 0,
            entropy_ctx: 0,
            u_has_coeffs: u_eob > 0,
        },
        None,
    );

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
    encode_tile_data_with_coeffs(&y_levels, y_eob, &u_levels, u_eob, &v_levels, v_eob, 0, 0)
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
    pub y_mode: u8,
    pub uv_mode: u8,
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

pub fn encode_tile_data_multi_sb(
    sb_cols: usize,
    sb_rows: usize,
    sb_grid: &[SbCoeffs],
    allow_update_cdf: bool,
) -> Vec<u8> {
    let mut ec = DaalaCoder::new();

    let mut cdf_ctx = if allow_update_cdf {
        Some(FrameCdfContext::new())
    } else {
        None
    };

    let mut above_y = vec![0u8; sb_cols * 16];
    let mut above_u = vec![0u8; sb_cols * 8];
    let mut above_v = vec![0u8; sb_cols * 8];

    for sb_row in 0..sb_rows {
        let mut left_y = [0u8; 16];
        let mut left_u = [0u8; 8];
        let mut left_v = [0u8; 8];

        for sb_col in 0..sb_cols {
            let sb = &sb_grid[sb_row * sb_cols + sb_col];

            if let Some(ref mut ctx) = cdf_ctx {
                ec.encode_symbol_mut(0, &mut ctx.do_split);
                ec.encode_symbol_mut(0, &mut ctx.y_mode_set);
                ec.encode_symbol_mut(sb.y_mode as usize, &mut ctx.y_mode_idx[0]);
                ec.encode_symbol_mut(sb.uv_mode as usize, &mut ctx.uv_mode[0]);
            } else {
                ec.encode_symbol(0, &DO_SPLIT_ICDF);
                ec.encode_symbol(0, &Y_MODE_SET_ICDF);
                ec.encode_symbol(sb.y_mode as usize, &Y_MODE_IDX_ICDF);
                ec.encode_symbol(sb.uv_mode as usize, &UV_MODE_ICDF);
            }

            let y_above = &above_y[sb_col * 16..(sb_col + 1) * 16];
            let y_dc_sign_ctx = compute_dc_sign_ctx(y_above, &left_y);
            let y_coeffs = PlaneCoeffs {
                levels: &sb.y_levels,
                eob: sb.y_eob,
            };
            encode_plane_full(
                &mut ec,
                &y_coeffs,
                &PlaneEncodeCtx {
                    plane: 0,
                    txs_ctx_idx: 4,
                    bwl: 5,
                    height: 32,
            num_coeffs: 1024,
                    txb_skip_icdf: &TXB_SKIP_Y_ICDF[4][0],
                    dc_sign_ctx: y_dc_sign_ctx,
                    entropy_ctx: 0,
                    u_has_coeffs: false,
                },
                cdf_ctx.as_mut(),
            );

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
            encode_plane_full(
                &mut ec,
                &u_coeffs,
                &PlaneEncodeCtx {
                    plane: 1,
                    txs_ctx_idx: 3,
                    bwl: 5,
                    height: 32,
            num_coeffs: 1024,
                    txb_skip_icdf: &TXB_SKIP_U_CTX_ICDF[u_entropy_ctx],
                    dc_sign_ctx: 0,
                    entropy_ctx: u_entropy_ctx,
                    u_has_coeffs: false,
                },
                cdf_ctx.as_mut(),
            );

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
            encode_plane_full(
                &mut ec,
                &v_coeffs,
                &PlaneEncodeCtx {
                    plane: 2,
                    txs_ctx_idx: 3,
                    bwl: 5,
                    height: 32,
            num_coeffs: 1024,
                    txb_skip_icdf: v_skip_icdf,
                    dc_sign_ctx: 0,
                    entropy_ctx: v_entropy_ctx,
                    u_has_coeffs: sb.u_eob > 0,
                },
                cdf_ctx.as_mut(),
            );

            let v_cul = compute_cul_level(&sb.v_levels, sb.v_eob);
            for v in &mut above_v[sb_col * 8..(sb_col + 1) * 8] {
                *v = v_cul;
            }
            left_v.fill(v_cul);
        }
    }

    ec.done()
}

use crate::encode::EncodedSuperblock;

fn bwl_for_bsize(bsize: usize) -> u32 {
    match bsize {
        4 => 2, 8 => 3, 16 => 4, 32 | 64 => 5,
        _ => panic!("unsupported bsize {}", bsize),
    }
}

fn height_for_bsize(bsize: usize) -> usize {
    match bsize {
        4 => 4, 8 => 8, 16 => 16, 32 | 64 => 32,
        _ => panic!("unsupported bsize {}", bsize),
    }
}

fn txs_ctx_idx_for_bsize(bsize: usize) -> usize {
    match bsize {
        4 => 0, 8 => 1, 16 => 2, 32 => 3, 64 => 4,
        _ => panic!("unsupported bsize {}", bsize),
    }
}

fn num_coeffs_for_bsize(bsize: usize) -> usize {
    match bsize {
        4 => 16, 8 => 64, 16 => 256, 32 | 64 => 1024,
        _ => panic!("unsupported bsize {}", bsize),
    }
}

enum PartitionNode {
    Leaf(usize),
    Split([Box<PartitionNode>; 4]),
}

fn build_partition_tree(blocks: &[crate::encode::EncodedBlock], sb_x: usize, sb_y: usize) -> PartitionNode {
    build_partition_subtree(blocks, sb_x, sb_y, 64)
}

fn build_partition_subtree(
    blocks: &[crate::encode::EncodedBlock],
    x: usize,
    y: usize,
    bsize: usize,
) -> PartitionNode {
    if let Some(idx) = blocks.iter().position(|b| b.x == x && b.y == y && b.bsize == bsize) {
        return PartitionNode::Leaf(idx);
    }
    let half = bsize / 2;
    PartitionNode::Split([
        Box::new(build_partition_subtree(blocks, x, y, half)),
        Box::new(build_partition_subtree(blocks, x + half, y, half)),
        Box::new(build_partition_subtree(blocks, x, y + half, half)),
        Box::new(build_partition_subtree(blocks, x + half, y + half, half)),
    ])
}

fn partition_split_ctx(above_part: &[u8], left_part: &[u8], bsize: usize) -> usize {
    let bsize_group = match bsize {
        64 => 3, 32 => 2, 16 => 1, _ => 0,
    };
    let shift = match bsize {
        64 => 3, 32 => 2, 16 => 1, _ => 0,
    };
    let above_val = above_part.first().copied().unwrap_or(0);
    let left_val = left_part.first().copied().unwrap_or(0);
    let above_bit = (above_val >> shift) & 1;
    let left_bit = (left_val >> shift) & 1;
    bsize_group * 4 + (left_bit as usize) * 2 + (above_bit as usize)
}

fn sq_split_ctx(above_part: &[u8], left_part: &[u8], bsize: usize) -> usize {
    let shift = match bsize { 64 => 3, 32 => 2, 16 => 1, _ => 0 };
    let above_val = above_part.first().copied().unwrap_or(0);
    let left_val = left_part.first().copied().unwrap_or(0);
    let above_bit = (above_val >> shift) & 1;
    let left_bit = (left_val >> shift) & 1;
    (left_bit as usize) * 2 + (above_bit as usize)
}

fn encode_partition_and_coeffs(
    ec: &mut DaalaCoder,
    node: &PartitionNode,
    blocks: &[crate::encode::EncodedBlock],
    bsize: usize,
    above_part: &[u8],
    left_part: &[u8],
    above_y: &[u8],
    left_y: &[u8; 16],
    above_u: &[u8],
    left_u: &[u8; 8],
    above_v: &[u8],
    left_v: &[u8; 8],
    cdf_ctx: Option<&mut FrameCdfContext>,
) {
    match node {
        PartitionNode::Leaf(idx) => {
            let block = &blocks[*idx];
            let txs_ctx = txs_ctx_idx_for_bsize(bsize);
            let bwl = bwl_for_bsize(bsize);
            let h = height_for_bsize(bsize);
            let nc = num_coeffs_for_bsize(bsize);

            let uv_bsize = bsize / 2;
            let uv_txs_ctx = txs_ctx_idx_for_bsize(uv_bsize);
            let uv_bwl = bwl_for_bsize(uv_bsize);
            let uv_h = height_for_bsize(uv_bsize);
            let uv_nc = num_coeffs_for_bsize(uv_bsize);

            if let Some(cdf) = cdf_ctx {
                if bsize > 8 {
                    let ctx = partition_split_ctx(above_part, left_part, bsize);
                    ec.encode_symbol_mut(0, &mut cdf.do_split_part[0][ctx.min(63)]);
                }
                ec.encode_symbol_mut(0, &mut cdf.y_mode_set);
                ec.encode_symbol_mut(block.mode as usize, &mut cdf.y_mode_idx[0]);
                ec.encode_symbol_mut(block.uv_mode as usize, &mut cdf.uv_mode[0]);

                let y_dc_sign_ctx = compute_dc_sign_ctx(above_y, left_y);
                let coeffs = PlaneCoeffs { levels: &block.levels, eob: block.eob };
                encode_plane_full(ec, &coeffs, &PlaneEncodeCtx {
                    plane: 0, txs_ctx_idx: txs_ctx, bwl, height: h, num_coeffs: nc,
                    txb_skip_icdf: &TXB_SKIP_Y_ICDF[txs_ctx][0],
                    dc_sign_ctx: y_dc_sign_ctx, entropy_ctx: 0, u_has_coeffs: false,
                }, Some(cdf));

                let u_entropy_ctx = compute_entropy_ctx(above_u, left_u);
                let u_coeffs = PlaneCoeffs { levels: &block.u_levels, eob: block.u_eob };
                encode_plane_full(ec, &u_coeffs, &PlaneEncodeCtx {
                    plane: 1, txs_ctx_idx: uv_txs_ctx, bwl: uv_bwl, height: uv_h, num_coeffs: uv_nc,
                    txb_skip_icdf: &TXB_SKIP_U_CTX_ICDF[u_entropy_ctx],
                    dc_sign_ctx: 0, entropy_ctx: u_entropy_ctx, u_has_coeffs: false,
                }, Some(cdf));

                let v_entropy_ctx = compute_entropy_ctx(above_v, left_v);
                let v_skip_icdf = if block.u_eob > 0 {
                    &TXB_SKIP_V_U_NONZERO_CTX_ICDF[v_entropy_ctx]
                } else {
                    &TXB_SKIP_V_CTX_ICDF[v_entropy_ctx]
                };
                let v_coeffs = PlaneCoeffs { levels: &block.v_levels, eob: block.v_eob };
                encode_plane_full(ec, &v_coeffs, &PlaneEncodeCtx {
                    plane: 2, txs_ctx_idx: uv_txs_ctx, bwl: uv_bwl, height: uv_h, num_coeffs: uv_nc,
                    txb_skip_icdf: v_skip_icdf,
                    dc_sign_ctx: 0, entropy_ctx: v_entropy_ctx, u_has_coeffs: block.u_eob > 0,
                }, Some(cdf));
            } else {
                if bsize > 8 {
                    let ctx = partition_split_ctx(above_part, left_part, bsize);
                    ec.encode_symbol(0, &DO_SPLIT_PART_ICDF[0][ctx.min(63)]);
                }
                ec.encode_symbol(0, &Y_MODE_SET_ICDF);
                ec.encode_symbol(block.mode as usize, &Y_MODE_IDX_ICDF);
                ec.encode_symbol(block.uv_mode as usize, &UV_MODE_ICDF);

                let y_dc_sign_ctx = compute_dc_sign_ctx(above_y, left_y);
                let coeffs = PlaneCoeffs { levels: &block.levels, eob: block.eob };
                encode_plane_full(ec, &coeffs, &PlaneEncodeCtx {
                    plane: 0, txs_ctx_idx: txs_ctx, bwl, height: h, num_coeffs: nc,
                    txb_skip_icdf: &TXB_SKIP_Y_ICDF[txs_ctx][0],
                    dc_sign_ctx: y_dc_sign_ctx, entropy_ctx: 0, u_has_coeffs: false,
                }, None);

                let u_entropy_ctx = compute_entropy_ctx(above_u, left_u);
                let u_coeffs = PlaneCoeffs { levels: &block.u_levels, eob: block.u_eob };
                encode_plane_full(ec, &u_coeffs, &PlaneEncodeCtx {
                    plane: 1, txs_ctx_idx: uv_txs_ctx, bwl: uv_bwl, height: uv_h, num_coeffs: uv_nc,
                    txb_skip_icdf: &TXB_SKIP_U_CTX_ICDF[u_entropy_ctx],
                    dc_sign_ctx: 0, entropy_ctx: u_entropy_ctx, u_has_coeffs: false,
                }, None);

                let v_entropy_ctx = compute_entropy_ctx(above_v, left_v);
                let v_skip_icdf = if block.u_eob > 0 {
                    &TXB_SKIP_V_U_NONZERO_CTX_ICDF[v_entropy_ctx]
                } else {
                    &TXB_SKIP_V_CTX_ICDF[v_entropy_ctx]
                };
                let v_coeffs = PlaneCoeffs { levels: &block.v_levels, eob: block.v_eob };
                encode_plane_full(ec, &v_coeffs, &PlaneEncodeCtx {
                    plane: 2, txs_ctx_idx: uv_txs_ctx, bwl: uv_bwl, height: uv_h, num_coeffs: uv_nc,
                    txb_skip_icdf: v_skip_icdf,
                    dc_sign_ctx: 0, entropy_ctx: v_entropy_ctx, u_has_coeffs: block.u_eob > 0,
                }, None);
            }
        }
        PartitionNode::Split(children) => {
            let ctx = partition_split_ctx(above_part, left_part, bsize);
            let sq_ctx = sq_split_ctx(above_part, left_part, bsize);
            let half = bsize / 2;
            let mi_half = half / 4;
            let empty = vec![0u8; mi_half.max(1)];

            if let Some(cdf) = cdf_ctx {
                ec.encode_symbol_mut(1, &mut cdf.do_split_part[0][ctx.min(63)]);
                ec.encode_symbol_mut(1, &mut cdf.do_square_split[0][sq_ctx.min(7)]);
                for (i, child) in children.iter().enumerate() {
                    let child_above = if i < 2 { above_part } else { &empty };
                    let child_left = if i % 2 == 0 { left_part } else { &empty };
                    encode_partition_and_coeffs(ec, child, blocks, half, child_above, child_left, above_y, left_y, above_u, left_u, above_v, left_v, Some(cdf));
                }
            } else {
                ec.encode_symbol(1, &DO_SPLIT_PART_ICDF[0][ctx.min(63)]);
                ec.encode_symbol(1, &DO_SQUARE_SPLIT_ICDF[0][sq_ctx.min(7)]);
                for (i, child) in children.iter().enumerate() {
                    let child_above = if i < 2 { above_part } else { &empty };
                    let child_left = if i % 2 == 0 { left_part } else { &empty };
                    encode_partition_and_coeffs(ec, child, blocks, half, child_above, child_left, above_y, left_y, above_u, left_u, above_v, left_v, None);
                }
            }
        }
    }
}

fn collect_leaves(node: &PartitionNode, out: &mut Vec<usize>) {
    match node {
        PartitionNode::Leaf(idx) => out.push(*idx),
        PartitionNode::Split(children) => {
            for child in children { collect_leaves(child, out); }
        }
    }
}

fn compute_cul_level_vec(levels: &[i32], eob: u16) -> u8 {
    if eob == 0 { return 0; }
    let sum: usize = levels[..eob as usize].iter().map(|&l| l.unsigned_abs() as usize).sum();
    sum.min(COEFF_CONTEXT_MASK as usize) as u8
}

pub fn encode_tile_data_partitioned(
    sb_cols: usize,
    sb_rows: usize,
    sbs: &[EncodedSuperblock],
    allow_update_cdf: bool,
) -> Vec<u8> {
    let mut ec = DaalaCoder::new();
    let mut cdf_ctx = if allow_update_cdf { Some(FrameCdfContext::new()) } else { None };

    let mi_cols = sb_cols * 16;
    let mut above_part = vec![0u8; mi_cols];
    let mut above_y = vec![0u8; sb_cols * 16];
    let mut above_u = vec![0u8; sb_cols * 8];
    let mut above_v = vec![0u8; sb_cols * 8];

    for sb_row in 0..sb_rows {
        let mut left_part = vec![0u8; 16];
        let mut left_y = [0u8; 16];
        let mut left_u = [0u8; 8];
        let mut left_v = [0u8; 8];

        for sb_col in 0..sb_cols {
            let sb = &sbs[sb_row * sb_cols + sb_col];
            let sb_x = sb_col * 64;
            let sb_y_pos = sb_row * 64;

            let tree = build_partition_tree(&sb.y_blocks, sb_x, sb_y_pos);
            let above_slice = &above_part[sb_col * 16..(sb_col + 1) * 16];
            let y_above = &above_y[sb_col * 16..(sb_col + 1) * 16];

            encode_partition_and_coeffs(
                &mut ec, &tree, &sb.y_blocks, 64,
                above_slice, &left_part,
                y_above, &left_y,
                &above_u[sb_col * 8..(sb_col + 1) * 8], &left_u,
                &above_v[sb_col * 8..(sb_col + 1) * 8], &left_v,
                cdf_ctx.as_mut(),
            );

            let y_cul = {
                let total: u32 = sb.y_blocks.iter().map(|b| compute_cul_level_vec(&b.levels, b.eob) as u32).sum();
                (total / sb.y_blocks.len() as u32) as u8
            };
            for v in &mut above_y[sb_col * 16..(sb_col + 1) * 16] { *v = y_cul; }
            left_y.fill(y_cul);

            let part_val = if sb.y_blocks.len() == 1 { 0u8 } else {
                let min_bsize = sb.y_blocks.iter().map(|b| b.bsize).min().unwrap_or(64);
                match min_bsize { 32 => 1, 16 => 3, 8 => 7, _ => 0 }
            };
            for v in &mut above_part[sb_col * 16..(sb_col + 1) * 16] { *v = part_val; }
            left_part.fill(part_val);

            let u_cul = {
                let total: u32 = sb.y_blocks.iter().map(|b| compute_cul_level_vec(&b.u_levels, b.u_eob) as u32).sum();
                (total / sb.y_blocks.len() as u32) as u8
            };
            for v in &mut above_u[sb_col * 8..(sb_col + 1) * 8] { *v = u_cul; }
            left_u.fill(u_cul);

            let v_cul = {
                let total: u32 = sb.y_blocks.iter().map(|b| compute_cul_level_vec(&b.v_levels, b.v_eob) as u32).sum();
                (total / sb.y_blocks.len() as u32) as u8
            };
            for v in &mut above_v[sb_col * 8..(sb_col + 1) * 8] { *v = v_cul; }
            left_v.fill(v_cul);
        }
    }

    ec.done()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{EncodedBlock, EncodedSuperblock, IntraMode};

    #[test]
    fn partitioned_matches_legacy_for_none() {
        let mut sb_old = SbCoeffs {
            y_levels: [0i32; 1024],
            y_eob: 0,
            u_levels: [0i32; 1024],
            u_eob: 0,
            v_levels: [0i32; 1024],
            v_eob: 0,
            y_mode: 0,
            uv_mode: 0,
        };
        sb_old.y_levels[0] = 100;
        sb_old.y_eob = 1;

        let old_data = encode_tile_data_multi_sb(1, 1, &[sb_old], false);

        let sb_new = EncodedSuperblock {
            y_blocks: vec![EncodedBlock {
                x: 0, y: 0, bsize: 64,
                mode: IntraMode::DcPred,
                levels: {
                    let mut v = vec![0i32; 1024];
                    v[0] = 100;
                    v
                },
                eob: 1,
                log_scale: 2,
                uv_mode: IntraMode::DcPred,
                u_levels: vec![0i32; 1024],
                u_eob: 0,
                v_levels: vec![0i32; 1024],
                v_eob: 0,
            }],
            u_levels: [0i32; 1024],
            u_eob: 0,
            v_levels: [0i32; 1024],
            v_eob: 0,
            uv_mode: IntraMode::DcPred,
        };

        let new_data = encode_tile_data_partitioned(1, 1, &[sb_new], false);

        assert_eq!(
            old_data, new_data,
            "PART_NONE should match legacy\nold len={} new len={}\nold: {:02x?}\nnew: {:02x?}",
            old_data.len(), new_data.len(),
            &old_data[..old_data.len().min(20)],
            &new_data[..new_data.len().min(20)],
        );
    }

    #[test]
    fn partitioned_matches_legacy_with_uv() {
        let mut sb_old = SbCoeffs {
            y_levels: [0i32; 1024],
            y_eob: 0,
            u_levels: [0i32; 1024],
            u_eob: 0,
            v_levels: [0i32; 1024],
            v_eob: 0,
            y_mode: 0,
            uv_mode: 0,
        };
        sb_old.y_levels[0] = 100;
        sb_old.y_eob = 1;
        sb_old.u_levels[0] = 50;
        sb_old.u_eob = 1;
        sb_old.v_levels[0] = -30;
        sb_old.v_eob = 1;

        let old_data = encode_tile_data_multi_sb(1, 1, &[sb_old], false);

        let sb_new = EncodedSuperblock {
            y_blocks: vec![EncodedBlock {
                x: 0, y: 0, bsize: 64,
                mode: IntraMode::DcPred,
                levels: {
                    let mut v = vec![0i32; 1024];
                    v[0] = 100;
                    v
                },
                eob: 1,
                log_scale: 2,
                uv_mode: IntraMode::DcPred,
                u_levels: {
                    let mut v = vec![0i32; 1024];
                    v[0] = 50;
                    v
                },
                u_eob: 1,
                v_levels: {
                    let mut v = vec![0i32; 1024];
                    v[0] = -30;
                    v
                },
                v_eob: 1,
            }],
            u_levels: { let mut l = [0i32; 1024]; l[0] = 50; l },
            u_eob: 1,
            v_levels: { let mut l = [0i32; 1024]; l[0] = -30; l },
            v_eob: 1,
            uv_mode: IntraMode::DcPred,
        };

        let new_data = encode_tile_data_partitioned(1, 1, &[sb_new], false);

        assert_eq!(
            old_data, new_data,
            "PART_NONE with UV should match legacy\nold len={} new len={}\nold: {:02x?}\nnew: {:02x?}",
            old_data.len(), new_data.len(),
            &old_data[..old_data.len().min(20)],
            &new_data[..new_data.len().min(20)],
        );
    }

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
        let data =
            encode_tile_data_with_coeffs(&levels, 3, &[0i32; 1024], 0, &[0i32; 1024], 0, 0, 0);
        assert!(!data.is_empty());
    }

    #[test]
    fn tile_data_eob_1_produces_data() {
        let mut levels = [0i32; 1024];
        levels[0] = 50;
        let data =
            encode_tile_data_with_coeffs(&levels, 1, &[0i32; 1024], 0, &[0i32; 1024], 0, 0, 0);
        assert!(!data.is_empty());
    }

    #[test]
    fn encode_eob_position_1() {
        let mut ec = DaalaCoder::new();
        encode_eob(&mut ec, 1, 0, None);
        let data = ec.done();
        assert!(!data.is_empty());
    }

    #[test]
    fn encode_eob_various_positions() {
        for eob in [1, 2, 3, 5, 9, 17, 33, 100] {
            let mut ec = DaalaCoder::new();
            encode_eob(&mut ec, eob, 0, None);
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
            y_mode: 0,
            uv_mode: 0,
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
            0,
            0,
        );
        let multi = encode_tile_data_multi_sb(1, 1, &[sb], false);
        assert_eq!(
            single, multi,
            "1x1 multi-SB should match single-SB encoding"
        );
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
                y_mode: 0,
                uv_mode: 0,
            })
            .collect();
        let data = encode_tile_data_multi_sb(2, 2, &sbs, false);
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
                    y_mode: 0,
                    uv_mode: 0,
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
        let data = encode_tile_data_multi_sb(2, 2, &sbs, false);
        assert!(!data.is_empty());
    }
}
