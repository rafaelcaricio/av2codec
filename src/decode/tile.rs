use std::collections::HashSet;
use crate::block::{PartitionType, TxSize, PARTITION_PLOFFSET, PARTITION_CONTEXTS, SQUARE_SPLIT_CONTEXTS};
use crate::cdf_context::FrameCdfContext;
use crate::cdf_tables::*;
use crate::decode::entropy::DaalaDecoder;
use crate::scan::{get_scan, DEFAULT_SCAN_32X32};

const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];
const FIRST_MODE_COUNT: usize = 13;
const SECOND_MODE_COUNT: usize = 16;
const NON_DIRECTIONAL_MODES_COUNT: usize = 5;
const CFL_SIGNS: usize = 3;

const ADAPTIVE_TABLE: [usize; 5] = [4, 8, 16, 32, 64];
const CDEF_UNIT_SIZE: usize = 64;
const CCSO_UNIT_SIZE: usize = 256;

#[derive(Clone, Copy, PartialEq)]
enum TreeType {
    Shared,
    Luma,
    Chroma,
}

struct TileState {
    cdef_read: HashSet<(usize, usize)>,
    ccso_read: [HashSet<(usize, usize)>; 3],
}

pub struct TileDecodeConfig {
    pub enable_cfl_intra: bool,
    pub enable_fsc: bool,
    pub enable_idtx_intra: bool,
    pub enable_mrls: bool,
    pub enable_ist: bool,
    pub enable_cctx: bool,
    pub tx_mode_select: bool,
    pub reduced_tx_set_used: u8,
    pub reduced_tx_part_set: bool,
    pub allow_intrabc: bool,
    pub allow_screen_content_tools: bool,
    pub enable_sdp: bool,
    pub cdef_frame_enable: bool,
    pub nb_cdef_strengths: u32,
    pub ccso_frame_enable: bool,
    pub ccso_enable: [bool; 3],
    pub sb_size: usize,
}

