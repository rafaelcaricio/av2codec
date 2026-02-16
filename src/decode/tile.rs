use crate::cdf_context::FrameCdfContext;
use crate::cdf_tables::*;
use crate::decode::entropy::DaalaDecoder;
use crate::scan::DEFAULT_SCAN_32X32;

const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];
const Y_MODE_SET_ICDF: [u16; 4] = [3905, 1746, 1044, 0];
const Y_MODE_IDX_ICDF: [u16; 8] = [17593, 12693, 11040, 8670, 6363, 5113, 3908, 0];
const UV_MODE_ICDF: [u16; 8] = [23405, 11811, 9903, 8015, 6357, 4785, 2340, 0];
const TXB_SKIP_Y_ICDF: [u16; 2] = [29968, 0];

const ADAPTIVE_TABLE: [usize; 5] = [4, 8, 16, 32, 64];

const COEFF_CONTEXT_BITS: u32 = 3;
const COEFF_CONTEXT_MASK: u8 = (1 << COEFF_CONTEXT_BITS) - 1;

pub struct DecodedSbCoeffs {
    pub y_levels: [i32; 1024],
    pub y_eob: u16,
    pub u_levels: [i32; 1024],
    pub u_eob: u16,
    pub v_levels: [i32; 1024],
    pub v_eob: u16,
    pub y_mode: u8,
    pub uv_mode: u8,
}

fn get_adaptive_param(ctx: usize) -> u32 {
    let mut m = 0;
    while m < ADAPTIVE_TABLE.len() && ctx >= ADAPTIVE_TABLE[m] {
        m += 1;
    }
    (m + 1) as u32
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

fn decode_exp_golomb(ec: &mut DaalaDecoder, mut k: u32) -> u32 {
    let mut total = 0u32;
    loop {
        if ec.decode_literal(1) == 1 {
            break;
        }
        total += 1 << k;
        k += 1;
    }
    if k > 0 {
        total += ec.decode_literal(k);
    }
    total
}

fn decode_truncated_rice(ec: &mut DaalaDecoder, m: u32, k: u32, cmax: u32) -> u32 {
    let mut q = 0u32;
    while q < cmax {
        if ec.decode_literal(1) == 1 {
            break;
        }
        q += 1;
    }
    if q >= cmax {
        let eg = decode_exp_golomb(ec, k);
        (cmax << m) + eg
    } else {
        let r = if m > 0 { ec.decode_literal(m) } else { 0 };
        (q << m) | r
    }
}

fn decode_adaptive_hr(ec: &mut DaalaDecoder, hr_ctx: usize) -> usize {
    let m = get_adaptive_param(hr_ctx);
    let k = m + 1;
    let cmax = (m + 4).min(6);
    decode_truncated_rice(ec, m, k, cmax) as usize
}

fn decode_eob(
    ec: &mut DaalaDecoder,
    pl_ctx: usize,
    mut cdf_ctx: Option<&mut FrameCdfContext>,
) -> u16 {
    let raw_token = if let Some(ref mut ctx) = cdf_ctx {
        ec.decode_symbol_mut(&mut ctx.eob_multi_1024[pl_ctx])
    } else {
        ec.decode_symbol(&EOB_MULTI_1024_ICDF[pl_ctx])
    };

    let eob_token = if raw_token >= EOB_PT_INDEX_COUNT - 1 {
        let extra_token = ec.decode_literal(2) as usize;
        (EOB_PT_INDEX_COUNT - 1) + extra_token
    } else {
        raw_token
    };

    let eob_pt = eob_token + 1;
    let eob_offset_bits = EOB_OFFSET_BITS[eob_pt] as u32;
    let base = EOB_GROUP_START[eob_pt];

    if eob_offset_bits > 0 {
        let msb = if let Some(ref mut ctx) = cdf_ctx {
            ec.decode_symbol_mut(&mut ctx.eob_extra) as u16
        } else {
            ec.decode_symbol(&EOB_EXTRA_ICDF) as u16
        };
        let rest = if eob_offset_bits > 1 {
            ec.decode_literal(eob_offset_bits - 1) as u16
        } else {
            0
        };
        let extra = (msb << (eob_offset_bits - 1)) | rest;
        base + extra
    } else {
        base
    }
}

fn decode_low_range(
    ec: &mut DaalaDecoder,
    base_levels: usize,
    static_cdf: &[u16],
    mutable_cdf: Option<&mut Vec<u16>>,
) -> usize {
    let br_sym = if let Some(cdf) = mutable_cdf {
        ec.decode_symbol_mut(cdf)
    } else {
        ec.decode_symbol(static_cdf)
    };
    base_levels + 1 + br_sym
}

struct PlaneDecodeCtx<'a> {
    plane: usize,
    txs_ctx_idx: usize,
    bwl: u32,
    height: usize,
    txb_skip_icdf: &'a [u16],
    dc_sign_ctx: usize,
    entropy_ctx: usize,
    u_has_coeffs: bool,
}