impl Default for TileDecodeConfig {
    fn default() -> Self {
        Self {
            enable_cfl_intra: false,
            enable_fsc: false,
            enable_idtx_intra: false,
            enable_mrls: false,
            enable_ist: false,
            enable_cctx: false,
            tx_mode_select: false,
            reduced_tx_set_used: 0,
            reduced_tx_part_set: false,
            allow_intrabc: false,
            allow_screen_content_tools: false,
            enable_sdp: false,
            cdef_frame_enable: false,
            nb_cdef_strengths: 0,
            ccso_frame_enable: false,
            ccso_enable: [false; 3],
            sb_size: 128,
        }
    }
}

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
            0 => &mut cdf.txb_skip_y[txs_ctx_idx][ctx.entropy_ctx],
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
                ec.decode_symbol_mut(&mut ctx.y_mode_idx[0]) as u8
            } else {
                ec.decode_symbol(&Y_MODE_IDX_ICDF[0]) as u8
            };
            let uv_mode = if let Some(ref mut ctx) = cdf_ctx {
                ec.decode_symbol_mut(&mut ctx.uv_mode[0]) as u8
            } else {
                ec.decode_symbol(&UV_MODE_ICDF[0]) as u8
            };

            let y_above = &above_y[sb_col * 16..(sb_col + 1) * 16];
            let y_dc_sign_ctx = compute_dc_sign_ctx(y_above, &left_y);
            let (y_levels, y_eob) = decode_plane_full(
                &mut ec,
                &PlaneDecodeCtx {
                    plane: 0,
                    txs_ctx_idx: 4,
                    bwl: 5,
                    height: 32,
                    txb_skip_icdf: &TXB_SKIP_Y_ICDF[4][0],
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
                    txs_ctx_idx: 3,
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
                    txs_ctx_idx: 3,
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

pub struct TuData {
    pub levels: Vec<i32>,
    pub eob: u16,
    pub tx_size: TxSize,
    pub row_offset: usize,
    pub col_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TxPartitionType {
    None,
    Split,
    Horz,
    Vert,
    Horz4,
    Vert4,
}

pub struct DecodedBlock {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub y_mode: u8,
    pub uv_mode: u8,
    pub y_tu: Vec<TuData>,
    pub u_tu: Vec<TuData>,
    pub v_tu: Vec<TuData>,
}

fn compute_eob_from_token(
    ec: &mut DaalaDecoder,
    eob_token: usize,
    cdf: &mut FrameCdfContext,
) -> u16 {
    let eob_pt = eob_token + 1;
    let bits = EOB_OFFSET_BITS[eob_pt] as u32;
    let base = EOB_GROUP_START[eob_pt];
    if bits > 0 {
        let msb = ec.decode_symbol_mut(&mut cdf.eob_extra) as u16;
        let rest = if bits > 1 {
            ec.decode_literal(bits - 1) as u16
        } else {
            0
        };
        base + (msb << (bits - 1)) | rest
    } else {
        base
    }
}

fn decode_eob_var(
    ec: &mut DaalaDecoder,
    tx_size: TxSize,
    pl_ctx: usize,
    cdf: &mut FrameCdfContext,
) -> u16 {
    let num_coeffs = tx_size.num_coeffs();
    let (raw, extra_bits) = match num_coeffs {
        16 => (ec.decode_symbol_mut(&mut cdf.eob_multi_16[pl_ctx]), 0u32),
        32 => (ec.decode_symbol_mut(&mut cdf.eob_multi_32[pl_ctx]), 0),
        64 => (ec.decode_symbol_mut(&mut cdf.eob_multi_64[pl_ctx]), 0),
        128 => (ec.decode_symbol_mut(&mut cdf.eob_multi_128[pl_ctx]), 0),
        256 => (ec.decode_symbol_mut(&mut cdf.eob_multi_256[pl_ctx]), 1),
        512 => (ec.decode_symbol_mut(&mut cdf.eob_multi_512[pl_ctx]), 2),
        1024 => (ec.decode_symbol_mut(&mut cdf.eob_multi_1024[pl_ctx]), 2),
        _ => unreachable!("unexpected num_coeffs: {num_coeffs}"),
    };

    let eob_token = if extra_bits > 0 && raw >= EOB_PT_INDEX_COUNT - 1 {
        let extra = ec.decode_literal(extra_bits) as usize;
        (EOB_PT_INDEX_COUNT - 1) + extra
    } else {
        raw
    };

    compute_eob_from_token(ec, eob_token, cdf)
}

fn read_tx_type(
    ec: &mut DaalaDecoder,
    tx_size: TxSize,
    y_mode: u8,
    fsc_mode: usize,
    reduced_tx_set_used: u8,
    cdf: &mut FrameCdfContext,
) -> usize {
    if fsc_mode != 0 {
        return IDTX;
    }

    if reduced_tx_set_used == 2 {
        return DCT_DCT;
    }

    let width = tx_size.width();
    let square_tx_size = tx_size.txs_ctx();

    let tx_size_sqr_up = tx_size.txs_ctx();

    if tx_size_sqr_up > 3 {
        if tx_size_sqr_up >= 3 && width >= 32 {
            return DCT_DCT;
        }
        let _is_long_side_dct = if tx_size_sqr_up == 3 {
            ec.decode_symbol_mut(&mut cdf.tx_ext_32_intra)
        } else {
            1
        };
        let short_side_idx = ec.decode_symbol_mut(&mut cdf.intra_ext_tx_short_side[square_tx_size.min(3)]);
        ec.trace_state(&format!("after_tx_type_large short_side={}", short_side_idx));
        return DCT_DCT;
    }

    if tx_size_sqr_up == 3 {
        if reduced_tx_set_used == 0 {
            return DCT_DCT;
        }
        let tx_type_idx = ec.decode_symbol_mut(&mut cdf.intra_ext_tx_reduced);
        ec.trace_state(&format!("after_tx_type_32x32_reduced={}", tx_type_idx));
        return if tx_type_idx == 0 { DCT_DCT } else { IDTX };
    }

    if reduced_tx_set_used == 1 {
        let tx_type_idx = ec.decode_symbol_mut(&mut cdf.intra_ext_tx_reduced);
        ec.trace_state(&format!("after_tx_type_reduced={}", tx_type_idx));
        return if tx_type_idx == 0 { DCT_DCT } else { ADST_ADST };
    }

    let size_info = AV2_SIZE_CLASS[square_tx_size.min(4)];
    let mode = y_mode as usize;
    let tx_type_idx = ec.decode_symbol_mut(&mut cdf.intra_ext_tx[square_tx_size.min(3)]);
    ec.trace_state(&format!("after_tx_type={} size={} mode={}", tx_type_idx, square_tx_size, mode));

    if mode < 13 && tx_type_idx < 7 {
        AV2_MD_IDX2TYPE[size_info][mode][tx_type_idx] as usize
    } else {
        DCT_DCT
    }
}

fn read_sec_tx_type(
    ec: &mut DaalaDecoder,
    tx_size: TxSize,
    primary_tx_type: usize,
    y_mode: u8,
    eob: usize,
    cdf: &mut FrameCdfContext,
) {
    if primary_tx_type != DCT_DCT && primary_tx_type != ADST_ADST {
        return;
    }

    if y_mode >= PAETH_PRED {
        return;
    }

    let width = tx_size.width();
    let height = tx_size.height();
    let square_tx_size = tx_size.txs_ctx();

    let (st_size_class, eob_limit) = if width == 4 || height == 4 {
        (0, IST_4X4_HEIGHT)
    } else if width == 8 && height == 8 && primary_tx_type == DCT_DCT {
        (1, IST_8X8_HEIGHT_RED)
    } else if width >= 8 && height >= 8 && primary_tx_type == DCT_DCT {
        (2, IST_8X8_HEIGHT)
    } else if width >= 8 && height >= 8 {
        (3, IST_ADST_NZ_CNT)
    } else {
        return;
    };

    let _ = st_size_class;

    if eob > eob_limit {
        return;
    }

    let stx_idx = square_tx_size.min(4);
    ec.trace_state(&format!("before_stx idx={} cdf={:?}", stx_idx, &cdf.stx[stx_idx]));
    let stx_flag = ec.decode_symbol_mut(&mut cdf.stx[stx_idx]);
    ec.trace_state(&format!("after_stx_flag={} txs={}", stx_flag, square_tx_size));

    if stx_flag > 0 {
        if primary_tx_type == ADST_ADST && width >= 8 && height >= 8 {
            let _stx_set = ec.decode_symbol_mut(&mut cdf.most_probable_stx_set_adst_adst);
            ec.trace_state(&format!("after_stx_set_adst={}", _stx_set));
        } else {
            let _stx_set = ec.decode_symbol_mut(&mut cdf.most_probable_stx_set);
            ec.trace_state(&format!("after_stx_set={}", _stx_set));
        }
    }
}

fn decode_coeff_tu(
    ec: &mut DaalaDecoder,
    plane: usize,
    tx_size: TxSize,
    dc_sign_ctx: usize,
    entropy_ctx: usize,
    u_has_coeffs: bool,
    y_mode: u8,
    fsc_mode: usize,
    cfg: &TileDecodeConfig,
    cdf: &mut FrameCdfContext,
) -> TuData {
    let bwl = tx_size.bwl();
    let width = tx_size.coded_width();
    let height = tx_size.coded_height();
    let num_coeffs = tx_size.num_coeffs();
    let scan = get_scan(tx_size);
    let txs_idx = tx_size.txs_ctx();

    let plane_name = match plane { 0 => "y", 1 => "u", _ => "v" };
    ec.trace_state(&format!("before_txb_skip_{} txs_ctx={} skip_ctx={}", plane_name, txs_idx, entropy_ctx));

    let skip = match plane {
        0 => ec.decode_symbol_mut(&mut cdf.txb_skip_y[txs_idx][entropy_ctx]),
        1 => ec.decode_symbol_mut(&mut cdf.txb_skip_u[entropy_ctx]),
        _ => {
            if u_has_coeffs {
                ec.decode_symbol_mut(&mut cdf.txb_skip_v_u_nz[entropy_ctx])
            } else {
                ec.decode_symbol_mut(&mut cdf.txb_skip_v[entropy_ctx])
            }
        }
    };

    if skip == 1 {
        return TuData {
            levels: vec![0; num_coeffs],
            eob: 0,
            tx_size,
            row_offset: 0,
            col_offset: 0,
        };
    }

    let pl_ctx = if plane == 0 { 0 } else { 2 };
    let eob = decode_eob_var(ec, tx_size, pl_ctx, cdf);
    ec.trace_state(&format!("after_eob_{} eob={} txs={}", plane_name, eob, txs_idx));

    let tx_type = if plane == 0 {
        read_tx_type(ec, tx_size, y_mode, fsc_mode, cfg.reduced_tx_set_used, cdf)
    } else {
        DCT_DCT
    };
    ec.trace_state(&format!("after_tx_type_{} type={}", plane_name, tx_type));

    if plane == 1 && cfg.enable_cctx && eob > 1 {
        let _cctx = ec.decode_symbol_mut(&mut cdf.cctx_type);
        ec.trace_state(&format!("after_cctx_type={}", _cctx));
    }

    let padded_stride = width + TX_PAD_HOR;
    let padded_height = height + 2;
    if plane == 0 && cfg.enable_ist && eob > 1 && fsc_mode == 0 {
        read_sec_tx_type(ec, tx_size, tx_type, y_mode, eob as usize, cdf);
    }

    let mut levels_buf = vec![0u8; padded_stride * padded_height];
    let mut abs_levels = vec![0usize; eob as usize];

    let eob_scan_idx = (eob - 1) as usize;
    let eob_pos = scan[eob_scan_idx] as usize;
    let eob_row = eob_pos >> bwl;
    let eob_col = eob_pos - (eob_row << bwl);
    let eob_lf = is_lf_2d(eob_row, eob_col, plane);

    let coeff_ctx = get_lower_levels_ctx_eob(bwl, height, eob_scan_idx);

    if plane == 0 {
        if eob_lf {
            let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_lf_eob_y[txs_idx][coeff_ctx]);
            let base_value = sym + 1;
            if base_value > LF_NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_lf_eob_2d(eob_pos);
                abs_levels[eob_scan_idx] = decode_low_range(
                    ec,
                    LF_NUM_BASE_LEVELS,
                    &COEFF_BR_LF_Y_ICDF[br_ctx],
                    Some(&mut cdf.coeff_br_lf_y[br_ctx]),
                );
            } else {
                abs_levels[eob_scan_idx] = base_value;
            }
        } else {
            let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_eob_y[txs_idx][coeff_ctx]);
            let base_value = sym + 1;
            if base_value > NUM_BASE_LEVELS {
                abs_levels[eob_scan_idx] = decode_low_range(
                    ec,
                    NUM_BASE_LEVELS,
                    &COEFF_BR_Y_ICDF[0],
                    Some(&mut cdf.coeff_br_y[0]),
                );
            } else {
                abs_levels[eob_scan_idx] = base_value;
            }
        }
    } else if eob_lf {
        let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_lf_eob_uv[coeff_ctx]);
        abs_levels[eob_scan_idx] = sym + 1;
    } else {
        let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_eob_uv[coeff_ctx]);
        let base_value = sym + 1;
        if base_value > NUM_BASE_LEVELS {
            abs_levels[eob_scan_idx] = decode_low_range(
                ec,
                NUM_BASE_LEVELS,
                &COEFF_BR_UV_ICDF[0],
                Some(&mut cdf.coeff_br_uv[0]),
            );
        } else {
            abs_levels[eob_scan_idx] = base_value;
        }
    }

    levels_buf[get_padded_idx(eob_pos, bwl)] =
        stored_level(abs_levels[eob_scan_idx], eob_lf, plane);

    if eob > 1 {
        for c in (1..eob_scan_idx).rev() {
            let pos = scan[c] as usize;
            let row = pos >> bwl;
            let col = pos - (row << bwl);
            let lf = is_lf_2d(row, col, plane);

            if plane == 0 {
                if lf {
                    let lctx = get_lower_levels_ctx_lf_2d(&levels_buf, pos, bwl);
                    let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_lf_y[txs_idx][lctx]);
                    if sym > LF_NUM_BASE_LEVELS {
                        let br_ctx = get_br_lf_ctx_2d(&levels_buf, pos, bwl);
                        abs_levels[c] = decode_low_range(
                            ec,
                            LF_NUM_BASE_LEVELS,
                            &COEFF_BR_LF_Y_ICDF[br_ctx],
                            Some(&mut cdf.coeff_br_lf_y[br_ctx]),
                        );
                    } else {
                        abs_levels[c] = sym;
                    }
                } else {
                    let lctx = get_lower_levels_ctx_2d(&levels_buf, pos, bwl, plane);
                    let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_y[txs_idx][lctx]);
                    if sym > NUM_BASE_LEVELS {
                        let br_ctx = get_br_ctx_2d(&levels_buf, pos, bwl);
                        abs_levels[c] = decode_low_range(
                            ec,
                            NUM_BASE_LEVELS,
                            &COEFF_BR_Y_ICDF[br_ctx],
                            Some(&mut cdf.coeff_br_y[br_ctx]),
                        );
                    } else {
                        abs_levels[c] = sym;
                    }
                }
            } else if lf {
                let lctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, pos, bwl, plane);
                let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_lf_uv[lctx]);
                abs_levels[c] = sym;
            } else {
                let lctx = get_lower_levels_ctx_2d_chroma(&levels_buf, pos, bwl, plane);
                let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_uv[lctx]);
                if sym > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d_chroma(&levels_buf, pos, bwl);
                    abs_levels[c] = decode_low_range(
                        ec,
                        NUM_BASE_LEVELS,
                        &COEFF_BR_UV_ICDF[br_ctx],
                        Some(&mut cdf.coeff_br_uv[br_ctx]),
                    );
                } else {
                    abs_levels[c] = sym;
                }
            }

            levels_buf[get_padded_idx(pos, bwl)] = stored_level(abs_levels[c], lf, plane);
        }

        let dc_pos = scan[0] as usize;
        let dc_row = dc_pos >> bwl;
        let dc_col = dc_pos - (dc_row << bwl);
        let dc_lf = is_lf_2d(dc_row, dc_col, plane);

        if plane == 0 {
            if dc_lf {
                let lctx = get_lower_levels_ctx_lf_2d(&levels_buf, dc_pos, bwl);
                let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_lf_y[txs_idx][lctx]);
                if sym > LF_NUM_BASE_LEVELS {
                    let br_ctx = get_br_lf_ctx_dc_2d(&levels_buf, dc_pos, bwl);
                    abs_levels[0] = decode_low_range(
                        ec,
                        LF_NUM_BASE_LEVELS,
                        &COEFF_BR_LF_Y_ICDF[br_ctx],
                        Some(&mut cdf.coeff_br_lf_y[br_ctx]),
                    );
                } else {
                    abs_levels[0] = sym;
                }
            } else {
                let lctx = get_lower_levels_ctx_2d(&levels_buf, dc_pos, bwl, plane);
                let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_y[txs_idx][lctx]);
                if sym > NUM_BASE_LEVELS {
                    let br_ctx = get_br_ctx_2d(&levels_buf, dc_pos, bwl);
                    abs_levels[0] = decode_low_range(
                        ec,
                        NUM_BASE_LEVELS,
                        &COEFF_BR_Y_ICDF[br_ctx],
                        Some(&mut cdf.coeff_br_y[br_ctx]),
                    );
                } else {
                    abs_levels[0] = sym;
                }
            }
        } else if dc_lf {
            let lctx = get_lower_levels_ctx_lf_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_lf_uv[lctx]);
            abs_levels[0] = sym;
        } else {
            let lctx = get_lower_levels_ctx_2d_chroma(&levels_buf, dc_pos, bwl, plane);
            let sym = ec.decode_symbol_mut(&mut cdf.coeff_base_uv[lctx]);
            if sym > NUM_BASE_LEVELS {
                let br_ctx = get_br_ctx_2d_chroma(&levels_buf, dc_pos, bwl);
                abs_levels[0] = decode_low_range(
                    ec,
                    NUM_BASE_LEVELS,
                    &COEFF_BR_UV_ICDF[br_ctx],
                    Some(&mut cdf.coeff_br_uv[br_ctx]),
                );
            } else {
                abs_levels[0] = sym;
            }
        }

        levels_buf[get_padded_idx(dc_pos, bwl)] = stored_level(abs_levels[0], dc_lf, plane);
    }

    let mut levels = vec![0i32; num_coeffs];
    let mut hr_avg = 0usize;
    for c in (0..eob as usize).rev() {
        let pos = scan[c] as usize;
        let abs_level = abs_levels[c];
        if abs_level == 0 {
            continue;
        }

        let row = pos >> bwl;
        let col = pos - (row << bwl);
        let is_dc_2d = c == 0;

        let sign = if is_dc_2d {
            if plane == 0 {
                ec.decode_symbol_mut(&mut cdf.dc_sign_y[dc_sign_ctx])
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

    TuData {
        levels,
        eob,
        tx_size,
        row_offset: 0,
        col_offset: 0,
    }
}

fn decode_block_tus(
    ec: &mut DaalaDecoder,
    plane: usize,
    block_w: usize,
    block_h: usize,
    dc_sign_ctx: usize,
    entropy_ctx: usize,
    u_has_coeffs: bool,
    y_mode: u8,
    fsc_mode: usize,
    cfg: &TileDecodeConfig,
    cdf: &mut FrameCdfContext,
) -> Vec<TuData> {
    let tx_partition = if cfg.tx_mode_select
        && !(block_w == 4 && block_h == 4)
        && plane == 0
    {
        read_tx_partition(ec, block_w, block_h, fsc_mode, cfg.reduced_tx_part_set, cdf)
    } else {
        TxPartitionType::None
    };

    let max_tx_w = block_w.min(64);
    let max_tx_h = block_h.min(64);
    let sub_tus = get_tx_partition_sub_tus(tx_partition, max_tx_w, max_tx_h);

    let mut tus = Vec::with_capacity(sub_tus.len());
    for (sub_tx_size, row_off, col_off) in &sub_tus {
        let mut tu = decode_coeff_tu(
            ec,
            plane,
            *sub_tx_size,
            dc_sign_ctx,
            entropy_ctx,
            u_has_coeffs,
            y_mode,
            fsc_mode,
            cfg,
            cdf,
        );
        tu.row_offset = *row_off;
        tu.col_offset = *col_off;
        tus.push(tu);
    }
    tus
}

fn read_y_mode(ec: &mut DaalaDecoder, y_mode_ctx: usize, cdf: &mut FrameCdfContext) -> u8 {
    ec.trace_state("before_mode_set");
    let mode_set = ec.decode_symbol_mut(&mut cdf.y_mode_set);
    ec.trace_state(&format!("after_mode_set={}", mode_set));
    if mode_set == 0 {
        let mut mode_idx = ec.decode_symbol_mut(&mut cdf.y_mode_idx[y_mode_ctx]);
        ec.trace_state(&format!("after_mode_idx={} ctx={}", mode_idx, y_mode_ctx));
        if mode_idx == 7 {
            let offset = ec.decode_symbol_mut(&mut cdf.y_mode_idx_offset[y_mode_ctx]);
            ec.trace_state(&format!("after_mode_offset={}", offset));
            mode_idx += offset;
        }
        mode_idx as u8
    } else {
        let literal = ec.decode_literal(4) as usize;
        (FIRST_MODE_COUNT + (mode_set - 1) * SECOND_MODE_COUNT + literal) as u8
    }
}

fn is_cfl_allowed(block_w: usize, block_h: usize) -> bool {
    let uv_w = block_w / 2;
    let uv_h = block_h / 2;
    uv_w >= 4 && uv_h >= 4 && uv_w == uv_h
}

fn read_uv_mode(
    ec: &mut DaalaDecoder,
    y_mode: u8,
    block_w: usize,
    block_h: usize,
    cfl_ctx: usize,
    enable_cfl_intra: bool,
    cdf: &mut FrameCdfContext,
) -> u8 {
    if is_cfl_allowed(block_w, block_h) {
        ec.trace_state(&format!("before_is_cfl ctx={}", cfl_ctx));
        let is_cfl = ec.decode_symbol_mut(&mut cdf.cfl[cfl_ctx]);
        ec.trace_state(&format!("after_is_cfl={}", is_cfl));
        if is_cfl == 1 {
            if enable_cfl_intra {
                let cfl_idx = ec.decode_symbol_mut(&mut cdf.cfl_index);
                ec.trace_state(&format!("after_cfl_index={}", cfl_idx));
                if cfl_idx != 0 {
                    return 13;
                }
            }
            let joint_sign = ec.decode_symbol_mut(&mut cdf.cfl_sign);
            ec.trace_state(&format!("after_cfl_sign={}", joint_sign));
            let u_sign = (joint_sign + 1) / CFL_SIGNS;
            let v_sign = (joint_sign + 1) % CFL_SIGNS;
            if u_sign != 0 {
                let alpha_u_ctx = joint_sign - 2;
                let _alpha_u = ec.decode_symbol_mut(&mut cdf.cfl_alpha[alpha_u_ctx]);
                ec.trace_state(&format!("after_cfl_alpha_u={} ctx={}", _alpha_u, alpha_u_ctx));
            }
            if v_sign != 0 {
                let alpha_v_ctx = v_sign * CFL_SIGNS + u_sign - CFL_SIGNS;
                let _alpha_v = ec.decode_symbol_mut(&mut cdf.cfl_alpha[alpha_v_ctx]);
                ec.trace_state(&format!("after_cfl_alpha_v={} ctx={}", _alpha_v, alpha_v_ctx));
            }
            return 13;
        }
    }

    let uv_ctx = if (y_mode as usize) >= NON_DIRECTIONAL_MODES_COUNT { 1 } else { 0 };
    ec.trace_state(&format!("before_uv_mode ctx={}", uv_ctx));
    let mut uv_mode_idx = ec.decode_symbol_mut(&mut cdf.uv_mode[uv_ctx]);
    if uv_mode_idx == 7 {
        uv_mode_idx += ec.decode_literal(3) as usize;
    }
    uv_mode_idx as u8
}

struct ModeInfoGrid {
    mi_cols: usize,
    is_directional: Vec<bool>,
    is_cfl: Vec<bool>,
}

impl ModeInfoGrid {
    fn new(frame_w: usize, frame_h: usize) -> Self {
        let mi_cols = (frame_w + 3) / 4;
        let mi_rows = (frame_h + 3) / 4;
        let n = mi_cols * mi_rows;
        Self {
            mi_cols,
            is_directional: vec![false; n],
            is_cfl: vec![false; n],
        }
    }

    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, directional: bool, cfl: bool) {
        let mi_x = x / 4;
        let mi_y = y / 4;
        let mi_w = w / 4;
        let mi_h = h / 4;
        let mi_rows = self.is_directional.len() / self.mi_cols;
        for r in mi_y..(mi_y + mi_h).min(mi_rows) {
            for c in mi_x..(mi_x + mi_w).min(self.mi_cols) {
                let idx = r * self.mi_cols + c;
                self.is_directional[idx] = directional;
                self.is_cfl[idx] = cfl;
            }
        }
    }

    fn y_mode_ctx(&self, x: usize, y: usize) -> usize {
        let mi_x = x / 4;
        let mi_y = y / 4;
        let mut cnt = 0;
        if mi_y > 0 {
            let above = (mi_y - 1) * self.mi_cols + mi_x;
            if self.is_directional[above] {
                cnt += 1;
            }
        }
        if mi_x > 0 {
            let left = mi_y * self.mi_cols + (mi_x - 1);
            if self.is_directional[left] {
                cnt += 1;
            }
        }
        cnt
    }

    fn cfl_ctx(&self, x: usize, y: usize) -> usize {
        let mi_x = x / 4;
        let mi_y = y / 4;
        let mut cnt = 0;
        if mi_y > 0 {
            let above = (mi_y - 1) * self.mi_cols + mi_x;
            if self.is_cfl[above] {
                cnt += 1;
            }
        }
        if mi_x > 0 {
            let left = mi_y * self.mi_cols + (mi_x - 1);
            if self.is_cfl[left] {
                cnt += 1;
            }
        }
        cnt
    }
}

fn get_palette_color_index_context(
    color_map: &[u8],
    stride: usize,
    r: usize,
    c: usize,
    color_order: &mut [u8],
    n_colors: usize,
) -> usize {
    let mut scores = [0u32; 8];

    if c > 0 {
        scores[color_map[r * stride + c - 1] as usize] += 2;
    }
    if r > 0 {
        scores[color_map[(r - 1) * stride + c] as usize] += 2;
    }
    if c > 0 && r > 0 {
        scores[color_map[(r - 1) * stride + c - 1] as usize] += 1;
    }

    let l = if c > 0 { color_map[r * stride + c - 1] } else { 255 };
    let t = if r > 0 { color_map[(r - 1) * stride + c] } else { 255 };
    let tl = if c > 0 && r > 0 { color_map[(r - 1) * stride + c - 1] } else { 255 };

    let ctx = if c == 0 && r == 0 {
        0
    } else if c == 0 || r == 0 {
        0
    } else if l == tl && tl == t {
        4
    } else if l == t {
        3
    } else if l == tl || tl == t {
        2
    } else {
        1
    };

    let mut order: Vec<(u32, u8)> = (0..n_colors as u8).map(|i| (scores[i as usize], i)).collect();
    order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (i, (_, idx)) in order.iter().enumerate() {
        color_order[i] = *idx;
    }

    ctx
}

fn read_palette_colors_y(
    ec: &mut DaalaDecoder,
    n: usize,
    palette_cache: &[u16],
) -> Vec<u16> {
    let bit_depth = 8u32;
    let n_cache = palette_cache.len();
    let mut colors = Vec::with_capacity(n);

    let mut idx = 0;
    for i in 0..n_cache {
        if idx >= n {
            break;
        }
        let use_cache = ec.decode_bit();
        if use_cache != 0 {
            colors.push(palette_cache[i]);
            idx += 1;
        }
    }

    if idx < n {
        colors.push(ec.decode_literal(bit_depth) as u16);
        idx += 1;
    }

    if idx < n {
        let min_bits = bit_depth - 3;
        let mut bits = min_bits + ec.decode_literal(2);

        for _ in idx..n {
            let prev = *colors.last().unwrap();
            let range = ((1u32 << bit_depth) - prev as u32 - 1) as i32;
            if range <= 0 {
                colors.push(((1u32 << bit_depth) - 1) as u16);
            } else {
                let delta = ec.decode_literal(bits) + 1;
                let new_val = (prev as u32 + delta).min((1u32 << bit_depth) - 1);
                colors.push(new_val as u16);
                let new_range = range - (new_val as i32 - prev as i32);
                if new_range > 0 {
                    let log2_range = 32 - (new_range as u32).leading_zeros();
                    bits = bits.min(log2_range);
                } else {
                    bits = 0;
                }
            }
        }
    }

    for i in 0..colors.len() {
        for j in 1..colors.len() - i {
            if colors[j - 1] > colors[j] {
                colors.swap(j - 1, j);
            }
        }
    }

    colors
}

fn decode_palette_map(
    ec: &mut DaalaDecoder,
    width: usize,
    height: usize,
    n_colors: usize,
    cdf: &mut FrameCdfContext,
) {
    let transverse_allowed = width < 64 && height < 64;
    let direction = if transverse_allowed {
        ec.decode_bit() as usize
    } else {
        0
    };

    let (axis2_limit, axis1_limit) = if direction == 0 {
        (height, width)
    } else {
        (width, height)
    };

    let plane_block_width = width;
    let mut color_map = vec![0u8; width * height];
    let ps = n_colors - 2;

    let mut prev_identity_row_flag = 0usize;

    for ax2 in 0..axis2_limit {
        let ctx = if ax2 == 0 { 3 } else { prev_identity_row_flag };
        let identity_row_flag = ec.decode_symbol_mut(&mut cdf.identity_row_y[ctx]);

        if identity_row_flag == 2 && ax2 > 0 {
            for ax1 in 0..axis1_limit {
                let (r, c) = if direction == 0 { (ax2, ax1) } else { (ax1, ax2) };
                let (pr, pc) = if direction == 0 { (ax2 - 1, ax1) } else { (ax1, ax2 - 1) };
                color_map[r * plane_block_width + c] = color_map[pr * plane_block_width + pc];
            }
        } else if identity_row_flag == 1 {
            for ax1 in 0..axis1_limit {
                let (r, c) = if direction == 0 { (ax2, ax1) } else { (ax1, ax2) };
                if ax1 == 0 {
                    if ax2 == 0 {
                        let val = ec.decode_uniform(n_colors as u32) as u8;
                        color_map[r * plane_block_width + c] = val;
                    } else {
                        let mut color_order = [0u8; 8];
                        let color_ctx = get_palette_color_index_context(
                            &color_map, plane_block_width, r, c, &mut color_order, n_colors,
                        );
                        let color_idx = ec.decode_symbol_mut(&mut cdf.palette_y_color_index[ps][color_ctx]);
                        color_map[r * plane_block_width + c] = color_order[color_idx];
                    }
                } else {
                    let (nr, nc) = if direction == 0 {
                        (r, c.wrapping_sub(1))
                    } else {
                        (r.wrapping_sub(1), c)
                    };
                    if nr < height && nc < width {
                        color_map[r * plane_block_width + c] = color_map[nr * plane_block_width + nc];
                    }
                }
            }
        } else {
            for ax1 in 0..axis1_limit {
                let (r, c) = if direction == 0 { (ax2, ax1) } else { (ax1, ax2) };
                if ax2 == 0 && ax1 == 0 {
                    let val = ec.decode_uniform(n_colors as u32) as u8;
                    color_map[r * plane_block_width + c] = val;
                } else {
                    let mut color_order = [0u8; 8];
                    let color_ctx = get_palette_color_index_context(
                        &color_map, plane_block_width, r, c, &mut color_order, n_colors,
                    );
                    let color_idx = ec.decode_symbol_mut(&mut cdf.palette_y_color_index[ps][color_ctx]);
                    color_map[r * plane_block_width + c] = color_order[color_idx];
                }
            }
        }

        prev_identity_row_flag = identity_row_flag;
    }
}

fn decode_leaf_block(
    ec: &mut DaalaDecoder,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    tree_type: TreeType,
    cfg: &TileDecodeConfig,
    cdf: &mut FrameCdfContext,
    mi: &mut ModeInfoGrid,
    blocks: &mut Vec<DecodedBlock>,
    state: &mut TileState,
) {
    if tree_type != TreeType::Chroma {
        if cfg.allow_intrabc && !(width == 64 && height == 64) && width <= 64 && height <= 64 {
            let ctx = 0;
            let _use_intrabc = ec.decode_symbol_mut(&mut cdf.intrabc[ctx]);
            ec.trace_state(&format!("after_use_intrabc={} block=({},{})", _use_intrabc, x, y));
        }

        if cfg.cdef_frame_enable && cfg.nb_cdef_strengths >= 2 {
            let cdef_unit = (x / CDEF_UNIT_SIZE, y / CDEF_UNIT_SIZE);
            if !state.cdef_read.contains(&cdef_unit) {
                let ctx = 0;
                let _cdef_strength_idx0 = ec.decode_symbol_mut(&mut cdf.cdef_strength_index0[ctx]);
                ec.trace_state(&format!("after_cdef_idx0={} block=({},{})", _cdef_strength_idx0, x, y));
                state.cdef_read.insert(cdef_unit);
            }
        }

        if cfg.ccso_frame_enable {
            let ccso_unit = (x / CCSO_UNIT_SIZE, y / CCSO_UNIT_SIZE);
            for plane in 0..3 {
                if cfg.ccso_enable[plane] && !state.ccso_read[plane].contains(&ccso_unit) {
                    let ctx = 0;
                    let _ccso_blk = ec.decode_symbol_mut(&mut cdf.ccso[plane][ctx]);
                    ec.trace_state(&format!("after_ccso_p{}={} block=({},{})", plane, _ccso_blk, x, y));
                    state.ccso_read[plane].insert(ccso_unit);
                }
            }
        }
    }

    let mut y_mode = 0u8;
    let mut fsc_mode = 0usize;
    let mut is_directional = false;

    if tree_type != TreeType::Chroma {
        let y_mode_ctx = mi.y_mode_ctx(x, y);
        y_mode = read_y_mode(ec, y_mode_ctx, cdf);

        fsc_mode = if cfg.enable_idtx_intra && allow_fsc_intra(width, height) {
            let bsize_group = fsc_bsize_group(width, height);
            let ctx = 0;
            let fsc = ec.decode_symbol_mut(&mut cdf.fsc_mode[ctx][bsize_group]);
            ec.trace_state(&format!("after_fsc_mode={} bsg={} block=({},{})", fsc, bsize_group, x, y));
            fsc
        } else {
            0
        };

        is_directional = y_mode >= 1 && y_mode <= 8;
        let _mrl_index = if cfg.enable_mrls && is_directional {
            let ctx = 0;
            let mrl = ec.decode_symbol_mut(&mut cdf.mrl_index[ctx]);
            ec.trace_state(&format!("after_mrl_index={} block=({},{})", mrl, x, y));
            if mrl > 0 {
                let multi_ctx = 0;
                let _multi = ec.decode_symbol_mut(&mut cdf.multi_line_mrl[multi_ctx]);
                ec.trace_state(&format!("after_multi_line_mrl={} block=({},{})", _multi, x, y));
            }
            mrl
        } else {
            0
        };
    }

    let mut uv_mode = 0u8;
    if tree_type != TreeType::Luma {
        let cfl_ctx = mi.cfl_ctx(x, y);
        uv_mode = read_uv_mode(ec, y_mode, width, height, cfl_ctx, cfg.enable_cfl_intra, cdf);
    }

    let mut palette_y_active = false;
    if tree_type != TreeType::Chroma
        && cfg.allow_screen_content_tools
        && y_mode == 0
        && width * height >= 64
        && width <= 64
        && height <= 64
    {
        ec.trace_state(&format!("before_palette_y block=({},{}) {}x{}", x, y, width, height));
        let palette_y = ec.decode_symbol_mut(&mut cdf.palette_y_mode);
        ec.trace_state(&format!("after_palette_y={} block=({},{})", palette_y, x, y));

        if palette_y == 1 {
            palette_y_active = true;
            let palette_size_raw = ec.decode_symbol_mut(&mut cdf.palette_y_size);
            let n_colors = palette_size_raw + 2;
            ec.trace_state(&format!("after_palette_size={} n_colors={} block=({},{})", palette_size_raw, n_colors, x, y));

            let palette_cache: Vec<u16> = Vec::new();
            let _palette_colors = read_palette_colors_y(ec, n_colors, &palette_cache);
            ec.trace_state(&format!("after_palette_colors block=({},{})", x, y));

            decode_palette_map(ec, width, height, n_colors, cdf);
            ec.trace_state(&format!("after_palette_map block=({},{})", x, y));
        }
    }

    let is_cfl = uv_mode == 13;
    mi.fill(x, y, width, height, is_directional, is_cfl);

    let _ = palette_y_active;
    let y_tu = if tree_type != TreeType::Chroma {
        decode_block_tus(ec, 0, width, height, 0, 0, false, y_mode, fsc_mode, cfg, cdf)
    } else {
        vec![]
    };

    let uv_w = width / 2;
    let uv_h = height / 2;
    let (u_tu, v_tu) = if tree_type != TreeType::Luma && uv_w >= 4 && uv_h >= 4 {
        let u_tu = decode_block_tus(ec, 1, uv_w, uv_h, 0, 0, false, y_mode, 0, cfg, cdf);
        let v_u_has = u_tu.iter().any(|t| t.eob > 0);
        let v_tu = decode_block_tus(ec, 2, uv_w, uv_h, 0, 0, v_u_has, y_mode, 0, cfg, cdf);
        ec.trace_state(&format!("after_v_coeffs block=({},{})", x, y));
        (u_tu, v_tu)
    } else {
        if tree_type != TreeType::Chroma {
            ec.trace_state(&format!("after_luma_coeffs block=({},{})", x, y));
        }
        (vec![], vec![])
    };

    blocks.push(DecodedBlock {
        x,
        y,
        width,
        height,
        y_mode,
        uv_mode,
        y_tu,
        u_tu,
        v_tu,
    });
}

fn subblock_offsets(
    partition: PartitionType,
    w: usize,
    h: usize,
) -> Vec<(usize, usize, usize, usize)> {
    match partition {
        PartitionType::None => vec![(0, 0, w, h)],
        PartitionType::Horz => vec![(0, 0, w, h / 2), (0, h / 2, w, h / 2)],
        PartitionType::Vert => vec![(0, 0, w / 2, h), (w / 2, 0, w / 2, h)],
        PartitionType::Split => {
            let hw = w / 2;
            let hh = h / 2;
            vec![
                (0, 0, hw, hh),
                (hw, 0, hw, hh),
                (0, hh, hw, hh),
                (hw, hh, hw, hh),
            ]
        }
        PartitionType::Horz3 => {
            let qh = h / 4;
            let hw = w / 2;
            let hh = h / 2;
            vec![
                (0, 0, w, qh),
                (0, qh, hw, hh),
                (hw, qh, hw, hh),
                (0, qh + hh, w, qh),
            ]
        }
        PartitionType::Vert3 => {
            let qw = w / 4;
            let hw = w / 2;
            let hh = h / 2;
            vec![
                (0, 0, qw, h),
                (qw, 0, hw, hh),
                (qw, hh, hw, hh),
                (qw + hw, 0, qw, h),
            ]
        }
        PartitionType::Horz4A => {
            let e = h / 8;
            vec![
                (0, 0, w, e),
                (0, e, w, 2 * e),
                (0, 3 * e, w, 4 * e),
                (0, 7 * e, w, e),
            ]
        }
        PartitionType::Horz4B => {
            let e = h / 8;
            vec![
                (0, 0, w, e),
                (0, e, w, 4 * e),
                (0, 5 * e, w, 2 * e),
                (0, 7 * e, w, e),
            ]
        }
        PartitionType::Vert4A => {
            let e = w / 8;
            vec![
                (0, 0, e, h),
                (e, 0, 2 * e, h),
                (3 * e, 0, 4 * e, h),
                (7 * e, 0, e, h),
            ]
        }
        PartitionType::Vert4B => {
            let e = w / 8;
            vec![
                (0, 0, e, h),
                (e, 0, 4 * e, h),
                (5 * e, 0, 2 * e, h),
                (7 * e, 0, e, h),
            ]
        }
    }
}

fn bsize_map_val(w: usize, h: usize) -> usize {
    match (w, h) {
        (4, 4) | (4, 8) | (8, 4) | (8, 8) => 0,
        (8, 16) | (16, 8) | (16, 16) => 1,
        (16, 32) | (32, 16) | (32, 32) => 2,
        (32, 64) | (64, 32) | (64, 64) => 3,
        (64, 128) => 4,
        (128, 64) => 5,
        (128, 128) => 6,
        (4, 16) => 10,
        (16, 4) => 11,
        (8, 32) => 12,
        (32, 8) => 13,
        (16, 64) => 14,
        (64, 16) => 15,
        _ => 0,
    }
}

fn bsize_rect_map_val(w: usize, h: usize) -> usize {
    match (w, h) {
        (4, 4) | (4, 8) | (8, 4) | (8, 8) | (16, 16) => 0,
        (8, 16) | (16, 32) => 1,
        (16, 8) | (32, 16) => 2,
        (32, 32) => 3,
        (32, 64) => 4,
        (64, 32) => 5,
        (64, 64) => 6,
        (64, 128) => 7,
        (128, 64) => 8,
        (128, 128) => 9,
        (4, 16) | (8, 32) | (16, 64) => 13,
        (16, 4) | (32, 8) | (64, 16) => 14,
        _ => 0,
    }
}

fn is_partition_point(w: usize, h: usize) -> bool {
    matches!(
        (w, h),
        (4, 8)
            | (8, 4)
            | (8, 8)
            | (8, 16)
            | (16, 8)
            | (16, 16)
            | (16, 32)
            | (32, 16)
            | (32, 32)
            | (32, 64)
            | (64, 32)
            | (64, 64)
            | (64, 128)
            | (128, 64)
            | (128, 128)
            | (256, 256)
            | (256, 128)
            | (128, 256)
            | (4, 16)
            | (16, 4)
            | (8, 32)
            | (32, 8)
            | (16, 64)
            | (64, 16)
    )
}

fn is_square_split_eligible(w: usize, h: usize) -> bool {
    w == h && (w == 128 || w == 256)
}

fn rect_type_implied(w: usize, h: usize) -> Option<usize> {
    match (w, h) {
        (4, 8) | (64, 128) | (4, 16) => Some(0),
        (8, 4) | (128, 64) | (16, 4) => Some(1),
        _ => None,
    }
}

fn is_ext_partition_valid(w: usize, h: usize, rect_type: usize) -> bool {
    if w.min(h) < 16 {
        return false;
    }
    let (sub_w, sub_h) = if rect_type == 0 {
        (w, h / 4)
    } else {
        (w / 4, h)
    };
    sub_w >= 4 && sub_h >= 4 && is_partition_point(sub_w, sub_h)
}

fn is_3way_valid(w: usize, h: usize, rect_type: usize) -> bool {
    let (sub_w, sub_h) = if rect_type == 0 {
        (w / 2, h / 2)
    } else {
        (w / 2, h / 2)
    };
    sub_w >= 4 && sub_h >= 4
}

fn is_4way_valid(w: usize, h: usize, rect_type: usize) -> bool {
    let min_sub = if rect_type == 0 { h / 8 } else { w / 8 };
    min_sub >= 4
}

fn partition_split_ctx(w: usize, h: usize) -> usize {
    (bsize_map_val(w, h) * PARTITION_PLOFFSET).min(PARTITION_CONTEXTS - 1)
}

fn partition_rect_ctx(w: usize, h: usize) -> usize {
    (bsize_rect_map_val(w, h) * PARTITION_PLOFFSET).min(PARTITION_CONTEXTS - 1)
}

fn partition_sq_split_ctx(w: usize, _h: usize) -> usize {
    if w == 256 { PARTITION_PLOFFSET } else { 0 }
}

fn read_partition_type(
    ec: &mut DaalaDecoder,
    w: usize,
    h: usize,
    cdf: &mut FrameCdfContext,
) -> PartitionType {
    let plane = 0;

    let split_ctx = partition_split_ctx(w, h);
    ec.trace_state(&format!("before_do_split bsize={}x{} ctx={}", w, h, split_ctx));
    let do_split = ec.decode_symbol_mut(&mut cdf.do_split_part[plane][split_ctx.min(63)]);
    ec.trace_state(&format!("after_do_split={} bsize={}x{}", do_split, w, h));
    if do_split == 0 {
        return PartitionType::None;
    }

    if is_square_split_eligible(w, h) {
        let sq_ctx = partition_sq_split_ctx(w, h).min(SQUARE_SPLIT_CONTEXTS - 1);
        let do_sq = ec.decode_symbol_mut(&mut cdf.do_square_split[plane][sq_ctx]);
        ec.trace_state(&format!("after_do_sq_split={} bsize={}x{}", do_sq, w, h));
        if do_sq == 1 {
            return PartitionType::Split;
        }
    }

    let rect_type = if let Some(implied) = rect_type_implied(w, h) {
        implied
    } else {
        let rect_ctx = partition_rect_ctx(w, h);
        let rt = ec.decode_symbol_mut(&mut cdf.rect_type[plane][rect_ctx.min(63)]);
        ec.trace_state(&format!("after_rect_type={} ctx={} bsize={}x{}", rt, rect_ctx, w, h));
        rt
    };

    let base_type = if rect_type == 0 {
        PartitionType::Horz
    } else {
        PartitionType::Vert
    };

    if !is_ext_partition_valid(w, h, rect_type) {
        return base_type;
    }

    let ext_3way = is_3way_valid(w, h, rect_type);
    let ext_4way = is_4way_valid(w, h, rect_type);

    if !ext_3way && !ext_4way {
        return base_type;
    }

    let do_ext = if ext_3way || ext_4way {
        let ext_ctx = partition_split_ctx(w, h);
        eprintln!("[CDF] do_ext_partition[{}][{}] = {:?}", plane, ext_ctx.min(63), &cdf.do_ext_partition[plane][ext_ctx.min(63)]);
        let ext = ec.decode_symbol_mut(&mut cdf.do_ext_partition[plane][ext_ctx.min(63)]);
        ec.trace_state(&format!("after_do_ext={} ctx={} bsize={}x{}", ext, ext_ctx, w, h));
        ext
    } else {
        0
    };

    if do_ext == 0 {
        return base_type;
    }

    if !ext_4way {
        return match base_type {
            PartitionType::Horz => PartitionType::Horz3,
            PartitionType::Vert => PartitionType::Vert3,
            _ => base_type,
        };
    }

    if !ext_3way {
        let four_way_type = ec.decode_literal(1);
        return match (base_type, four_way_type) {
            (PartitionType::Horz, 0) => PartitionType::Horz4A,
            (PartitionType::Horz, _) => PartitionType::Horz4B,
            (PartitionType::Vert, 0) => PartitionType::Vert4A,
            (PartitionType::Vert, _) => PartitionType::Vert4B,
            _ => base_type,
        };
    }

    let uneven_ctx = partition_split_ctx(w, h);
    let uneven = ec.decode_symbol_mut(&mut cdf.do_uneven_4way[plane][uneven_ctx.min(63)]);
    if uneven == 0 {
        return match base_type {
            PartitionType::Horz => PartitionType::Horz3,
            PartitionType::Vert => PartitionType::Vert3,
            _ => base_type,
        };
    }

    let four_way_type = ec.decode_literal(1);
    match (base_type, four_way_type) {
        (PartitionType::Horz, 0) => PartitionType::Horz4A,
        (PartitionType::Horz, _) => PartitionType::Horz4B,
        (PartitionType::Vert, 0) => PartitionType::Vert4A,
        (PartitionType::Vert, _) => PartitionType::Vert4B,
        _ => base_type,
    }
}

fn implied_boundary_partition(
    x: usize, y: usize, w: usize, h: usize,
    frame_w: usize, frame_h: usize,
) -> Option<PartitionType> {
    let has_rows = (y + h / 2) < frame_h;
    let has_cols = (x + w / 2) < frame_w;

    if has_rows && has_cols {
        return None;
    }

    if w == h {
        if has_rows && !has_cols {
            Some(PartitionType::Vert)
        } else {
            Some(PartitionType::Horz)
        }
    } else if h > w {
        if !has_rows {
            Some(PartitionType::Horz)
        } else if !has_cols && (w < 16 || (x + w / 4) >= frame_w) {
            Some(PartitionType::Horz)
        } else {
            None
        }
    } else {
        if !has_cols {
            Some(PartitionType::Vert)
        } else if !has_rows && (h < 16 || (y + h / 4) >= frame_h) {
            Some(PartitionType::Vert)
        } else {
            None
        }
    }
}

fn decode_partition(
    ec: &mut DaalaDecoder,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    frame_w: usize,
    frame_h: usize,
    tree_type: TreeType,
    cfg: &TileDecodeConfig,
    cdf: &mut FrameCdfContext,
    mi: &mut ModeInfoGrid,
    blocks: &mut Vec<DecodedBlock>,
    state: &mut TileState,
) {
    if x >= frame_w || y >= frame_h {
        return;
    }

    if cfg.enable_sdp && w == 64 && h == 64 && tree_type == TreeType::Shared {
        decode_partition(ec, x, y, w, h, frame_w, frame_h, TreeType::Luma, cfg, cdf, mi, blocks, state);
        decode_partition(ec, x, y, w, h, frame_w, frame_h, TreeType::Chroma, cfg, cdf, mi, blocks, state);
        return;
    }

    if !is_partition_point(w, h) {
        let cw = w.min(frame_w - x);
        let ch = h.min(frame_h - y);
        decode_leaf_block(ec, x, y, cw, ch, tree_type, cfg, cdf, mi, blocks, state);
        return;
    }

    let partition = if let Some(implied) = implied_boundary_partition(x, y, w, h, frame_w, frame_h) {
        implied
    } else {
        read_partition_type(ec, w, h, cdf)
    };

    match partition {
        PartitionType::None => {
            decode_leaf_block(ec, x, y, w, h, tree_type, cfg, cdf, mi, blocks, state);
        }
        PartitionType::Split => {
            let hw = w / 2;
            let hh = h / 2;
            decode_partition(ec, x, y, hw, hh, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
            decode_partition(ec, x + hw, y, hw, hh, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
            decode_partition(ec, x, y + hh, hw, hh, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
            decode_partition(ec, x + hw, y + hh, hw, hh, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
        }
        PartitionType::Horz => {
            let hh = h / 2;
            decode_partition(ec, x, y, w, hh, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
            decode_partition(ec, x, y + hh, w, hh, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
        }
        PartitionType::Vert => {
            let hw = w / 2;
            decode_partition(ec, x, y, hw, h, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
            decode_partition(ec, x + hw, y, hw, h, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
        }
        _ => {
            let subs = subblock_offsets(partition, w, h);
            for (dx, dy, sw, sh) in subs {
                if sw > 0 && sh > 0 {
                    decode_partition(ec, x + dx, y + dy, sw, sh, frame_w, frame_h, tree_type, cfg, cdf, mi, blocks, state);
                }
            }
        }
    }
}

fn read_tx_partition(
    ec: &mut DaalaDecoder,
    block_w: usize,
    block_h: usize,
    fsc_mode: usize,
    reduced_tx_part_set: bool,
    cdf: &mut FrameCdfContext,
) -> TxPartitionType {
    if coding_block_disallows_tx_partitioning(block_w, block_h) {
        return TxPartitionType::None;
    }

    let max_tx_w = block_w.min(64);
    let max_tx_h = block_h.min(64);

    let allow_horz = {
        let sub_h = max_tx_h >> 1;
        sub_h >= 4 && is_valid_tx_size(max_tx_w, sub_h)
    };
    let allow_vert = {
        let sub_w = max_tx_w >> 1;
        sub_w >= 4 && is_valid_tx_size(sub_w, max_tx_h)
    };

    if !allow_horz && !allow_vert {
        return TxPartitionType::None;
    }

    let bsize_group = tx_part_group(block_w, block_h);
    if bsize_group >= TXFM_SPLIT_GROUP {
        return TxPartitionType::None;
    }

    let is_fsc = if fsc_mode != 0 { 1 } else { 0 };
    let is_inter = 0;

    let do_partition = ec.decode_symbol_mut(&mut cdf.txfm_do_partition[is_fsc][is_inter][bsize_group]);
    ec.trace_state(&format!("after_txfm_do_partition={} group={} bsize={}x{}", do_partition, bsize_group, block_w, block_h));
    if do_partition == 0 {
        return TxPartitionType::None;
    }

    if allow_horz && allow_vert {
        let txsize_group = tx_type_group_vert_and_horz(block_w, block_h);
        if txsize_group < 255 {
            let partition_type = ec.decode_symbol_mut(
                &mut cdf.txfm_4way_partition_type[is_fsc][is_inter][txsize_group],
            );
            ec.trace_state(&format!("after_txfm_4way_type={} tsg={}", partition_type, txsize_group));
            match partition_type {
                0 => TxPartitionType::Split,
                1 => TxPartitionType::Horz,
                2 => TxPartitionType::Vert,
                3 => TxPartitionType::Horz4,
                4 => TxPartitionType::Vert4,
                _ => TxPartitionType::Split,
            }
        } else {
            TxPartitionType::Split
        }
    } else {
        let txsize_group = tx_type_group_vert_or_horz(block_w, block_h);
        if txsize_group > 0 && !reduced_tx_part_set {
            let partition_type = ec.decode_symbol_mut(
                &mut cdf.txfm_2or3_way_partition_type[is_fsc][is_inter][txsize_group - 1],
            );
            ec.trace_state(&format!("after_txfm_2or3way_type={} tsg={}", partition_type, txsize_group));
            if allow_horz {
                if partition_type == 0 { TxPartitionType::Horz } else { TxPartitionType::Horz4 }
            } else {
                if partition_type == 0 { TxPartitionType::Vert } else { TxPartitionType::Vert4 }
            }
        } else {
            if allow_horz { TxPartitionType::Horz } else { TxPartitionType::Vert }
        }
    }
}

fn get_tx_partition_sub_tus(
    partition: TxPartitionType,
    max_tx_w: usize,
    max_tx_h: usize,
) -> Vec<(TxSize, usize, usize)> {
    match partition {
        TxPartitionType::None => {
            vec![(crate::block::max_txsize_rect_for_block(max_tx_w, max_tx_h), 0, 0)]
        }
        TxPartitionType::Split => {
            let hw = max_tx_w / 2;
            let hh = max_tx_h / 2;
            let sub = crate::block::max_txsize_rect_for_block(hw, hh);
            vec![(sub, 0, 0), (sub, 0, hw), (sub, hh, 0), (sub, hh, hw)]
        }
        TxPartitionType::Horz => {
            let hh = max_tx_h / 2;
            let sub = crate::block::max_txsize_rect_for_block(max_tx_w, hh);
            vec![(sub, 0, 0), (sub, hh, 0)]
        }
        TxPartitionType::Vert => {
            let hw = max_tx_w / 2;
            let sub = crate::block::max_txsize_rect_for_block(hw, max_tx_h);
            vec![(sub, 0, 0), (sub, 0, hw)]
        }
        TxPartitionType::Horz4 => {
            let qh = max_tx_h / 4;
            let sub = crate::block::max_txsize_rect_for_block(max_tx_w, qh);
            vec![(sub, 0, 0), (sub, qh, 0), (sub, 2 * qh, 0), (sub, 3 * qh, 0)]
        }
        TxPartitionType::Vert4 => {
            let qw = max_tx_w / 4;
            let sub = crate::block::max_txsize_rect_for_block(qw, max_tx_h);
            vec![(sub, 0, 0), (sub, 0, qw), (sub, 0, 2 * qw), (sub, 0, 3 * qw)]
        }
    }
}

const TXFM_SPLIT_GROUP: usize = 9;

fn coding_block_disallows_tx_partitioning(w: usize, h: usize) -> bool {
    (w >= 64 && h >= 128) || (w >= 128 && h >= 64)
}

fn is_valid_tx_size(w: usize, h: usize) -> bool {
    matches!(
        (w, h),
        (4, 4)
            | (8, 8)
            | (16, 16)
            | (32, 32)
            | (64, 64)
            | (4, 8)
            | (8, 4)
            | (8, 16)
            | (16, 8)
            | (16, 32)
            | (32, 16)
            | (32, 64)
            | (64, 32)
            | (4, 16)
            | (16, 4)
            | (8, 32)
            | (32, 8)
            | (16, 64)
            | (64, 16)
    )
}

fn tx_part_group(w: usize, h: usize) -> usize {
    match (w, h) {
        (4, 4) | (4, 8) | (8, 4) => 0,
        (8, 8) => 1,
        (8, 16) | (16, 8) => 2,
        (16, 16) => 3,
        (16, 32) | (32, 16) => 4,
        (32, 32) => 5,
        (32, 64) | (64, 32) => 6,
        (64, 64) | (64, 128) | (128, 64) | (128, 128) => 7,
        (4, 16) | (16, 4) | (8, 32) | (32, 8) | (16, 64) | (64, 16) | (4, 32) | (32, 4) | (8, 64) | (64, 8) => 8,
        _ => TXFM_SPLIT_GROUP,
    }
}

fn tx_type_group_vert_and_horz(w: usize, h: usize) -> usize {
    match (w, h) {
        (8, 8) => 0,
        (8, 16) => 1,
        (16, 8) => 2,
        (16, 16) => 3,
        (16, 32) => 4,
        (32, 16) => 5,
        (32, 32) => 6,
        (32, 64) => 7,
        (64, 32) => 8,
        (64, 64) | (64, 128) | (128, 64) | (128, 128) => 9,
        (8, 32) | (8, 64) => 10,
        (32, 8) | (64, 8) => 11,
        (16, 64) => 12,
        (64, 16) => 13,
        _ => 255,
    }
}

fn tx_type_group_vert_or_horz(w: usize, h: usize) -> usize {
    match (w, h) {
        (4, 4) | (4, 8) | (8, 4) => 0,
        (4, 16) | (4, 32) => 1,
        (16, 4) | (32, 4) => 2,
        _ => 0,
    }
}

fn allow_fsc_intra(block_w: usize, block_h: usize) -> bool {
    block_w >= 4 && block_h >= 4 && block_w <= 32 && block_h <= 32
}

fn fsc_bsize_group(w: usize, h: usize) -> usize {
    match (w, h) {
        (4, 4) => 0,
        (4, 8) | (8, 4) => 1,
        (8, 8) => 2,
        (8, 16) | (16, 8) | (4, 16) | (16, 4) => 3,
        (16, 16) | (8, 32) | (32, 8) | (4, 32) | (32, 4) => 4,
        (16, 32) | (32, 16) | (32, 32) => 5,
        _ => unreachable!("unexpected block size for FSC: {}x{}", w, h),
    }
}

pub fn decode_tile_data_partitioned(
    data: &[u8],
    width: usize,
    height: usize,
    sb_size: usize,
) -> Vec<DecodedBlock> {
    decode_tile_data_partitioned_opts(data, width, height, sb_size, false, 0)
}

pub fn decode_tile_data_partitioned_opts(
    data: &[u8],
    width: usize,
    height: usize,
    sb_size: usize,
    enable_cfl_intra: bool,
    base_qindex: u8,
) -> Vec<DecodedBlock> {
    decode_tile_data_partitioned_cfg(data, width, height, sb_size, base_qindex, &TileDecodeConfig {
        enable_cfl_intra,
        ..TileDecodeConfig::default()
    })
}

pub fn decode_tile_data_partitioned_cfg(
    data: &[u8],
    width: usize,
    height: usize,
    sb_size: usize,
    base_qindex: u8,
    cfg: &TileDecodeConfig,
) -> Vec<DecodedBlock> {
    let mut ec = DaalaDecoder::new(data);
    let mut cdf = FrameCdfContext::new_with_qindex(base_qindex);
    let mut mi = ModeInfoGrid::new(width, height);
    let mut blocks = Vec::new();
    let mut state = TileState {
        cdef_read: HashSet::new(),
        ccso_read: [HashSet::new(), HashSet::new(), HashSet::new()],
    };

    let sb_cols = (width + sb_size - 1) / sb_size;
    let sb_rows = (height + sb_size - 1) / sb_size;

    for sb_row in 0..sb_rows {
        for sb_col in 0..sb_cols {
            let x = sb_col * sb_size;
            let y = sb_row * sb_size;
            decode_partition(
                &mut ec, x, y, sb_size, sb_size, width, height,
                TreeType::Shared, cfg, &mut cdf, &mut mi, &mut blocks, &mut state,
            );
        }
    }

    blocks
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