fn decode_plane_full(
    ec: &mut DaalaDecoder,
    ctx: &PlaneDecodeCtx,
    mut cdf_ctx: Option<&mut FrameCdfContext>,
) -> ([i32; 1024], u16) {
    let plane = ctx.plane;
    let txs_ctx_idx = ctx.txs_ctx_idx;
    let bwl = ctx.bwl;
    let height = ctx.height;
    let dc_sign_ctx = ctx.dc_sign_ctx;

    let mut levels = [0i32; 1024];

    let skip = if let Some(ref mut cdf) = cdf_ctx {
        let txb_skip_cdf = match ctx.plane {
            0 => &mut cdf.txb_skip_y,
            1 => &mut cdf.txb_skip_u[ctx.entropy_ctx],
            _ => if ctx.u_has_coeffs {
                &mut cdf.txb_skip_v_u_nz[ctx.entropy_ctx]
            } else {
                &mut cdf.txb_skip_v[ctx.entropy_ctx]
            },
        };
        ec.decode_symbol_mut(txb_skip_cdf)
    } else {
        ec.decode_symbol(ctx.txb_skip_icdf)
    };
    if skip == 1 {
        return (levels, 0);
    }

    let pl_ctx = if plane == 0 { 0 } else { 2 };
    let eob = decode_eob(ec, pl_ctx, cdf_ctx.as_deref_mut());

    let width = 1usize << bwl;
    let padded_stride = width + TX_PAD_HOR;
    let padded_height = height + 2;
    let mut levels_buf = vec![0u8; padded_stride * padded_height];

    let mut abs_levels = vec![0usize; eob as usize];

    let eob_scan_idx = (eob - 1) as usize;
    let eob_pos = DEFAULT_SCAN_32X32[eob_scan_idx] as usize;
    let eob_row = eob_pos >> bwl;
    let eob_col = eob_pos - (eob_row << bwl);
    let eob_lf = is_lf_2d(eob_row, eob_col, plane);

    let coeff_ctx = get_lower_levels_ctx_eob(bwl, height, eob_scan_idx);

    if plane == 0 {
        if eob_lf {
            let sym = if let Some(ref mut cdf) = cdf_ctx {
                ec.decode_symbol_mut(&mut cdf.coeff_base_lf_eob_y[txs_ctx_idx][coeff_ctx])
            } else {
                ec.decode_symbol(&COEFF_BASE_LF_EOB_Y_ICDF[txs_ctx_idx][coeff_ctx])
            };
            let base_value = sym + 1;
            if base_value > LF_NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_lf_eob_2d(eob_pos);
                abs_levels[eob_scan_idx] = decode_low_range(
                    ec,
                    LF_NUM_BASE_LEVELS,
                    &COEFF_BR_LF_Y_ICDF[br_ctx],
                    cdf_ctx.as_deref_mut().map(|c| &mut c.coeff_br_lf_y[br_ctx]),
                );
            } else {
                abs_levels[eob_scan_idx] = base_value;
            }
        } else {
            let sym = if let Some(ref mut cdf) = cdf_ctx {
                ec.decode_symbol_mut(&mut cdf.coeff_base_eob_y[txs_ctx_idx][coeff_ctx])
            } else {
                ec.decode_symbol(&COEFF_BASE_EOB_Y_ICDF[txs_ctx_idx][coeff_ctx])
            };
            let base_value = sym + 1;
            if base_value > NUM_BASE_LEVELS {
                abs_levels[eob_scan_idx] = decode_low_range(
                    ec,
                    NUM_BASE_LEVELS,
                    &COEFF_BR_Y_ICDF[0],
                    cdf_ctx.as_deref_mut().map(|c| &mut c.coeff_br_y[0]),
                );
            } else {
                abs_levels[eob_scan_idx] = base_value;
            }
        }
    } else if eob_lf {
        let sym = if let Some(ref mut cdf) = cdf_ctx {
            ec.decode_symbol_mut(&mut cdf.coeff_base_lf_eob_uv[coeff_ctx])
        } else {
            ec.decode_symbol(&COEFF_BASE_LF_EOB_UV_ICDF[coeff_ctx])
        };
        abs_levels[eob_scan_idx] = sym + 1;
    } else {
        let sym = if let Some(ref mut cdf) = cdf_ctx {
            ec.decode_symbol_mut(&mut cdf.coeff_base_eob_uv[coeff_ctx])
        } else {
            ec.decode_symbol(&COEFF_BASE_EOB_UV_ICDF[coeff_ctx])
        };
        let base_value = sym + 1;
        if base_value > NUM_BASE_LEVELS {
            abs_levels[eob_scan_idx] = decode_low_range(
                ec,
                NUM_BASE_LEVELS,
                &COEFF_BR_UV_ICDF[0],
                cdf_ctx.as_deref_mut().map(|c| &mut c.coeff_br_uv[0]),
            );
        } else {
            abs_levels[eob_scan_idx] = base_value;
        }
    }

    levels_buf[get_padded_idx(eob_pos, bwl)] =
        stored_level(abs_levels[eob_scan_idx], eob_lf, plane);

    if eob > 1 {
        for c in (1..eob_scan_idx).rev() {
            let pos = DEFAULT_SCAN_32X32[c] as usize;
            let row = pos >> bwl;
            let col = pos - (row << bwl);
            let lf = is_lf_2d(row, col, plane);

            if plane == 0 {
                if lf {
                    let lctx = get_lower_levels_ctx_lf_2d(&levels_buf, pos, bwl);
                    let sym = if let Some(ref mut cdf) = cdf_ctx {
                        ec.decode_symbol_mut(&mut cdf.coeff_base_lf_y[txs_ctx_idx][lctx])
                    } else {
                        ec.decode_symbol(&COEFF_BASE_LF_Y_ICDF[txs_ctx_idx][lctx])
                    };
                    if sym > LF_NUM_BASE_LEVELS {
                        let br_ctx = get_br_lf_ctx_2d(&levels_buf, pos, bwl);
                        abs_levels[c] = decode_low_range(
                            ec,
                            LF_NUM_BASE_LEVELS,
                            &COEFF_BR_LF_Y_ICDF[br_ctx],
                            cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_lf_y[br_ctx]),
                        );
                    } else {
                        abs_levels[c] = sym;
                    }
                } else {
                    let lctx = get_lower_levels_ctx_2d(&levels_buf, pos, bwl, plane);
                    let sym = if let Some(ref mut cdf) = cdf_ctx {
                        ec.decode_symbol_mut(&mut cdf.coeff_base_y[txs_ctx_idx][lctx])
                    } else {
                        ec.decode_symbol(&COEFF_BASE_Y_ICDF[txs_ctx_idx][lctx])
                    };
                    if sym > NUM_BASE_LEVELS {
                        let br_ctx = get_br_ctx_2d(&levels_buf, pos, bwl);
                        abs_levels[c] = decode_low_range(
                            ec,
                            NUM_BASE_LEVELS,
                            &COEFF_BR_Y_ICDF[br_ctx],
                            cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_y[br_ctx]),
                        );
                    } else {
                        abs_levels[c] = sym;
                    }
                }
            } else if lf {
                let lctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, pos, bwl, plane);
                let sym = if let Some(ref mut cdf) = cdf_ctx {
                    ec.decode_symbol_mut(&mut cdf.coeff_base_lf_uv[lctx])
                } else {
                    ec.decode_symbol(&COEFF_BASE_LF_UV_ICDF[lctx])
                };
                abs_levels[c] = sym;
            } else {
                let lctx = get_lower_levels_ctx_2d_chroma(&levels_buf, pos, bwl, plane);
                let sym = if let Some(ref mut cdf) = cdf_ctx {
                    ec.decode_symbol_mut(&mut cdf.coeff_base_uv[lctx])
                } else {
                    ec.decode_symbol(&COEFF_BASE_UV_ICDF[lctx])
                };
                if sym > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d_chroma(&levels_buf, pos, bwl);
                    abs_levels[c] = decode_low_range(
                        ec,
                        NUM_BASE_LEVELS,
                        &COEFF_BR_UV_ICDF[br_ctx],
                        cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_uv[br_ctx]),
                    );
                } else {
                    abs_levels[c] = sym;
                }
            }

            levels_buf[get_padded_idx(pos, bwl)] = stored_level(abs_levels[c], lf, plane);
        }

        let dc_pos = DEFAULT_SCAN_32X32[0] as usize;
        let dc_row = dc_pos >> bwl;
        let dc_col = dc_pos - (dc_row << bwl);
        let dc_lf = is_lf_2d(dc_row, dc_col, plane);

        if plane == 0 {
            if dc_lf {
                let lctx = get_lower_levels_ctx_lf_2d(&levels_buf, dc_pos, bwl);
                let sym = if let Some(ref mut cdf) = cdf_ctx {
                    ec.decode_symbol_mut(&mut cdf.coeff_base_lf_y[txs_ctx_idx][lctx])
                } else {
                    ec.decode_symbol(&COEFF_BASE_LF_Y_ICDF[txs_ctx_idx][lctx])
                };
                if sym > LF_NUM_BASE_LEVELS {
                    let br_ctx = get_br_lf_ctx_dc_2d(&levels_buf, dc_pos, bwl);
                    abs_levels[0] = decode_low_range(
                        ec,
                        LF_NUM_BASE_LEVELS,
                        &COEFF_BR_LF_Y_ICDF[br_ctx],
                        cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_lf_y[br_ctx]),
                    );
                } else {
                    abs_levels[0] = sym;
                }
            } else {
                let lctx = get_lower_levels_ctx_2d(&levels_buf, dc_pos, bwl, plane);
                let sym = if let Some(ref mut cdf) = cdf_ctx {
                    ec.decode_symbol_mut(&mut cdf.coeff_base_y[txs_ctx_idx][lctx])
                } else {
                    ec.decode_symbol(&COEFF_BASE_Y_ICDF[txs_ctx_idx][lctx])
                };
                if sym > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d(&levels_buf, dc_pos, bwl);
                    abs_levels[0] = decode_low_range(
                        ec,
                        NUM_BASE_LEVELS,
                        &COEFF_BR_Y_ICDF[br_ctx],
                        cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_y[br_ctx]),
                    );
                } else {
                    abs_levels[0] = sym;
                }
            }
        } else if dc_lf {
            let lctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            let sym = if let Some(ref mut cdf) = cdf_ctx {
                ec.decode_symbol_mut(&mut cdf.coeff_base_lf_uv[lctx])
            } else {
                ec.decode_symbol(&COEFF_BASE_LF_UV_ICDF[lctx])
            };
            abs_levels[0] = sym;
        } else {
            let lctx = get_lower_levels_ctx_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            let sym = if let Some(ref mut cdf) = cdf_ctx {
                ec.decode_symbol_mut(&mut cdf.coeff_base_uv[lctx])
            } else {
                ec.decode_symbol(&COEFF_BASE_UV_ICDF[lctx])
            };
            if sym > NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_2d_chroma(&levels_buf, dc_pos, bwl);
                abs_levels[0] = decode_low_range(
                    ec,
                    NUM_BASE_LEVELS,
                    &COEFF_BR_UV_ICDF[br_ctx],
                    cdf_ctx.as_deref_mut().map(|cd| &mut cd.coeff_br_uv[br_ctx]),
                );
            } else {
                abs_levels[0] = sym;
            }
        }

        levels_buf[get_padded_idx(dc_pos, bwl)] = stored_level(abs_levels[0], dc_lf, plane);
    }

    let mut hr_avg = 0usize;
    for c in (0..eob as usize).rev() {
        let pos = DEFAULT_SCAN_32X32[c] as usize;
        let abs_level = abs_levels[c];

        if abs_level == 0 {
            continue;
        }

        let row = pos >> bwl;
        let col = pos - (row << bwl);
        let is_dc_2d = c == 0;

        let sign = if is_dc_2d {
            if plane == 0 {
                if let Some(ref mut cdf) = cdf_ctx {
                    ec.decode_symbol_mut(&mut cdf.dc_sign_y[dc_sign_ctx])
                } else {
                    ec.decode_symbol(&DC_SIGN_Y_ICDF[dc_sign_ctx])
                }
            } else {
                ec.decode_literal(1) as usize
            }
        } else {
            ec.decode_literal(1) as usize
        };

        let lf = is_lf_2d(row, col, plane);
        let max_br = if lf {
            if plane == 0 {
                LF_MAX_BASE_BR_RANGE
            } else {
                LF_NUM_BASE_LEVELS + 1
            }
        } else {
            MAX_BASE_BR_RANGE
        };

        let final_level = if abs_level >= max_br {
            let hr = decode_adaptive_hr(ec, hr_avg);
            hr_avg = (hr_avg + hr) >> 1;
            abs_level + hr
        } else {
            abs_level
        };

        levels[c] = if sign == 1 {
            -(final_level as i32)
        } else {
            final_level as i32
        };
    }

    (levels, eob)
}

pub fn decode_tile_data_multi_sb(
    data: &[u8],
    sb_cols: usize,
    sb_rows: usize,
    allow_update_cdf: bool,
) -> Vec<DecodedSbCoeffs> {
    let mut ec = DaalaDecoder::new(data);
    let mut result = Vec::with_capacity(sb_cols * sb_rows);

    let mut cdf_ctx = if allow_update_cdf {
        Some(FrameCdfContext::new())
    } else {
        None
    };

    let mut above_y = vec![0u8; sb_cols * 16];
    let mut above_u = vec![0u8; sb_cols * 8];
    let mut above_v = vec![0u8; sb_cols * 8];

    for _sb_row in 0..sb_rows {
        let mut left_y = [0u8; 16];
        let mut left_u = [0u8; 8];
        let mut left_v = [0u8; 8];

        for sb_col in 0..sb_cols {
            let _partition = if let Some(ref mut ctx) = cdf_ctx {
                ec.decode_symbol_mut(&mut ctx.do_split)
            } else {
                ec.decode_symbol(&DO_SPLIT_ICDF)
            };
            let _mode_set = if let Some(ref mut ctx) = cdf_ctx {
                ec.decode_symbol_mut(&mut ctx.y_mode_set)
            } else {
                ec.decode_symbol(&Y_MODE_SET_ICDF)
            };
            let y_mode = if let Some(ref mut ctx) = cdf_ctx {
                ec.decode_symbol_mut(&mut ctx.y_mode_idx) as u8
            } else {
                ec.decode_symbol(&Y_MODE_IDX_ICDF) as u8
            };
            let uv_mode = if let Some(ref mut ctx) = cdf_ctx {
                ec.decode_symbol_mut(&mut ctx.uv_mode) as u8
            } else {
                ec.decode_symbol(&UV_MODE_ICDF) as u8
            };

            let y_above = &above_y[sb_col * 16..(sb_col + 1) * 16];
            let y_dc_sign_ctx = compute_dc_sign_ctx(y_above, &left_y);
            let (y_levels, y_eob) = decode_plane_full(
                &mut ec,
                &PlaneDecodeCtx {
                    plane: 0,
                    txs_ctx_idx: 1,
                    bwl: 5,
                    height: 32,
                    txb_skip_icdf: &TXB_SKIP_Y_ICDF,
                    dc_sign_ctx: y_dc_sign_ctx,
                    entropy_ctx: 0,
                    u_has_coeffs: false,
                },
                cdf_ctx.as_mut(),
            );

            let y_cul = compute_cul_level(&y_levels, y_eob);
            for v in &mut above_y[sb_col * 16..(sb_col + 1) * 16] {
                *v = y_cul;
            }
            left_y.fill(y_cul);

            let u_above = &above_u[sb_col * 8..(sb_col + 1) * 8];
            let u_entropy_ctx = compute_entropy_ctx(u_above, &left_u);
            let (u_levels, u_eob) = decode_plane_full(
                &mut ec,
                &PlaneDecodeCtx {
                    plane: 1,
                    txs_ctx_idx: 0,
                    bwl: 5,
                    height: 32,
                    txb_skip_icdf: &TXB_SKIP_U_CTX_ICDF[u_entropy_ctx],
                    dc_sign_ctx: 0,
                    entropy_ctx: u_entropy_ctx,
                    u_has_coeffs: false,
                },
                cdf_ctx.as_mut(),
            );

            let u_cul = compute_cul_level(&u_levels, u_eob);
            for v in &mut above_u[sb_col * 8..(sb_col + 1) * 8] {
                *v = u_cul;
            }
            left_u.fill(u_cul);

            let v_above = &above_v[sb_col * 8..(sb_col + 1) * 8];
            let v_entropy_ctx = compute_entropy_ctx(v_above, &left_v);
            let v_skip_icdf = if u_eob > 0 {
                &TXB_SKIP_V_U_NONZERO_CTX_ICDF[v_entropy_ctx]
            } else {
                &TXB_SKIP_V_CTX_ICDF[v_entropy_ctx]
            };
            let (v_levels, v_eob) = decode_plane_full(
                &mut ec,
                &PlaneDecodeCtx {
                    plane: 2,
                    txs_ctx_idx: 0,
                    bwl: 5,
                    height: 32,
                    txb_skip_icdf: v_skip_icdf,
                    dc_sign_ctx: 0,
                    entropy_ctx: v_entropy_ctx,
                    u_has_coeffs: u_eob > 0,
                },
                cdf_ctx.as_mut(),
            );

            let v_cul = compute_cul_level(&v_levels, v_eob);
            for v in &mut above_v[sb_col * 8..(sb_col + 1) * 8] {
                *v = v_cul;
            }
            left_v.fill(v_cul);

            result.push(DecodedSbCoeffs {
                y_levels,
                y_eob,
                u_levels,
                u_eob,
                v_levels,
                v_eob,
                y_mode,
                uv_mode,
            });
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile;

    #[test]
    fn roundtrip_all_zero() {
        let data = tile::encode_tile_data();
        let decoded = decode_tile_data_multi_sb(&data, 1, 1, false);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].y_eob, 0);
        assert_eq!(decoded[0].u_eob, 0);
        assert_eq!(decoded[0].v_eob, 0);
        assert!(decoded[0].y_levels.iter().all(|&l| l == 0));
        assert!(decoded[0].u_levels.iter().all(|&l| l == 0));
        assert!(decoded[0].v_levels.iter().all(|&l| l == 0));
    }

    #[test]
    fn roundtrip_dc_only() {
        let y_dc = 50i32;
        let u_dc = -10i32;
        let v_dc = 5i32;
        let data = tile::encode_tile_data_with_dc(y_dc, u_dc, v_dc);
        let decoded = decode_tile_data_multi_sb(&data, 1, 1, false);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].y_eob, 1);
        assert_eq!(decoded[0].u_eob, 1);
        assert_eq!(decoded[0].v_eob, 1);
        assert_eq!(decoded[0].y_levels[0], y_dc);
        assert_eq!(decoded[0].u_levels[0], u_dc);
        assert_eq!(decoded[0].v_levels[0], v_dc);
    }

    #[test]
    fn roundtrip_ac_coefficients() {
        let mut y_levels = [0i32; 1024];
        y_levels[0] = 100;
        y_levels[1] = -20;
        y_levels[2] = 10;
        let data = tile::encode_tile_data_with_coeffs(
            &y_levels,
            3,
            &[0i32; 1024],
            0,
            &[0i32; 1024],
            0,
            0,
            0,
        );
        let decoded = decode_tile_data_multi_sb(&data, 1, 1, false);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].y_eob, 3);
        assert_eq!(decoded[0].y_levels[0], 100);
        assert_eq!(decoded[0].y_levels[1], -20);
        assert_eq!(decoded[0].y_levels[2], 10);
        assert_eq!(decoded[0].u_eob, 0);
        assert_eq!(decoded[0].v_eob, 0);
    }

    #[test]
    fn roundtrip_multi_sb_2x2() {
        let sbs: Vec<tile::SbCoeffs> = (0..4)
            .map(|i| {
                let mut sb = tile::SbCoeffs {
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
        let data = tile::encode_tile_data_multi_sb(2, 2, &sbs, false);
        let decoded = decode_tile_data_multi_sb(&data, 2, 2, false);
        assert_eq!(decoded.len(), 4);
        for (i, sb) in decoded.iter().enumerate() {
            assert_eq!(sb.y_eob, 1);
            assert_eq!(sb.y_levels[0], ((i as i32) + 1) * 10);
            assert_eq!(sb.u_eob, 1);
            assert_eq!(sb.u_levels[0], ((i as i32) + 1) * 5);
            assert_eq!(sb.v_eob, 1);
            assert_eq!(sb.v_levels[0], ((i as i32) + 1) * 3);
        }
    }

    #[test]
    fn roundtrip_large_dc() {
        let y_dc = 200i32;
        let u_dc = -150i32;
        let v_dc = 100i32;
        let data = tile::encode_tile_data_with_dc(y_dc, u_dc, v_dc);
        let decoded = decode_tile_data_multi_sb(&data, 1, 1, false);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].y_levels[0], y_dc);
        assert_eq!(decoded[0].u_levels[0], u_dc);
        assert_eq!(decoded[0].v_levels[0], v_dc);
    }

    #[test]
    fn roundtrip_uv_nonzero() {
        let mut u_levels = [0i32; 1024];
        u_levels[0] = 30;
        let mut v_levels = [0i32; 1024];
        v_levels[0] = -20;
        let data = tile::encode_tile_data_with_coeffs(
            &[0i32; 1024],
            0,
            &u_levels,
            1,
            &v_levels,
            1,
            0,
            0,
        );
        let decoded = decode_tile_data_multi_sb(&data, 1, 1, false);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].y_eob, 0);
        assert_eq!(decoded[0].u_eob, 1);
        assert_eq!(decoded[0].u_levels[0], 30);
        assert_eq!(decoded[0].v_eob, 1);
        assert_eq!(decoded[0].v_levels[0], -20);
    }
}
