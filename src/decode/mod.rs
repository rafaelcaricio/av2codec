pub mod bitreader;
pub mod entropy;
pub mod frame;
pub mod obu;
pub mod predict;
pub mod sequence;
pub mod tile;
pub mod transform;

use std::fmt;

#[derive(Debug)]
pub enum DecodeError {
    InvalidObu,
    UnsupportedProfile,
    UnsupportedFeature(&'static str),
    BitstreamCorrupt,
    UnexpectedEof,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::InvalidObu => write!(f, "invalid OBU"),
            DecodeError::UnsupportedProfile => write!(f, "unsupported profile"),
            DecodeError::UnsupportedFeature(feat) => write!(f, "unsupported feature: {}", feat),
            DecodeError::BitstreamCorrupt => write!(f, "bitstream corrupt"),
            DecodeError::UnexpectedEof => write!(f, "unexpected end of data"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn trace_block_match(tag: &str, x: usize, y: usize) -> bool {
    match std::env::var(tag) {
        Ok(spec) => {
            let mut parts = spec.split(',');
            let sx = parts.next().and_then(|v| v.trim().parse::<usize>().ok());
            let sy = parts.next().and_then(|v| v.trim().parse::<usize>().ok());
            sx == Some(x) && sy == Some(y)
        }
        Err(_) => false,
    }
}

pub struct DecodedFrame {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub frame_number: u64,
}

pub struct Decoder;

impl Decoder {
    pub fn decode_all(data: &[u8]) -> Result<Vec<DecodedFrame>, DecodeError> {
        let obus = obu::parse_obus(data);
        if obus.is_empty() {
            return Err(DecodeError::InvalidObu);
        }

        let seq = sequence::parse_sequence_header(obus[0].payload)?;
        let mut frames = Vec::new();
        let mut frame_number = 0u64;

        let sb_aligned = seq.sb_size == 64
            && seq.width % 64 == 0
            && seq.height % 64 == 0
            && seq.width > 0
            && seq.height > 0;

        for raw_obu in &obus[1..] {
            if raw_obu.obu_type != 4 {
                continue;
            }
            let fp = frame::parse_frame_header(raw_obu.payload, &seq)?;
            let dequant = crate::encode::ac_qlookup(fp.base_qindex);
            let uv_dequant = crate::encode::ac_qlookup(
                (fp.base_qindex as i32 + fp.u_ac_delta_q).clamp(0, 255) as u8,
            );
            let sb_cols = (seq.width / 64) as usize;
            let sb_rows = (seq.height / 64) as usize;

            let tile_cfg = tile::TileDecodeConfig {
                enable_cfl_intra: seq.enable_cfl_intra,
                enable_mhccp: seq.enable_mhccp,
                enable_fsc: seq.enable_fsc,
                enable_idtx_intra: seq.enable_idtx_intra,
                enable_mrls: seq.enable_mrls,
                enable_ist: seq.enable_ist,
                enable_cctx: seq.enable_cctx,
                tx_mode_select: fp.tx_mode_select,
                reduced_tx_set_used: fp.reduced_tx_set_used,
                reduced_tx_part_set: seq.reduced_tx_part_set,
                allow_intrabc: fp.allow_intrabc,
                allow_screen_content_tools: fp.allow_screen_content_tools,
                enable_sdp: seq.enable_sdp,
                cdef_frame_enable: fp.cdef_frame_enable,
                nb_cdef_strengths: fp.nb_cdef_strengths,
                ccso_frame_enable: fp.ccso_frame_enable,
                ccso_enable: fp.ccso_enable,
                sb_size: seq.sb_size as usize,
                tcq_mode: fp.tcq_mode,
                allow_parity_hiding: fp.allow_parity_hiding,
                enable_ext_partitions: seq.enable_ext_partitions,
                enable_uneven_4way_partitions: seq.enable_uneven_4way_partitions,
                enable_refmvbank: seq.enable_refmvbank,
                max_bvp_drl_bits: fp.max_bvp_drl_bits,
                cur_frame_force_integer_mv: fp.cur_frame_force_integer_mv,
                allow_intrabc_morph_pred: seq.enable_intra_bawp && fp.allow_screen_content_tools,
            };

            let (y_plane, u_plane, v_plane) = if !fp.tile_data.is_empty() {
                let use_partitioned =
                    !sb_aligned || fp.tx_mode_select || seq.enable_sdp || seq.enable_ibp;
                if !use_partitioned {
                    let legacy_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let decoded_sbs = tile::decode_tile_data_multi_sb(
                                fp.tile_data,
                                sb_cols,
                                sb_rows,
                                !fp.disable_cdf_update,
                            );
                            reconstruct_frame(
                                &decoded_sbs,
                                sb_cols,
                                sb_rows,
                                seq.width,
                                seq.height,
                                dequant,
                            )
                        }));
                    match legacy_result {
                        Ok(planes) => planes,
                        Err(_) => {
                            let partitioned_result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    let blocks = tile::decode_tile_data_partitioned_cfg(
                                        fp.tile_data,
                                        seq.width as usize,
                                        seq.height as usize,
                                        seq.sb_size as usize,
                                        fp.base_qindex,
                                        &tile_cfg,
                                    );
                                    reconstruct_partitioned_frame(
                                        &blocks,
                                        seq.width,
                                        seq.height,
                                        dequant,
                                        uv_dequant,
                                        fp.tcq_mode,
                                        seq.sb_size as usize,
                                        seq.enable_ibp,
                                    )
                                }));
                            partitioned_result.unwrap_or_else(|_| gray_frame(seq.width, seq.height))
                        }
                    }
                } else {
                    let blocks = tile::decode_tile_data_partitioned_cfg(
                        fp.tile_data,
                        seq.width as usize,
                        seq.height as usize,
                        seq.sb_size as usize,
                        fp.base_qindex,
                        &tile_cfg,
                    );
                    reconstruct_partitioned_frame(
                        &blocks,
                        seq.width,
                        seq.height,
                        dequant,
                        uv_dequant,
                        fp.tcq_mode,
                        seq.sb_size as usize,
                        seq.enable_ibp,
                    )
                }
            } else {
                gray_frame(seq.width, seq.height)
            };

            frames.push(DecodedFrame {
                y: y_plane,
                u: u_plane,
                v: v_plane,
                width: seq.width,
                height: seq.height,
                frame_number,
            });
            frame_number += 1;
        }
        Ok(frames)
    }
}

fn round_power_of_two_signed(value: i32, n: u32) -> i32 {
    if value < 0 {
        -((-value + (1 << (n - 1))) >> n)
    } else {
        (value + (1 << (n - 1))) >> n
    }
}

fn predict_dc_rect(recon: &[u8], stride: usize, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    predict::predict_intra(
        recon,
        stride,
        x,
        y,
        w,
        h,
        predict::PredictionMode::DcPred,
        0,
        0,
        0,
    )
}

fn subsampled_luma_q3(
    y_plane: &[u8],
    y_w: usize,
    y_h: usize,
    uv_x: usize,
    uv_y: usize,
) -> Option<i64> {
    let lx = uv_x * 2;
    let ly = uv_y * 2;
    if lx + 1 >= y_w || ly + 1 >= y_h {
        return None;
    }
    let sum = y_plane[ly * y_w + lx] as i64
        + y_plane[ly * y_w + lx + 1] as i64
        + y_plane[(ly + 1) * y_w + lx] as i64
        + y_plane[(ly + 1) * y_w + lx + 1] as i64;
    Some(sum * 2)
}

fn cfl_neighbor_avg_q3(
    y_plane: &[u8],
    y_w: usize,
    y_h: usize,
    uv_w: usize,
    uv_h: usize,
    uv_bx: usize,
    uv_by: usize,
    uv_bw: usize,
    uv_bh: usize,
) -> i32 {
    let mut sum = 0i64;
    let mut count = 0i64;

    if uv_by > 0 {
        let sy = uv_by - 1;
        for dx in 0..uv_bw {
            let sx = (uv_bx + dx).min(uv_w.saturating_sub(1));
            if let Some(v) = subsampled_luma_q3(y_plane, y_w, y_h, sx, sy) {
                sum += v;
                count += 1;
            }
        }
    }

    if uv_bx > 0 {
        let sx = uv_bx - 1;
        for dy in 0..uv_bh {
            let sy = (uv_by + dy).min(uv_h.saturating_sub(1));
            if let Some(v) = subsampled_luma_q3(y_plane, y_w, y_h, sx, sy) {
                sum += v;
                count += 1;
            }
        }
    }

    if count == 0 {
        128 * 8
    } else {
        ((sum + (count >> 1)) / count) as i32
    }
}

fn div_round_signed(num: i64, den: i64) -> i64 {
    if den == 0 {
        return 0;
    }
    let sign = if (num < 0) ^ (den < 0) { -1 } else { 1 };
    let a = num.abs();
    let b = den.abs();
    let q = (a + (b >> 1)) / b;
    sign * q
}

fn derive_cfl_alpha_q3_neighbors(
    y_plane: &[u8],
    y_w: usize,
    y_h: usize,
    chroma_plane: &[u8],
    uv_w: usize,
    uv_h: usize,
    uv_bx: usize,
    uv_by: usize,
    uv_bw: usize,
    uv_bh: usize,
) -> i32 {
    let mut n = 0i64;
    let mut sum_x = 0i64;
    let mut sum_y = 0i64;
    let mut sum_xx = 0i64;
    let mut sum_xy = 0i64;
    let trace = trace_block_match("WAV2C_TRACE_CFL", uv_bx, uv_by);
    let mut samples: Vec<(usize, usize, i64, i64)> = Vec::new();

    let mut add_sample = |sx: usize, sy: usize| {
        if sx >= uv_w || sy >= uv_h {
            return;
        }
        if let Some(x_q3) = subsampled_luma_q3(y_plane, y_w, y_h, sx, sy) {
            let yv = chroma_plane[sy * uv_w + sx] as i64;
            if trace {
                samples.push((sx, sy, x_q3, yv));
            }
            n += 1;
            sum_x += x_q3;
            sum_y += yv;
            sum_xx += x_q3 * x_q3;
            sum_xy += x_q3 * yv;
        }
    };

    if uv_by > 0 {
        let sy = uv_by - 1;
        for dx in 0..uv_bw {
            add_sample(uv_bx + dx, sy);
        }
    }
    if uv_bx > 0 {
        let sx = uv_bx - 1;
        for dy in 0..uv_bh {
            add_sample(sx, uv_by + dy);
        }
    }

    if n < 2 {
        return 0;
    }

    let cov_num = n * sum_xy - sum_x * sum_y;
    let var_num = n * sum_xx - sum_x * sum_x;
    if var_num == 0 {
        if trace {
            eprintln!(
                "[WAV2C_CFL_DERIVE] uv=({}, {}) size={}x{} n={} sum_x={} sum_y={} sum_xx={} sum_xy={} cov={} var={} alpha=0",
                uv_bx, uv_by, uv_bw, uv_bh, n, sum_x, sum_y, sum_xx, sum_xy, cov_num, var_num
            );
            for (sx, sy, x_q3, yv) in samples.iter() {
                eprintln!(
                    "[WAV2C_CFL_SAMPLE] s=({}, {}) x_q3={} y={}",
                    sx, sy, x_q3, yv
                );
            }
        }
        return 0;
    }

    let alpha_q3 = div_round_signed(cov_num << 11, var_num).clamp(-1024, 1024);
    if trace {
        eprintln!(
            "[WAV2C_CFL_DERIVE] uv=({}, {}) size={}x{} n={} sum_x={} sum_y={} sum_xx={} sum_xy={} cov={} var={} alpha={}",
            uv_bx, uv_by, uv_bw, uv_bh, n, sum_x, sum_y, sum_xx, sum_xy, cov_num, var_num, alpha_q3
        );
        for (sx, sy, x_q3, yv) in samples.iter() {
            eprintln!(
                "[WAV2C_CFL_SAMPLE] s=({}, {}) x_q3={} y={}",
                sx, sy, x_q3, yv
            );
        }
    }
    alpha_q3 as i32
}

fn luma_center_q0(y_plane: &[u8], y_w: usize, y_h: usize, uv_x: usize, uv_y: usize) -> Option<f64> {
    let lx = uv_x * 2;
    let ly = uv_y * 2;
    if lx + 1 >= y_w || ly + 1 >= y_h {
        return None;
    }
    let sum = y_plane[ly * y_w + lx] as f64
        + y_plane[ly * y_w + lx + 1] as f64
        + y_plane[(ly + 1) * y_w + lx] as f64
        + y_plane[(ly + 1) * y_w + lx + 1] as f64;
    Some(sum * 0.25)
}

fn solve_3x3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for i in 0..3 {
        let mut pivot = i;
        let mut best = a[i][i].abs();
        for r in (i + 1)..3 {
            if a[r][i].abs() > best {
                best = a[r][i].abs();
                pivot = r;
            }
        }
        if best < 1e-9 {
            return None;
        }
        if pivot != i {
            a.swap(i, pivot);
            b.swap(i, pivot);
        }
        let diag = a[i][i];
        for c in i..3 {
            a[i][c] /= diag;
        }
        b[i] /= diag;
        for r in 0..3 {
            if r == i {
                continue;
            }
            let f = a[r][i];
            if f.abs() < 1e-12 {
                continue;
            }
            for c in i..3 {
                a[r][c] -= f * a[i][c];
            }
            b[r] -= f * b[i];
        }
    }
    Some(b)
}

fn derive_mhccp_params_neighbors(
    y_plane: &[u8],
    y_w: usize,
    y_h: usize,
    chroma_plane: &[u8],
    uv_w: usize,
    uv_h: usize,
    uv_bx: usize,
    uv_by: usize,
    uv_bw: usize,
    uv_bh: usize,
    mh_dir: u8,
    plane_name: char,
) -> Option<[f64; 3]> {
    let mut ata = [[0.0f64; 3]; 3];
    let mut aty = [0.0f64; 3];
    let mut count = 0usize;
    let trace = trace_block_match("WAV2C_TRACE_MHCCP", uv_bx, uv_by);
    let mut samples: Vec<(usize, usize, f64, f64, f64, f64, f64)> = Vec::new();

    let mut add_sample = |sx: usize, sy: usize| {
        if sx >= uv_w || sy >= uv_h {
            return;
        }
        let center = match luma_center_q0(y_plane, y_w, y_h, sx, sy) {
            Some(v) => v,
            None => return,
        };
        let top = if sy > 0 {
            luma_center_q0(y_plane, y_w, y_h, sx, sy - 1).unwrap_or(center)
        } else {
            center
        };
        let left = if sx > 0 {
            luma_center_q0(y_plane, y_w, y_h, sx - 1, sy).unwrap_or(center)
        } else {
            center
        };

        let base = match mh_dir {
            1 => top,
            2 => left,
            _ => center,
        };
        let feats = [base, (center * center + 128.0) / 256.0, 128.0];
        let yv = chroma_plane[sy * uv_w + sx] as f64;
        if trace {
            samples.push((sx, sy, center, top, left, feats[1], yv));
        }

        for i in 0..3 {
            aty[i] += feats[i] * yv;
            for j in i..3 {
                ata[i][j] += feats[i] * feats[j];
            }
        }
        count += 1;
    };

    if uv_by > 0 {
        let sy = uv_by - 1;
        for dx in 0..uv_bw {
            add_sample(uv_bx + dx, sy);
        }
    }
    if uv_bx > 0 {
        let sx = uv_bx - 1;
        for dy in 0..uv_bh {
            add_sample(sx, uv_by + dy);
        }
    }

    if count < 3 {
        return None;
    }

    for i in 0..3 {
        for j in 0..i {
            ata[i][j] = ata[j][i];
        }
    }
    let ridge = 1024.0;
    ata[0][0] += ridge;
    ata[1][1] += ridge;
    ata[2][2] += ridge;

    let mut params = solve_3x3(ata, aty)?;
    params[0] = params[0].clamp(-2.0, 2.0);
    params[1] = params[1].clamp(-2.0, 2.0);
    params[2] = params[2].clamp(-1.0, 2.0);
    if trace {
        eprintln!(
            "[WAV2C_MHCCP_DERIVE] plane={} uv=({}, {}) size={}x{} dir={} count={} params={:?}",
            plane_name, uv_bx, uv_by, uv_bw, uv_bh, mh_dir, count, params
        );
        eprintln!(
            "[WAV2C_MHCCP_DERIVE] ata={:?} aty={:?}",
            ata, aty
        );
        for (sx, sy, center, top, left, nonlinear, yv) in samples.iter() {
            eprintln!(
                "[WAV2C_MHCCP_SAMPLE] plane={} s=({}, {}) c={:.3} t={:.3} l={:.3} nl={:.3} y={:.3}",
                plane_name, sx, sy, center, top, left, nonlinear, yv
            );
        }
    }
    Some(params)
}

fn mhccp_predict_block(
    y_plane: &[u8],
    y_w: usize,
    y_h: usize,
    uv_w: usize,
    uv_h: usize,
    uv_bx: usize,
    uv_by: usize,
    uv_bw: usize,
    uv_bh: usize,
    mh_dir: u8,
    params: [f64; 3],
    plane_name: char,
) -> Vec<u8> {
    let mut pred = vec![128u8; uv_bw * uv_bh];
    let trace = trace_block_match("WAV2C_TRACE_MHCCP", uv_bx, uv_by);
    for r in 0..uv_bh {
        for c in 0..uv_bw {
            let sx = uv_bx + c;
            let sy = uv_by + r;
            if sx >= uv_w || sy >= uv_h {
                continue;
            }
            let center = match luma_center_q0(y_plane, y_w, y_h, sx, sy) {
                Some(v) => v,
                None => continue,
            };
            let top = if sy > 0 {
                luma_center_q0(y_plane, y_w, y_h, sx, sy - 1).unwrap_or(center)
            } else {
                center
            };
            let left = if sx > 0 {
                luma_center_q0(y_plane, y_w, y_h, sx - 1, sy).unwrap_or(center)
            } else {
                center
            };
            let base = match mh_dir {
                1 => top,
                2 => left,
                _ => center,
            };
            let feat1 = (center * center + 128.0) / 256.0;
            let value = params[0] * base + params[1] * feat1 + params[2] * 128.0;
            pred[r * uv_bw + c] = value.round().clamp(0.0, 255.0) as u8;
        }
    }
    if trace {
        eprintln!(
            "[WAV2C_MHCCP_PRED] plane={} uv=({}, {}) size={}x{} dir={} params={:?} pred={:?}",
            plane_name, uv_bx, uv_by, uv_bw, uv_bh, mh_dir, params, pred
        );
    }
    pred
}

fn reconstruct_tu(
    recon: &mut [u8],
    stride: usize,
    tu_x: usize,
    tu_y: usize,
    tu_w: usize,
    tu_h: usize,
    pred: &[u8],
    pred_w: usize,
    pred_x_offset: usize,
    pred_y_offset: usize,
    residuals: &[i32],
) {
    for row in 0..tu_h {
        for col in 0..tu_w {
            let pred_val = pred[(pred_y_offset + row) * pred_w + pred_x_offset + col] as i32;
            let res_val = residuals[row * tu_w + col];
            let pixel = (pred_val + res_val).clamp(0, 255) as u8;
            recon[(tu_y + row) * stride + tu_x + col] = pixel;
        }
    }
}

const BLK_SIZE_LOG2_BAWP: [usize; 17] = [0, 0, 0, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4];
const LOG_TO_BLK_SIZE_BAWP: [usize; 5] = [0, 2, 4, 8, 16];

fn derive_number_ref_samples_bawp(
    above_valid: bool,
    left_valid: bool,
    width: usize,
    height: usize,
) -> (usize, usize) {
    let above_available = if width != 0 { above_valid } else { false };
    let left_available = if height != 0 { left_valid } else { false };

    if above_available && left_available {
        if width == 16 && height == 16 {
            (16, 16)
        } else if width > 4 && height > 4 {
            (8, 8)
        } else if width < 16 && height < 16 {
            (4, 4)
        } else if width == 16 {
            (16, 0)
        } else {
            (0, 16)
        }
    } else if above_available {
        (width, 0)
    } else if left_available {
        (0, height)
    } else {
        (0, 0)
    }
}

fn derive_linear_parameters_alpha(
    sum_x: i32,
    sum_y: i32,
    sum_xx: i32,
    sum_xy: i32,
    count: i32,
    shift: i32,
) -> i32 {
    if count == 0 {
        return 0;
    }
    let der = sum_xx as i64 - ((sum_x as i64 * sum_x as i64) / count as i64);
    let nor = sum_xy as i64 - ((sum_x as i64 * sum_y as i64) / count as i64);
    if der == 0 || nor == 0 {
        return 0;
    }
    let mut alpha = (nor << shift) / der;
    let alpha_max = ((2i64) << shift) - 1;
    if alpha > alpha_max {
        alpha = alpha_max;
    }
    if alpha < -alpha_max {
        alpha = -alpha_max;
    }
    alpha as i32
}

fn derive_linear_parameters_beta(
    sum_x: i32,
    sum_y: i32,
    count: i32,
    shift: i32,
    alpha: i32,
) -> i32 {
    if count == 0 {
        return 0;
    }
    ((((sum_y as i64) << shift) - (sum_x as i64 * alpha as i64)) / count as i64) as i32
}

fn apply_intrabc_morph_pred_luma(
    plane: &[u8],
    stride: usize,
    frame_w: usize,
    frame_h: usize,
    bx: usize,
    by: usize,
    bw: usize,
    bh: usize,
    src_x: i32,
    src_y: i32,
    pred: &mut [u8],
) {
    if src_x < 0 || src_y < 0 || bx >= frame_w || by >= frame_h {
        return;
    }
    let sx = src_x as usize;
    let sy = src_y as usize;
    if sx >= frame_w || sy >= frame_h {
        return;
    }

    let mut ref_w = bw.min(frame_w - bx);
    let mut ref_h = bh.min(frame_h - by);
    ref_w = ref_w.min(16);
    ref_h = ref_h.min(16);
    if ref_w == 0 || ref_h == 0 {
        return;
    }

    let width = LOG_TO_BLK_SIZE_BAWP[BLK_SIZE_LOG2_BAWP[ref_w]];
    let height = LOG_TO_BLK_SIZE_BAWP[BLK_SIZE_LOG2_BAWP[ref_h]];
    let (numb_up, numb_left) = derive_number_ref_samples_bawp(by > 0, bx > 0, width, height);

    let mut count = 0i32;
    let mut sum_x = 0i32;
    let mut sum_y = 0i32;
    let mut sum_xy = 0i32;
    let mut sum_xx = 0i32;

    let mut ref_pad = [0i32; 16];
    let mut recon_pad = [0i32; 16];

    if numb_up > 0 {
        if by == 0 || sy == 0 || bx + ref_w > frame_w || sx + ref_w > frame_w {
            return;
        }
        for i in 0..ref_w {
            ref_pad[i] = plane[(sy - 1) * stride + sx + i] as i32;
            recon_pad[i] = plane[(by - 1) * stride + bx + i] as i32;
        }
        let delta_w = width - ref_w;
        for i in 0..delta_w {
            ref_pad[i + ref_w] = ref_pad[i];
            recon_pad[i + ref_w] = recon_pad[i];
        }
        let step = width / numb_up;
        let start = if step == 1 { 0 } else { step >> 1 };
        let mut i = start;
        while i < width {
            sum_x += ref_pad[i];
            sum_y += recon_pad[i];
            sum_xy += ref_pad[i] * recon_pad[i];
            sum_xx += ref_pad[i] * ref_pad[i];
            i += step;
        }
        count += numb_up as i32;
    }

    if numb_left > 0 {
        if bx == 0 || sx == 0 || by + ref_h > frame_h || sy + ref_h > frame_h {
            return;
        }
        for i in 0..ref_h {
            ref_pad[i] = plane[(sy + i) * stride + sx - 1] as i32;
            recon_pad[i] = plane[(by + i) * stride + bx - 1] as i32;
        }
        let delta_h = height - ref_h;
        for i in 0..delta_h {
            ref_pad[i + ref_h] = ref_pad[i];
            recon_pad[i + ref_h] = recon_pad[i];
        }
        let step_left = height / numb_left;
        let start_left = if step_left == 1 { 0 } else { step_left >> 1 };
        let mut i = start_left;
        while i < height {
            sum_x += ref_pad[i];
            sum_y += recon_pad[i];
            sum_xy += ref_pad[i] * recon_pad[i];
            sum_xx += ref_pad[i] * ref_pad[i];
            i += step_left;
        }
        count += numb_left as i32;
    }

    let shift = 8i32;
    let (alpha, beta) = if count > 0 {
        let mut alpha = derive_linear_parameters_alpha(sum_x, sum_y, sum_xx, sum_xy, count, shift);
        if alpha == 0 {
            alpha = 1 << shift;
        }
        let beta = derive_linear_parameters_beta(sum_x, sum_y, count, shift, alpha);
        (alpha, beta)
    } else {
        (1 << shift, -(1 << (shift - 1)))
    };

    for j in 0..bh {
        for i in 0..bw {
            let idx = j * bw + i;
            let v = ((pred[idx] as i32 * alpha + beta) >> shift).clamp(0, 255) as u8;
            pred[idx] = v;
        }
    }
}

fn reconstruct_partitioned_frame(
    blocks: &[tile::DecodedBlock],
    width: u32,
    height: u32,
    dequant: i32,
    uv_dequant: i32,
    tcq_mode: bool,
    sb_size: usize,
    enable_ibp: bool,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    let uv_w = w / 2;
    let uv_h = h / 2;
    let mut y_plane = vec![128u8; w * h];
    let mut u_plane = vec![128u8; uv_w * uv_h];
    let mut v_plane = vec![128u8; uv_w * uv_h];

    let mi_cols = (w + 3) / 4;
    let mi_rows = (h + 3) / 4;
    let mut joint_grid = vec![0u8; mi_cols * mi_rows];

    for block in blocks {
        let bx = block.x;
        let by = block.y;
        let bw = block.width;
        let bh = block.height;

        if block.tree_type != tile::TreeType::Chroma {
            let mi_x = bx / 4;
            let mi_y = by / 4;
            let mi_w = bw / 4;
            let mi_h = bh / 4;

            let joint = block.joint_y_mode;
            let (pred_mode, angle_delta) = predict::decode_prediction_mode(joint);

            for r in 0..mi_h {
                for c in 0..mi_w {
                    joint_grid[(mi_y + r) * mi_cols + mi_x + c] = joint;
                }
            }

            if block.is_intrabc {
                let src_x = bx as i32 + block.bv_col / 8;
                let src_y = by as i32 + block.bv_row / 8;
                let mut y_pred = vec![128u8; bw * bh];
                for r in 0..bh {
                    for c in 0..bw {
                        let sy = src_y + r as i32;
                        let sx = src_x + c as i32;
                        if sy >= 0 && sx >= 0 && sy < h as i32 && sx < w as i32 {
                            y_pred[r * bw + c] = y_plane[sy as usize * w + sx as usize];
                        }
                    }
                }
                if block.morph_pred {
                    apply_intrabc_morph_pred_luma(
                        &y_plane,
                        w,
                        w,
                        h,
                        bx,
                        by,
                        bw,
                        bh,
                        src_x,
                        src_y,
                        &mut y_pred,
                    );
                }
                if block.y_tu.is_empty() {
                    for r in 0..bh {
                        let dst = (by + r) * w + bx;
                        let src = r * bw;
                        y_plane[dst..dst + bw].copy_from_slice(&y_pred[src..src + bw]);
                    }
                } else {
                    for tu in block.y_tu.iter() {
                        let tu_w = tu.tx_size.width();
                        let tu_h = tu.tx_size.height();
                        let tx = bx + tu.col_offset;
                        let ty = by + tu.row_offset;
                        let residuals = transform::dequantize_and_inverse_tcq(
                            &tu.levels, tu.eob, dequant, tu.tx_size, tu.tx_type, tcq_mode,
                        );
                        reconstruct_tu(
                            &mut y_plane,
                            w,
                            tx,
                            ty,
                            tu_w,
                            tu_h,
                            &y_pred,
                            bw,
                            tu.col_offset,
                            tu.row_offset,
                            &residuals,
                        );
                    }
                }
            } else if !block.palette_y_map.is_empty() {
                let mut y_pred = vec![128u8; bw * bh];
                for r in 0..bh {
                    for c in 0..bw {
                        let color_idx = block.palette_y_map[r * bw + c] as usize;
                        y_pred[r * bw + c] = block.palette_y_colors[color_idx] as u8;
                    }
                }
                for tu in block.y_tu.iter() {
                    let tu_w = tu.tx_size.width();
                    let tu_h = tu.tx_size.height();
                    let tx = bx + tu.col_offset;
                    let ty = by + tu.row_offset;
                    let residuals = transform::dequantize_and_inverse_tcq(
                        &tu.levels, tu.eob, dequant, tu.tx_size, tu.tx_type, tcq_mode,
                    );
                    reconstruct_tu(
                        &mut y_plane,
                        w,
                        tx,
                        ty,
                        tu_w,
                        tu_h,
                        &y_pred,
                        bw,
                        tu.col_offset,
                        tu.row_offset,
                        &residuals,
                    );
                }
            } else {
                let sb_y = (by / sb_size) * sb_size;
                for tu in block.y_tu.iter() {
                    let tu_w = tu.tx_size.width();
                    let tu_h = tu.tx_size.height();
                    let tx = bx + tu.col_offset;
                    let ty = by + tu.row_offset;
                    let y_pred = predict::predict_intra_with_control(
                        &y_plane,
                        w,
                        tx,
                        ty,
                        tu_w,
                        tu_h,
                        pred_mode,
                        angle_delta,
                        block.mrl_index,
                        sb_y,
                        predict::IntraPredictControl {
                            plane: 0,
                            enable_ibp,
                            is_cfl: false,
                        },
                    );
                    let residuals = transform::dequantize_and_inverse_tcq(
                        &tu.levels, tu.eob, dequant, tu.tx_size, tu.tx_type, tcq_mode,
                    );
                    reconstruct_tu(
                        &mut y_plane,
                        w,
                        tx,
                        ty,
                        tu_w,
                        tu_h,
                        &y_pred,
                        tu_w,
                        0,
                        0,
                        &residuals,
                    );
                }
            }
        }

        if block.tree_type == tile::TreeType::Luma || !block.is_chroma_ref {
            continue;
        }

        let chroma_x = block.chroma_x;
        let chroma_y = block.chroma_y;
        let chroma_width = block.chroma_width;
        let chroma_height = block.chroma_height;
        let uv_bx = chroma_x / 2;
        let uv_by = chroma_y / 2;
        let uv_bw = chroma_width / 2;
        let uv_bh = chroma_height / 2;

        if uv_bw >= 4 && uv_bh >= 4 {
            let (u_pred, v_pred) = if block.is_intrabc {
                let uv_src_x = uv_bx as i32 + block.bv_col / 16;
                let uv_src_y = uv_by as i32 + block.bv_row / 16;
                let mut u_pred = vec![128u8; uv_bw * uv_bh];
                let mut v_pred = vec![128u8; uv_bw * uv_bh];
                for r in 0..uv_bh {
                    for c in 0..uv_bw {
                        let sy = uv_src_y + r as i32;
                        let sx = uv_src_x + c as i32;
                        if sy >= 0 && sx >= 0 && sy < uv_h as i32 && sx < uv_w as i32 {
                            u_pred[r * uv_bw + c] = u_plane[sy as usize * uv_w + sx as usize];
                            v_pred[r * uv_bw + c] = v_plane[sy as usize * uv_w + sx as usize];
                        }
                    }
                }
                (u_pred, v_pred)
            } else if block.uv_mode == 13 {
                if block.cfl_idx == 2 {
                    let u_params = derive_mhccp_params_neighbors(
                        &y_plane,
                        w,
                        h,
                        &u_plane,
                        uv_w,
                        uv_h,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        block.mhccp_dir,
                        'U',
                    )
                    .unwrap_or([0.0, 0.0, 1.0]);
                    let v_params = derive_mhccp_params_neighbors(
                        &y_plane,
                        w,
                        h,
                        &v_plane,
                        uv_w,
                        uv_h,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        block.mhccp_dir,
                        'V',
                    )
                    .unwrap_or([0.0, 0.0, 1.0]);
                    let u_pred = mhccp_predict_block(
                        &y_plane,
                        w,
                        h,
                        uv_w,
                        uv_h,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        block.mhccp_dir,
                        u_params,
                        'U',
                    );
                    let v_pred = mhccp_predict_block(
                        &y_plane,
                        w,
                        h,
                        uv_w,
                        uv_h,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        block.mhccp_dir,
                        v_params,
                        'V',
                    );
                    (u_pred, v_pred)
                } else {
                    let u_dc_pred = predict::predict_intra_with_control(
                        &u_plane,
                        uv_w,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        predict::PredictionMode::DcPred,
                        0,
                        0,
                        0,
                        predict::IntraPredictControl {
                            plane: 1,
                            enable_ibp,
                            is_cfl: true,
                        },
                    );
                    let v_dc_pred = predict::predict_intra_with_control(
                        &v_plane,
                        uv_w,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        predict::PredictionMode::DcPred,
                        0,
                        0,
                        0,
                        predict::IntraPredictControl {
                            plane: 2,
                            enable_ibp,
                            is_cfl: true,
                        },
                    );

                    let mut recon_q3 = vec![0i16; uv_bw * uv_bh];
                    for r in 0..uv_bh {
                        for c in 0..uv_bw {
                            let ly = chroma_y + r * 2;
                            let lx = chroma_x + c * 2;
                            let sum = y_plane[ly * w + lx] as i32
                                + y_plane[ly * w + lx + 1] as i32
                                + y_plane[(ly + 1) * w + lx] as i32
                                + y_plane[(ly + 1) * w + lx + 1] as i32;
                            recon_q3[r * uv_bw + c] = (sum * 2) as i16;
                        }
                    }

                    let avg_q3 =
                        cfl_neighbor_avg_q3(&y_plane, w, h, uv_w, uv_h, uv_bx, uv_by, uv_bw, uv_bh);

                    let trace_cfl = trace_block_match("WAV2C_TRACE_CFL", uv_bx, uv_by);
                    let (alpha_u_q3, alpha_v_q3) = if block.cfl_idx == 0 {
                        (block.cfl_alpha_u as i32 * 32, block.cfl_alpha_v as i32 * 32)
                    } else {
                        (
                            derive_cfl_alpha_q3_neighbors(
                                &y_plane, w, h, &u_plane, uv_w, uv_h, uv_bx, uv_by, uv_bw, uv_bh,
                            ),
                            -derive_cfl_alpha_q3_neighbors(
                                &y_plane, w, h, &v_plane, uv_w, uv_h, uv_bx, uv_by, uv_bw, uv_bh,
                            ),
                        )
                    };
                    if trace_cfl {
                        eprintln!(
                            "[WAV2C_CFL_BLOCK] uv=({}, {}) size={}x{} cfl_idx={} avg_q3={} alpha_u={} alpha_v={}",
                            uv_bx, uv_by, uv_bw, uv_bh, block.cfl_idx, avg_q3, alpha_u_q3, alpha_v_q3
                        );
                        eprintln!("[WAV2C_CFL_DC] u_dc={:?}", u_dc_pred);
                        eprintln!("[WAV2C_CFL_DC] v_dc={:?}", v_dc_pred);
                        eprintln!("[WAV2C_CFL_AC] recon_q3={:?}", recon_q3);
                    }

                    let mut u_cfl = vec![0u8; uv_bw * uv_bh];
                    let mut v_cfl = vec![0u8; uv_bw * uv_bh];
                    for r in 0..uv_bh {
                        for c in 0..uv_bw {
                            let ac = recon_q3[r * uv_bw + c] as i32 - avg_q3;
                            let u_dc = u_dc_pred[r * uv_bw + c] as i32;
                            let u_scaled = round_power_of_two_signed(alpha_u_q3 * ac, 11);
                            u_cfl[r * uv_bw + c] = (u_dc + u_scaled).clamp(0, 255) as u8;

                            let v_dc = v_dc_pred[r * uv_bw + c] as i32;
                            let v_scaled = round_power_of_two_signed(alpha_v_q3 * ac, 11);
                            v_cfl[r * uv_bw + c] = (v_dc + v_scaled).clamp(0, 255) as u8;
                        }
                    }
                    if trace_cfl {
                        eprintln!("[WAV2C_CFL_PRED] u_pred={:?}", u_cfl);
                        eprintln!("[WAV2C_CFL_PRED] v_pred={:?}", v_cfl);
                    }
                    (u_cfl, v_cfl)
                }
            } else {
                let uv_pred_mode = predict::PredictionMode::from_u8(block.uv_mode);
                let (_, y_angle_delta) = predict::decode_prediction_mode(block.joint_y_mode);
                let uv_angle_delta =
                    if block.uv_mode == block.y_mode && (1..=8).contains(&block.uv_mode) {
                        y_angle_delta
                    } else {
                        0
                    };
                let tu_count = block.u_tu.len().min(block.v_tu.len());
                if tu_count == 0 {
                    let u_p = predict::predict_intra_with_control(
                        &u_plane,
                        uv_w,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        uv_pred_mode,
                        uv_angle_delta,
                        0,
                        0,
                        predict::IntraPredictControl {
                            plane: 1,
                            enable_ibp,
                            is_cfl: false,
                        },
                    );
                    let v_p = predict::predict_intra_with_control(
                        &v_plane,
                        uv_w,
                        uv_bx,
                        uv_by,
                        uv_bw,
                        uv_bh,
                        uv_pred_mode,
                        uv_angle_delta,
                        0,
                        0,
                        predict::IntraPredictControl {
                            plane: 2,
                            enable_ibp,
                            is_cfl: false,
                        },
                    );
                    (u_p, v_p)
                } else {
                    for i in 0..tu_count {
                        let u_tu = &block.u_tu[i];
                        let v_tu = &block.v_tu[i];

                        let mut u_dq = transform::dequantize_only(
                            &u_tu.levels,
                            u_tu.eob,
                            uv_dequant,
                            u_tu.tx_size,
                            u_tu.tx_type,
                            tcq_mode,
                        );
                        let mut v_dq = transform::dequantize_only(
                            &v_tu.levels,
                            v_tu.eob,
                            uv_dequant,
                            v_tu.tx_size,
                            v_tu.tx_type,
                            tcq_mode,
                        );

                        if u_tu.cctx_type != 0 {
                            transform::inverse_cctx(&mut u_dq, &mut v_dq, u_tu.cctx_type);
                        }

                        let u_res =
                            transform::inverse_transform_only(&u_dq, u_tu.tx_size, u_tu.tx_type);
                        let v_res =
                            transform::inverse_transform_only(&v_dq, v_tu.tx_size, v_tu.tx_type);

                        let u_tu_w = u_tu.tx_size.width();
                        let u_tu_h = u_tu.tx_size.height();
                        let u_tx = uv_bx + u_tu.col_offset;
                        let u_ty = uv_by + u_tu.row_offset;
                        let u_pred = predict::predict_intra_with_control(
                            &u_plane,
                            uv_w,
                            u_tx,
                            u_ty,
                            u_tu_w,
                            u_tu_h,
                            uv_pred_mode,
                            uv_angle_delta,
                            0,
                            0,
                            predict::IntraPredictControl {
                                plane: 1,
                                enable_ibp,
                                is_cfl: false,
                            },
                        );
                        reconstruct_tu(
                            &mut u_plane,
                            uv_w,
                            u_tx,
                            u_ty,
                            u_tu_w,
                            u_tu_h,
                            &u_pred,
                            u_tu_w,
                            0,
                            0,
                            &u_res,
                        );

                        let v_tu_w = v_tu.tx_size.width();
                        let v_tu_h = v_tu.tx_size.height();
                        let v_tx = uv_bx + v_tu.col_offset;
                        let v_ty = uv_by + v_tu.row_offset;
                        let v_pred = predict::predict_intra_with_control(
                            &v_plane,
                            uv_w,
                            v_tx,
                            v_ty,
                            v_tu_w,
                            v_tu_h,
                            uv_pred_mode,
                            uv_angle_delta,
                            0,
                            0,
                            predict::IntraPredictControl {
                                plane: 2,
                                enable_ibp,
                                is_cfl: false,
                            },
                        );
                        reconstruct_tu(
                            &mut v_plane,
                            uv_w,
                            v_tx,
                            v_ty,
                            v_tu_w,
                            v_tu_h,
                            &v_pred,
                            v_tu_w,
                            0,
                            0,
                            &v_res,
                        );
                    }
                    continue;
                }
            };

            let tu_count = block.u_tu.len().min(block.v_tu.len());
            if tu_count == 0 {
                for r in 0..uv_bh {
                    let dst = (uv_by + r) * uv_w + uv_bx;
                    let src = r * uv_bw;
                    u_plane[dst..dst + uv_bw].copy_from_slice(&u_pred[src..src + uv_bw]);
                    v_plane[dst..dst + uv_bw].copy_from_slice(&v_pred[src..src + uv_bw]);
                }
            } else {
                for i in 0..tu_count {
                    let u_tu = &block.u_tu[i];
                    let v_tu = &block.v_tu[i];

                    let mut u_dq = transform::dequantize_only(
                        &u_tu.levels,
                        u_tu.eob,
                        uv_dequant,
                        u_tu.tx_size,
                        u_tu.tx_type,
                        tcq_mode,
                    );
                    let mut v_dq = transform::dequantize_only(
                        &v_tu.levels,
                        v_tu.eob,
                        uv_dequant,
                        v_tu.tx_size,
                        v_tu.tx_type,
                        tcq_mode,
                    );

                    if u_tu.cctx_type != 0 {
                        transform::inverse_cctx(&mut u_dq, &mut v_dq, u_tu.cctx_type);
                    }

                    let u_res =
                        transform::inverse_transform_only(&u_dq, u_tu.tx_size, u_tu.tx_type);
                    let v_res =
                        transform::inverse_transform_only(&v_dq, v_tu.tx_size, v_tu.tx_type);

                    let u_tu_w = u_tu.tx_size.width();
                    let u_tu_h = u_tu.tx_size.height();
                    reconstruct_tu(
                        &mut u_plane,
                        uv_w,
                        uv_bx + u_tu.col_offset,
                        uv_by + u_tu.row_offset,
                        u_tu_w,
                        u_tu_h,
                        &u_pred,
                        uv_bw,
                        u_tu.col_offset,
                        u_tu.row_offset,
                        &u_res,
                    );

                    let v_tu_w = v_tu.tx_size.width();
                    let v_tu_h = v_tu.tx_size.height();
                    reconstruct_tu(
                        &mut v_plane,
                        uv_w,
                        uv_bx + v_tu.col_offset,
                        uv_by + v_tu.row_offset,
                        v_tu_w,
                        v_tu_h,
                        &v_pred,
                        uv_bw,
                        v_tu.col_offset,
                        v_tu.row_offset,
                        &v_res,
                    );
                }
            }
        }
    }

    (y_plane, u_plane, v_plane)
}

fn gray_frame(width: u32, height: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    (
        vec![128u8; w * h],
        vec![128u8; (w / 2) * (h / 2)],
        vec![128u8; (w / 2) * (h / 2)],
    )
}

fn reconstruct_frame(
    sbs: &[tile::DecodedSbCoeffs],
    sb_cols: usize,
    sb_rows: usize,
    width: u32,
    height: u32,
    dequant: i32,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    let uv_w = w / 2;
    let uv_h = h / 2;
    let mut y_plane = vec![0u8; w * h];
    let mut u_plane = vec![0u8; uv_w * uv_h];
    let mut v_plane = vec![0u8; uv_w * uv_h];

    for sb_row in 0..sb_rows {
        for sb_col in 0..sb_cols {
            let sb = &sbs[sb_row * sb_cols + sb_col];

            let y_x = sb_col * 64;
            let y_y = sb_row * 64;
            let y_mode = crate::encode::IntraMode::from_index(sb.y_mode);
            let y_pred_block =
                crate::encode::predict_intra_block(&y_plane, w, y_x, y_y, 64, y_mode);
            let y_residuals =
                transform::dequantize_and_inverse(&sb.y_levels, sb.y_eob, dequant, 2, true);
            crate::encode::reconstruct_block(
                &mut y_plane,
                w,
                y_x,
                y_y,
                64,
                &y_pred_block,
                &y_residuals,
            );

            let uv_x = sb_col * 32;
            let uv_y = sb_row * 32;
            let uv_mode = crate::encode::IntraMode::from_index(sb.uv_mode);
            let u_pred_block =
                crate::encode::predict_intra_block(&u_plane, uv_w, uv_x, uv_y, 32, uv_mode);
            let u_residuals =
                transform::dequantize_and_inverse(&sb.u_levels, sb.u_eob, dequant, 1, false);
            crate::encode::reconstruct_block(
                &mut u_plane,
                uv_w,
                uv_x,
                uv_y,
                32,
                &u_pred_block,
                &u_residuals,
            );

            let v_pred_block =
                crate::encode::predict_intra_block(&v_plane, uv_w, uv_x, uv_y, 32, uv_mode);
            let v_residuals =
                transform::dequantize_and_inverse(&sb.v_levels, sb.v_eob, dequant, 1, false);
            crate::encode::reconstruct_block(
                &mut v_plane,
                uv_w,
                uv_x,
                uv_y,
                32,
                &v_pred_block,
                &v_residuals,
            );
        }
    }

    (y_plane, u_plane, v_plane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{encode_video_yuv, encode_video_yuv_with_cdf_update};

    #[test]
    fn decode_solid_gray() {
        let y = vec![128u8; 64 * 64];
        let u = vec![128u8; 32 * 32];
        let v = vec![128u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].width, 64);
        assert_eq!(frames[0].height, 64);
        assert!(frames[0].y.iter().all(|&p| p == 128));
    }

    #[test]
    fn decode_solid_green() {
        let y = vec![149u8; 64 * 64];
        let u = vec![43u8; 32 * 32];
        let v = vec![21u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        for (i, &p) in frames[0].y.iter().enumerate() {
            assert!(
                (p as i32 - 149).abs() <= 1,
                "Y pixel {} = {}, expected ~149",
                i,
                p
            );
        }
    }

    #[test]
    fn decode_multi_frame() {
        let frames_in: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = (0..3)
            .map(|i| {
                let val = 100 + i * 20;
                (
                    vec![val; 64 * 64],
                    vec![128u8; 32 * 32],
                    vec![128u8; 32 * 32],
                )
            })
            .collect();
        let bs = encode_video_yuv(64, 64, 50, &frames_in);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 3);
    }

    #[test]
    fn decode_128x128() {
        let y = vec![200u8; 128 * 128];
        let u = vec![100u8; 64 * 64];
        let v = vec![150u8; 64 * 64];
        let bs = encode_video_yuv(128, 128, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].width, 128);
        assert_eq!(frames[0].height, 128);
    }

    #[test]
    fn decode_gradient_y() {
        let mut y = vec![0u8; 128 * 128];
        for row in 0..128 {
            let val = (row * 4).min(255) as u8;
            for col in 0..128 {
                y[row * 128 + col] = val;
            }
        }
        let u = vec![128u8; 64 * 64];
        let v = vec![128u8; 64 * 64];
        let bs = encode_video_yuv(128, 128, 50, &[(y.clone(), u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        for row in 0..128 {
            let expected = (row * 4).min(255) as i32;
            for col in 0..128 {
                let actual = frames[0].y[row * 128 + col] as i32;
                assert!(
                    (actual - expected).abs() <= 2,
                    "Y pixel ({},{}) = {}, expected ~{}",
                    row,
                    col,
                    actual,
                    expected
                );
            }
        }
    }

    #[test]
    fn decode_256x192() {
        let y = vec![180u8; 256 * 192];
        let u = vec![80u8; 128 * 96];
        let v = vec![200u8; 128 * 96];
        let bs = encode_video_yuv(256, 192, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].width, 256);
        assert_eq!(frames[0].height, 192);
        for (i, &p) in frames[0].y.iter().enumerate() {
            assert!(
                (p as i32 - 180).abs() <= 1,
                "Y pixel {} = {}, expected ~180",
                i,
                p
            );
        }
    }

    #[test]
    fn decode_all_planes_exact() {
        let y = vec![128u8; 64 * 64];
        let u = vec![128u8; 32 * 32];
        let v = vec![128u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].y.iter().all(|&p| p == 128));
        assert!(frames[0].u.iter().all(|&p| p == 128));
        assert!(frames[0].v.iter().all(|&p| p == 128));
    }

    #[test]
    fn decode_multi_frame_values() {
        let y_values = [80u8, 128u8, 200u8];
        let frames_in: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = y_values
            .iter()
            .map(|&val| {
                (
                    vec![val; 64 * 64],
                    vec![128u8; 32 * 32],
                    vec![128u8; 32 * 32],
                )
            })
            .collect();
        let bs = encode_video_yuv(64, 64, 50, &frames_in);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 3);
        for (idx, &expected_y) in y_values.iter().enumerate() {
            let avg: f64 =
                frames[idx].y.iter().map(|&p| p as f64).sum::<f64>() / frames[idx].y.len() as f64;
            assert!(
                (avg - expected_y as f64).abs() <= 2.0,
                "Frame {} Y average = {:.1}, expected ~{}",
                idx,
                avg,
                expected_y
            );
        }
    }

    #[test]
    fn decode_low_qindex() {
        let y = vec![100u8; 64 * 64];
        let u = vec![60u8; 32 * 32];
        let v = vec![220u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 25, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        for (i, &p) in frames[0].y.iter().enumerate() {
            assert!(
                (p as i32 - 100).abs() <= 1,
                "Y pixel {} = {}, expected ~100",
                i,
                p
            );
        }
    }

    #[test]
    fn roundtrip_cdf_adaptation_enabled() {
        let y = vec![128u8; 64 * 64];
        let u = vec![128u8; 32 * 32];
        let v = vec![128u8; 32 * 32];
        let bs = encode_video_yuv_with_cdf_update(64, 64, 50, &[(y.clone(), u.clone(), v.clone())]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].y.iter().all(|&p| p == 128));
        assert!(frames[0].u.iter().all(|&p| p == 128));
        assert!(frames[0].v.iter().all(|&p| p == 128));
    }

    #[test]
    fn roundtrip_cdf_adaptation_with_coeffs() {
        let y = vec![200u8; 64 * 64];
        let u = vec![100u8; 32 * 32];
        let v = vec![150u8; 32 * 32];
        let bs = encode_video_yuv_with_cdf_update(64, 64, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        for &p in &frames[0].y {
            assert!(
                (p as i32 - 200).abs() <= 1,
                "Y pixel = {}, expected ~200",
                p
            );
        }
    }

    #[test]
    fn roundtrip_cdf_adaptation_128x128() {
        let y = vec![160u8; 128 * 128];
        let u = vec![128u8; 64 * 64];
        let v = vec![128u8; 64 * 64];
        let bs = encode_video_yuv_with_cdf_update(128, 128, 50, &[(y, u, v)]);
        let frames = Decoder::decode_all(&bs).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].width, 128);
        assert_eq!(frames[0].height, 128);
        for &p in &frames[0].y {
            assert!(
                (p as i32 - 160).abs() <= 1,
                "Y pixel = {}, expected ~160",
                p
            );
        }
    }

    #[test]
    fn parse_avmenc_minimal_obus() {
        let data = std::fs::read("tests/fixtures/avmenc_minimal.obu").unwrap();
        let obus = obu::parse_obus(&data);
        assert_eq!(obus.len(), 2, "Expected 2 OBUs, got {}", obus.len());
        assert_eq!(obus[0].obu_type, 1, "First OBU should be sequence header");
        assert_eq!(obus[1].obu_type, 4, "Second OBU should be CLK");
    }

    #[test]
    fn parse_avmenc_minimal_seq_header() {
        let data = std::fs::read("tests/fixtures/avmenc_minimal.obu").unwrap();
        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        assert_eq!(seq.width, 256);
        assert_eq!(seq.height, 128);
        assert!(seq.single_picture_header_flag);
        eprintln!("avmenc_minimal sb_size={}", seq.sb_size);
    }

    #[test]
    fn parse_avmenc_minimal_frame_header() {
        let data = std::fs::read("tests/fixtures/avmenc_minimal.obu").unwrap();
        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        let fp = frame::parse_frame_header(obus[1].payload, &seq).unwrap();
        assert_eq!(fp.base_qindex, 100);
        assert!(!fp.disable_cdf_update);
        assert!(!fp.tile_data.is_empty());
    }

    #[test]
    fn decode_avmenc_minimal_tile_data() {
        let data = std::fs::read("tests/fixtures/avmenc_minimal.obu").unwrap();
        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        let fp = frame::parse_frame_header(obus[1].payload, &seq).unwrap();

        let tile_cfg = tile::TileDecodeConfig {
            enable_cfl_intra: seq.enable_cfl_intra,
            enable_fsc: seq.enable_fsc,
            enable_idtx_intra: seq.enable_idtx_intra,
            enable_mrls: seq.enable_mrls,
            enable_ist: seq.enable_ist,
            enable_cctx: seq.enable_cctx,
            tx_mode_select: fp.tx_mode_select,
            reduced_tx_set_used: fp.reduced_tx_set_used,
            reduced_tx_part_set: seq.reduced_tx_part_set,
            allow_intrabc: fp.allow_intrabc,
            allow_screen_content_tools: fp.allow_screen_content_tools,
            enable_sdp: seq.enable_sdp,
            cdef_frame_enable: fp.cdef_frame_enable,
            nb_cdef_strengths: fp.nb_cdef_strengths,
            ccso_frame_enable: fp.ccso_frame_enable,
            ccso_enable: fp.ccso_enable,
            sb_size: seq.sb_size as usize,
            tcq_mode: fp.tcq_mode,
            allow_parity_hiding: fp.allow_parity_hiding,
            enable_ext_partitions: seq.enable_ext_partitions,
            enable_uneven_4way_partitions: seq.enable_uneven_4way_partitions,
            enable_mhccp: seq.enable_mhccp,
            enable_refmvbank: seq.enable_refmvbank,
            max_bvp_drl_bits: fp.max_bvp_drl_bits,
            cur_frame_force_integer_mv: fp.cur_frame_force_integer_mv,
            allow_intrabc_morph_pred: seq.enable_intra_bawp && fp.allow_screen_content_tools,
        };

        let blocks = tile::decode_tile_data_partitioned_cfg(
            fp.tile_data,
            seq.width as usize,
            seq.height as usize,
            seq.sb_size as usize,
            fp.base_qindex,
            &tile_cfg,
        );

        for (i, block) in blocks.iter().enumerate() {
            eprintln!(
                "Block {}: ({},{}) {}x{} y_mode={} uv_mode={}",
                i, block.x, block.y, block.width, block.height, block.y_mode, block.uv_mode
            );
        }

        assert!(!blocks.is_empty(), "Should decode at least one block");
    }

    #[test]
    fn reconstruct_avmenc_minimal_frame() {
        let data = std::fs::read("tests/fixtures/avmenc_minimal.obu").unwrap();
        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        let fp = frame::parse_frame_header(obus[1].payload, &seq).unwrap();
        let dequant = crate::encode::ac_qlookup(fp.base_qindex);

        let tile_cfg = tile::TileDecodeConfig {
            enable_cfl_intra: seq.enable_cfl_intra,
            enable_fsc: seq.enable_fsc,
            enable_idtx_intra: seq.enable_idtx_intra,
            enable_mrls: seq.enable_mrls,
            enable_ist: seq.enable_ist,
            enable_cctx: seq.enable_cctx,
            tx_mode_select: fp.tx_mode_select,
            reduced_tx_set_used: fp.reduced_tx_set_used,
            reduced_tx_part_set: seq.reduced_tx_part_set,
            allow_intrabc: fp.allow_intrabc,
            allow_screen_content_tools: fp.allow_screen_content_tools,
            enable_sdp: seq.enable_sdp,
            cdef_frame_enable: fp.cdef_frame_enable,
            nb_cdef_strengths: fp.nb_cdef_strengths,
            ccso_frame_enable: fp.ccso_frame_enable,
            ccso_enable: fp.ccso_enable,
            sb_size: seq.sb_size as usize,
            tcq_mode: fp.tcq_mode,
            allow_parity_hiding: fp.allow_parity_hiding,
            enable_ext_partitions: seq.enable_ext_partitions,
            enable_uneven_4way_partitions: seq.enable_uneven_4way_partitions,
            enable_mhccp: seq.enable_mhccp,
            enable_refmvbank: seq.enable_refmvbank,
            max_bvp_drl_bits: fp.max_bvp_drl_bits,
            cur_frame_force_integer_mv: fp.cur_frame_force_integer_mv,
            allow_intrabc_morph_pred: seq.enable_intra_bawp && fp.allow_screen_content_tools,
        };

        let blocks = tile::decode_tile_data_partitioned_cfg(
            fp.tile_data,
            seq.width as usize,
            seq.height as usize,
            seq.sb_size as usize,
            fp.base_qindex,
            &tile_cfg,
        );
        let uv_dequant = crate::encode::ac_qlookup(
            (fp.base_qindex as i32 + fp.u_ac_delta_q).clamp(0, 255) as u8,
        );
        let (y_plane, u_plane, v_plane) = reconstruct_partitioned_frame(
            &blocks,
            seq.width,
            seq.height,
            dequant,
            uv_dequant,
            fp.tcq_mode,
            seq.sb_size as usize,
            seq.enable_ibp,
        );

        let w = seq.width as usize;
        let h = seq.height as usize;
        assert_eq!(y_plane.len(), w * h);
        assert_eq!(u_plane.len(), w / 2 * h / 2);
        assert_eq!(v_plane.len(), w / 2 * h / 2);

        let ref_y4m = std::fs::read("/tmp/minimal_sph1_decoded.y4m").unwrap();
        let y4m_header = "YUV4MPEG2 W256 H128 F0:0 Ip C420jpeg\nFRAME\n";
        let header_len = y4m_header.len();
        let ref_y = &ref_y4m[header_len..header_len + w * h];
        let ref_u = &ref_y4m[header_len + w * h..header_len + w * h + w / 2 * h / 2];

        let y_mse: f64 = y_plane
            .iter()
            .zip(ref_y.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w * h) as f64;
        let y_psnr = if y_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / y_mse).log10()
        } else {
            100.0
        };

        let u_mse: f64 = u_plane
            .iter()
            .zip(ref_u.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w / 2 * h / 2) as f64;
        let u_psnr = if u_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / u_mse).log10()
        } else {
            100.0
        };

        let ref_v =
            &ref_y4m[header_len + w * h + w / 2 * h / 2..header_len + w * h + 2 * (w / 2 * h / 2)];
        let v_mse: f64 = v_plane
            .iter()
            .zip(ref_v.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w / 2 * h / 2) as f64;
        let v_psnr = if v_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / v_mse).log10()
        } else {
            100.0
        };

        eprintln!(
            "PSNR: Y={:.2} dB, U={:.2} dB, V={:.2} dB",
            y_psnr, u_psnr, v_psnr
        );
        assert!(
            y_psnr >= 40.0,
            "Y PSNR={:.2} dB, expected >= 40 dB (entropy sync verified, reconstruction pending)",
            y_psnr
        );
        assert!(u_psnr >= 40.0, "U PSNR={:.2} dB, expected >= 40 dB", u_psnr);
        assert!(v_psnr >= 40.0, "V PSNR={:.2} dB, expected >= 40 dB", v_psnr);
    }

    #[test]
    fn decode_avmenc_128x128_q30() {
        let data = std::fs::read("tests/fixtures/avmenc_128x128_q30.obu").unwrap();
        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        assert_eq!(seq.width, 128);
        assert_eq!(seq.height, 128);

        eprintln!(
            "SEQ: sph={} sb_size={} enable_sdp={} enable_cfl={} oh_bits={} lt_id_bits={} force_sct={} force_imv={} bvp_drl={} tcq={} ph={} sep_uv={} eq_dc={} y_dc={} uv_dc={} uv_ac={} cdef={} gdf={} rest={} ccso={} df_par={} fg={}",
            seq.single_picture_header_flag,
            seq.sb_size,
            seq.enable_sdp,
            seq.enable_cfl_intra,
            seq.order_hint_bits,
            seq.number_of_bits_for_lt_frame_id,
            seq.force_screen_content_tools,
            seq.force_integer_mv,
            seq.allow_frame_max_bvp_drl_bits,
            seq.enable_tcq,
            seq.enable_parity_hiding,
            seq.separate_uv_delta_q,
            seq.equal_ac_dc_q,
            seq.y_dc_delta_q_enabled,
            seq.uv_dc_delta_q_enabled,
            seq.uv_ac_delta_q_enabled,
            seq.enable_cdef,
            seq.enable_gdf,
            seq.enable_restoration,
            seq.enable_ccso,
            seq.df_par_bits,
            seq.film_grain_params_present
        );

        let fp = frame::parse_frame_header(obus[1].payload, &seq).unwrap();
        let header_bytes = obus[1].payload.len() - fp.tile_data.len();
        eprintln!(
            "FRAME: qindex={} disable_cdf={} oh={} header_bytes={} tile_data_len={} tx_mode_select={} tcq_mode={} reduced_tx_set={}",
            fp.base_qindex,
            fp.disable_cdf_update,
            fp.order_hint,
            header_bytes,
            fp.tile_data.len(),
            fp.tx_mode_select,
            fp.tcq_mode,
            fp.reduced_tx_set_used
        );
        eprintln!(
            "tile_data first 16 bytes: {:02x?}",
            &fp.tile_data[..fp.tile_data.len().min(16)]
        );
        eprintln!("expected tile_data start: f6 ac 00 72 85 91 72 ff (from reference)");
        let dequant = crate::encode::ac_qlookup(fp.base_qindex);

        let tile_cfg = tile::TileDecodeConfig {
            enable_cfl_intra: seq.enable_cfl_intra,
            enable_fsc: seq.enable_fsc,
            enable_idtx_intra: seq.enable_idtx_intra,
            enable_mrls: seq.enable_mrls,
            enable_ist: seq.enable_ist,
            enable_cctx: seq.enable_cctx,
            tx_mode_select: fp.tx_mode_select,
            reduced_tx_set_used: fp.reduced_tx_set_used,
            reduced_tx_part_set: seq.reduced_tx_part_set,
            allow_intrabc: fp.allow_intrabc,
            allow_screen_content_tools: fp.allow_screen_content_tools,
            enable_sdp: seq.enable_sdp,
            cdef_frame_enable: fp.cdef_frame_enable,
            nb_cdef_strengths: fp.nb_cdef_strengths,
            ccso_frame_enable: fp.ccso_frame_enable,
            ccso_enable: fp.ccso_enable,
            sb_size: seq.sb_size as usize,
            tcq_mode: fp.tcq_mode,
            allow_parity_hiding: fp.allow_parity_hiding,
            enable_ext_partitions: seq.enable_ext_partitions,
            enable_uneven_4way_partitions: seq.enable_uneven_4way_partitions,
            enable_mhccp: seq.enable_mhccp,
            enable_refmvbank: seq.enable_refmvbank,
            max_bvp_drl_bits: fp.max_bvp_drl_bits,
            cur_frame_force_integer_mv: fp.cur_frame_force_integer_mv,
            allow_intrabc_morph_pred: seq.enable_intra_bawp && fp.allow_screen_content_tools,
        };

        let blocks = tile::decode_tile_data_partitioned_cfg(
            fp.tile_data,
            seq.width as usize,
            seq.height as usize,
            seq.sb_size as usize,
            fp.base_qindex,
            &tile_cfg,
        );

        for (i, block) in blocks.iter().enumerate() {
            let y_eob: u32 = block.y_tu.iter().map(|t| t.eob as u32).sum();
            eprintln!(
                "Block[{:2}]: ({:3},{:3}) {:3}x{:3} y_mode={:2} y_tu={} y_eob={}",
                i,
                block.x,
                block.y,
                block.width,
                block.height,
                block.y_mode,
                block.y_tu.len(),
                y_eob
            );
            for (ti, tu) in block.y_tu.iter().enumerate() {
                let residuals = transform::dequantize_and_inverse_tcq(
                    &tu.levels,
                    tu.eob,
                    dequant,
                    tu.tx_size,
                    tu.tx_type,
                    fp.tcq_mode,
                );
                let nonzero: usize = residuals.iter().filter(|&&v| v != 0).count();
                let dc = if !tu.levels.is_empty() {
                    tu.levels[0]
                } else {
                    0
                };
                eprintln!(
                    "  TU[{}]: tx={:?} eob={} dc_level={} nonzero_residuals={} first_res=[{}, {}, {}, {}]",
                    ti,
                    tu.tx_size,
                    tu.eob,
                    dc,
                    nonzero,
                    residuals[0],
                    residuals[1],
                    residuals[2],
                    residuals[3]
                );
            }
        }

        let luma_pixels: usize = blocks
            .iter()
            .filter(|b| !b.y_tu.is_empty())
            .map(|b| b.width * b.height)
            .sum();
        let chroma_blocks: usize = blocks.iter().filter(|b| b.y_tu.is_empty()).count();
        eprintln!(
            "luma_pixels={} chroma_blocks={} total_blocks={}",
            luma_pixels,
            chroma_blocks,
            blocks.len()
        );
        assert!(
            luma_pixels <= 128 * 128,
            "luma coverage exceeds frame: {}",
            luma_pixels
        );

        let uv_dequant_128 = crate::encode::ac_qlookup(
            (fp.base_qindex as i32 + fp.u_ac_delta_q).clamp(0, 255) as u8,
        );
        let (y_plane, _u_plane, _v_plane) = reconstruct_partitioned_frame(
            &blocks,
            seq.width,
            seq.height,
            dequant,
            uv_dequant_128,
            fp.tcq_mode,
            seq.sb_size as usize,
            seq.enable_ibp,
        );

        let avmdec = "/Users/rafaelcaricio/development/avm_build/avmdec";
        let output = std::process::Command::new(avmdec)
            .args([
                "--codec=av2",
                "-o",
                "/tmp/avmenc_128x128_q30_ref.y4m",
                "tests/fixtures/avmenc_128x128_q30.obu",
            ])
            .output()
            .expect("avmdec not found");
        assert!(output.status.success(), "avmdec failed");

        let ref_y4m = std::fs::read("/tmp/avmenc_128x128_q30_ref.y4m").unwrap();
        let w = seq.width as usize;
        let h = seq.height as usize;
        let y4m_header = format!("YUV4MPEG2 W{} H{} F0:0 Ip C420jpeg\nFRAME\n", w, h);
        let ref_y = &ref_y4m[y4m_header.len()..y4m_header.len() + w * h];

        let y_mse: f64 = y_plane
            .iter()
            .zip(ref_y.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w * h) as f64;
        let y_psnr = if y_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / y_mse).log10()
        } else {
            100.0
        };
        eprintln!(
            "Y PSNR={:.2} dB (MSE={:.2}) sb_size={}",
            y_psnr, y_mse, seq.sb_size
        );

        for (bi, block) in blocks.iter().enumerate() {
            let (bx, by, bw, bh) = (block.x, block.y, block.width, block.height);
            let mut block_sse = 0.0f64;
            let mut block_count = 0usize;
            for row in by..(by + bh).min(h) {
                for col in bx..(bx + bw).min(w) {
                    let ours = y_plane[row * w + col] as f64;
                    let theirs = ref_y[row * w + col] as f64;
                    block_sse += (ours - theirs).powi(2);
                    block_count += 1;
                }
            }
            let block_mse = block_sse / block_count as f64;
            let block_psnr = if block_mse > 0.0 {
                10.0 * (255.0f64 * 255.0 / block_mse).log10()
            } else {
                100.0
            };

            if block_psnr < 40.0 {
                let sample_row = by;
                let end_col = (bx + 8).min(bx + bw).min(w);
                let our_samples: Vec<u8> =
                    (bx..end_col).map(|c| y_plane[sample_row * w + c]).collect();
                let ref_samples: Vec<u8> =
                    (bx..end_col).map(|c| ref_y[sample_row * w + c]).collect();
                eprintln!(
                    "  BAD Block[{:2}] ({},{}) {}x{}: PSNR={:.1} ours={:?} ref={:?}",
                    bi, bx, by, bw, bh, block_psnr, our_samples, ref_samples
                );
            }
        }
    }

    #[test]
    fn decode_avmenc_nofilter_psnr() {
        let obu_path = "tests/fixtures/avmenc_128x128_q30_nofilter.obu";
        if !std::path::Path::new(obu_path).exists() {
            eprintln!("SKIP: {} not found", obu_path);
            return;
        }
        let data = std::fs::read(obu_path).unwrap();
        let frames = Decoder::decode_all(&data).unwrap();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        let w = frame.width as usize;
        let h = frame.height as usize;

        let avmdec = "/Users/rafaelcaricio/development/avm_build/avmdec";
        let ref_path = "/tmp/avmenc_128x128_q30_nofilter_ref.y4m";
        let output = std::process::Command::new(avmdec)
            .args(["--codec=av2", "-o", ref_path, obu_path])
            .output()
            .expect("avmdec not found");
        assert!(
            output.status.success(),
            "avmdec failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let ref_y4m = std::fs::read(ref_path).unwrap();
        let y4m_header = format!("YUV4MPEG2 W{} H{} F0:0 Ip C420jpeg\nFRAME\n", w, h);
        let hlen = y4m_header.len();
        let ref_y = &ref_y4m[hlen..hlen + w * h];
        let ref_u = &ref_y4m[hlen + w * h..hlen + w * h + w / 2 * h / 2];
        let ref_v = &ref_y4m[hlen + w * h + w / 2 * h / 2..hlen + w * h + 2 * (w / 2 * h / 2)];

        let y_mse: f64 = frame
            .y
            .iter()
            .zip(ref_y.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w * h) as f64;
        let y_psnr = if y_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / y_mse).log10()
        } else {
            100.0
        };

        let u_mse: f64 = frame
            .u
            .iter()
            .zip(ref_u.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w / 2 * h / 2) as f64;
        let u_psnr = if u_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / u_mse).log10()
        } else {
            100.0
        };

        let v_mse: f64 = frame
            .v
            .iter()
            .zip(ref_v.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w / 2 * h / 2) as f64;
        let v_psnr = if v_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / v_mse).log10()
        } else {
            100.0
        };

        eprintln!(
            "No-filter PSNR: Y={:.2} dB  U={:.2} dB  V={:.2} dB",
            y_psnr, u_psnr, v_psnr
        );
        for row_y in [0, 8, 16, 32, 48, 64, 80, 96, 112] {
            if row_y < h {
                let ours: Vec<u8> = (0..16.min(w)).map(|c| frame.y[row_y * w + c]).collect();
                let refs: Vec<u8> = (0..16.min(w)).map(|c| ref_y[row_y * w + c]).collect();
                let row_mse: f64 = (0..w)
                    .map(|c| (frame.y[row_y * w + c] as f64 - ref_y[row_y * w + c] as f64).powi(2))
                    .sum::<f64>()
                    / w as f64;
                eprintln!(
                    "Row {:3}: ours={:?} ref={:?} row_mse={:.1}",
                    row_y, ours, refs, row_mse
                );
            }
        }

        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        let fp = frame::parse_frame_header(obus[1].payload, &seq).unwrap();
        let dequant = crate::encode::ac_qlookup(fp.base_qindex);
        eprintln!(
            "SEQ: sb_size={} sdp={} cdef={} gdf={} rest={} ccso={} tcq={}",
            seq.sb_size,
            seq.enable_sdp,
            seq.enable_cdef,
            seq.enable_gdf,
            seq.enable_restoration,
            seq.enable_ccso,
            seq.enable_tcq
        );
        eprintln!(
            "FP: qindex={} dequant={} tx_mode_select={} disable_cdf={}",
            fp.base_qindex, dequant, fp.tx_mode_select, fp.disable_cdf_update
        );

        let tile_cfg = tile::TileDecodeConfig {
            enable_cfl_intra: seq.enable_cfl_intra,
            enable_fsc: seq.enable_fsc,
            enable_idtx_intra: seq.enable_idtx_intra,
            enable_mrls: seq.enable_mrls,
            enable_ist: seq.enable_ist,
            enable_cctx: seq.enable_cctx,
            tx_mode_select: fp.tx_mode_select,
            reduced_tx_set_used: fp.reduced_tx_set_used,
            reduced_tx_part_set: seq.reduced_tx_part_set,
            allow_intrabc: fp.allow_intrabc,
            allow_screen_content_tools: fp.allow_screen_content_tools,
            enable_sdp: seq.enable_sdp,
            cdef_frame_enable: fp.cdef_frame_enable,
            nb_cdef_strengths: fp.nb_cdef_strengths,
            ccso_frame_enable: fp.ccso_frame_enable,
            ccso_enable: fp.ccso_enable,
            sb_size: seq.sb_size as usize,
            tcq_mode: fp.tcq_mode,
            allow_parity_hiding: fp.allow_parity_hiding,
            enable_ext_partitions: seq.enable_ext_partitions,
            enable_uneven_4way_partitions: seq.enable_uneven_4way_partitions,
            enable_mhccp: seq.enable_mhccp,
            enable_refmvbank: seq.enable_refmvbank,
            max_bvp_drl_bits: fp.max_bvp_drl_bits,
            cur_frame_force_integer_mv: fp.cur_frame_force_integer_mv,
            allow_intrabc_morph_pred: seq.enable_intra_bawp && fp.allow_screen_content_tools,
        };
        let blocks = tile::decode_tile_data_partitioned_cfg(
            fp.tile_data,
            w,
            h,
            seq.sb_size as usize,
            fp.base_qindex,
            &tile_cfg,
        );
        for (i, block) in blocks.iter().enumerate().take(15) {
            let y_eob: u32 = block.y_tu.iter().map(|t| t.eob as u32).sum();
            eprintln!(
                "NF Block[{:2}]: ({:3},{:3}) {:3}x{:3} y_mode={:2} y_tu={} y_eob={}",
                i,
                block.x,
                block.y,
                block.width,
                block.height,
                block.y_mode,
                block.y_tu.len(),
                y_eob
            );
            for (ti, tu) in block.y_tu.iter().enumerate() {
                let residuals = transform::dequantize_and_inverse_tcq(
                    &tu.levels,
                    tu.eob,
                    dequant,
                    tu.tx_size,
                    tu.tx_type,
                    fp.tcq_mode,
                );
                let dc = if !tu.levels.is_empty() {
                    tu.levels[0]
                } else {
                    0
                };
                eprintln!(
                    "  TU[{}]: tx={:?} eob={} dc_level={} first_res=[{}, {}, {}, {}]",
                    ti,
                    tu.tx_size,
                    tu.eob,
                    dc,
                    residuals[0],
                    residuals[1],
                    residuals[2],
                    residuals[3]
                );
            }
        }
    }

    #[test]
    fn decode_avmenc_noibc_psnr() {
        let obu_path = "tests/fixtures/avmenc_128x128_q30_noibc.obu";
        if !std::path::Path::new(obu_path).exists() {
            eprintln!("SKIP: {} not found", obu_path);
            return;
        }
        let data = std::fs::read(obu_path).unwrap();
        let frames = Decoder::decode_all(&data).unwrap();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        let w = frame.width as usize;
        let h = frame.height as usize;

        let avmdec = "/Users/rafaelcaricio/development/avm_build/avmdec";
        let ref_path = "/tmp/avmenc_128x128_q30_noibc_ref.y4m";
        let output = std::process::Command::new(avmdec)
            .args(["--codec=av2", "-o", ref_path, obu_path])
            .output()
            .expect("avmdec not found");
        assert!(
            output.status.success(),
            "avmdec failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let ref_y4m = std::fs::read(ref_path).unwrap();
        let y4m_header = format!("YUV4MPEG2 W{} H{} F0:0 Ip C420jpeg\nFRAME\n", w, h);
        let hlen = y4m_header.len();
        let ref_y = &ref_y4m[hlen..hlen + w * h];

        let y_mse: f64 = frame
            .y
            .iter()
            .zip(ref_y.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w * h) as f64;
        let y_psnr = if y_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / y_mse).log10()
        } else {
            100.0
        };

        let ref_u = &ref_y4m[hlen + w * h..hlen + w * h + w / 2 * h / 2];
        let ref_v = &ref_y4m[hlen + w * h + w / 2 * h / 2..hlen + w * h + 2 * (w / 2 * h / 2)];
        let u_mse: f64 = frame
            .u
            .iter()
            .zip(ref_u.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w / 2 * h / 2) as f64;
        let u_psnr = if u_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / u_mse).log10()
        } else {
            100.0
        };
        let v_mse: f64 = frame
            .v
            .iter()
            .zip(ref_v.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / (w / 2 * h / 2) as f64;
        let v_psnr = if v_mse > 0.0 {
            10.0 * (255.0f64 * 255.0 / v_mse).log10()
        } else {
            100.0
        };

        eprintln!("=== OVERALL PSNR ===");
        eprintln!(
            "NOIBC PSNR: Y={:.2} dB  U={:.2} dB  V={:.2} dB",
            y_psnr, u_psnr, v_psnr
        );
        eprintln!(
            "[ROW63] ours[0..32]={:?}",
            &frame.y[63 * w..63 * w + 32]
                .iter()
                .map(|&v| v)
                .collect::<Vec<u8>>()
        );
        eprintln!(
            "[ROW63] ours[32..64]={:?}",
            &frame.y[63 * w + 32..63 * w + 64]
                .iter()
                .map(|&v| v)
                .collect::<Vec<u8>>()
        );
        eprintln!(
            "[ROW63] ref[0..32]={:?}",
            &ref_y[63 * w..63 * w + 32]
                .iter()
                .map(|&v| v)
                .collect::<Vec<u8>>()
        );
        eprintln!(
            "[ROW63] ref[32..64]={:?}",
            &ref_y[63 * w + 32..63 * w + 64]
                .iter()
                .map(|&v| v)
                .collect::<Vec<u8>>()
        );
        eprintln!("Y MSE={:.4}  U MSE={:.4}  V MSE={:.4}", y_mse, u_mse, v_mse);

        eprintln!("\n=== PER-SB (64x64 quadrant) Y PSNR ===");
        let sb_regions = [
            ("Top-Left", 0usize, 0usize, 64usize, 64usize),
            ("Top-Right", 64, 0, 64, 64),
            ("Bottom-Left", 0, 64, 64, 64),
            ("Bottom-Right", 64, 64, 64, 64),
        ];
        for (name, sx, sy, sw, sh) in &sb_regions {
            let mut mse_sum = 0.0f64;
            let mut count = 0usize;
            let mut max_err = 0i32;
            let mut max_err_pos = (0usize, 0usize);
            for r in *sy..(*sy + *sh).min(h) {
                for c in *sx..(*sx + *sw).min(w) {
                    let ours = frame.y[r * w + c] as i32;
                    let refs = ref_y[r * w + c] as i32;
                    let diff = (ours - refs).abs();
                    mse_sum += (diff as f64).powi(2);
                    count += 1;
                    if diff > max_err {
                        max_err = diff;
                        max_err_pos = (c, r);
                    }
                }
            }
            let sb_mse = mse_sum / count as f64;
            let sb_psnr = if sb_mse > 0.0 {
                10.0 * (255.0f64 * 255.0 / sb_mse).log10()
            } else {
                100.0
            };
            eprintln!(
                "  {} ({},{})-({},{}) : PSNR={:.2} dB  MSE={:.4}  max_err={} at ({},{})",
                name,
                sx,
                sy,
                sx + sw,
                sy + sh,
                sb_psnr,
                sb_mse,
                max_err,
                max_err_pos.0,
                max_err_pos.1
            );
        }

        eprintln!("\n=== PER-BLOCK Y ANALYSIS ===");
        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        let fp = frame::parse_frame_header(obus[1].payload, &seq).unwrap();
        let dequant = crate::encode::ac_qlookup(fp.base_qindex);
        let tile_cfg = tile::TileDecodeConfig {
            enable_cfl_intra: seq.enable_cfl_intra,
            enable_fsc: seq.enable_fsc,
            enable_idtx_intra: seq.enable_idtx_intra,
            enable_mrls: seq.enable_mrls,
            enable_ist: seq.enable_ist,
            enable_cctx: seq.enable_cctx,
            tx_mode_select: fp.tx_mode_select,
            reduced_tx_set_used: fp.reduced_tx_set_used,
            reduced_tx_part_set: seq.reduced_tx_part_set,
            allow_intrabc: fp.allow_intrabc,
            allow_screen_content_tools: fp.allow_screen_content_tools,
            enable_sdp: seq.enable_sdp,
            cdef_frame_enable: fp.cdef_frame_enable,
            nb_cdef_strengths: fp.nb_cdef_strengths,
            ccso_frame_enable: fp.ccso_frame_enable,
            ccso_enable: fp.ccso_enable,
            sb_size: seq.sb_size as usize,
            tcq_mode: fp.tcq_mode,
            allow_parity_hiding: fp.allow_parity_hiding,
            enable_ext_partitions: seq.enable_ext_partitions,
            enable_uneven_4way_partitions: seq.enable_uneven_4way_partitions,
            enable_mhccp: seq.enable_mhccp,
            enable_refmvbank: seq.enable_refmvbank,
            max_bvp_drl_bits: fp.max_bvp_drl_bits,
            cur_frame_force_integer_mv: fp.cur_frame_force_integer_mv,
            allow_intrabc_morph_pred: seq.enable_intra_bawp && fp.allow_screen_content_tools,
        };
        let blocks = tile::decode_tile_data_partitioned_cfg(
            fp.tile_data,
            w,
            h,
            seq.sb_size as usize,
            fp.base_qindex,
            &tile_cfg,
        );

        let mode_name = |joint: u8| -> &str {
            let (mode, delta) = predict::decode_prediction_mode(joint);
            match mode {
                predict::PredictionMode::DcPred => "DC",
                predict::PredictionMode::VPred => "V",
                predict::PredictionMode::HPred => "H",
                predict::PredictionMode::D45Pred => "D45",
                predict::PredictionMode::D135Pred => "D135",
                predict::PredictionMode::D113Pred => "D113",
                predict::PredictionMode::D157Pred => "D157",
                predict::PredictionMode::D203Pred => "D203",
                predict::PredictionMode::D67Pred => "D67",
                predict::PredictionMode::SmoothPred => "SMOOTH",
                predict::PredictionMode::SmoothVPred => "SMOOTH_V",
                predict::PredictionMode::SmoothHPred => "SMOOTH_H",
                predict::PredictionMode::PaethPred => "PAETH",
            }
        };

        let mut error_blocks: Vec<(usize, f64, i32)> = Vec::new();
        for (i, block) in blocks.iter().enumerate() {
            let bx = block.x;
            let by = block.y;
            let bw = block.width;
            let bh = block.height;
            let mut block_mse_sum = 0.0f64;
            let mut block_count = 0usize;
            let mut block_max_err = 0i32;
            let mut block_max_pos = (0usize, 0usize);

            for r in by..(by + bh).min(h) {
                for c in bx..(bx + bw).min(w) {
                    let ours = frame.y[r * w + c] as i32;
                    let refs = ref_y[r * w + c] as i32;
                    let diff = (ours - refs).abs();
                    block_mse_sum += (diff as f64).powi(2);
                    block_count += 1;
                    if diff > block_max_err {
                        block_max_err = diff;
                        block_max_pos = (c, r);
                    }
                }
            }
            let block_mse = if block_count > 0 {
                block_mse_sum / block_count as f64
            } else {
                0.0
            };
            if block_mse > 0.0 {
                error_blocks.push((i, block_mse, block_max_err));
            }
        }

        error_blocks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        eprintln!(
            "Total blocks: {}  Blocks with non-zero MSE: {}",
            blocks.len(),
            error_blocks.len()
        );

        for &(i, block_mse, max_err) in error_blocks.iter() {
            let block = &blocks[i];
            let bx = block.x;
            let by = block.y;
            let bw = block.width;
            let bh = block.height;
            let (mode, angle_delta) = predict::decode_prediction_mode(block.joint_y_mode);
            let y_eob: u32 = block.y_tu.iter().map(|t| t.eob as u32).sum();
            let is_palette = !block.palette_y_map.is_empty();
            let block_psnr = if block_mse > 0.0 {
                10.0 * (255.0f64 * 255.0 / block_mse).log10()
            } else {
                100.0
            };

            eprintln!(
                "  Block[{:3}] ({:3},{:3}) {:3}x{:3} mode={:<8} delta={:2} palette={} y_tu={} y_eob={:4} | MSE={:.2} PSNR={:.1}dB max_err={}",
                i,
                bx,
                by,
                bw,
                bh,
                mode_name(block.joint_y_mode),
                angle_delta,
                is_palette,
                block.y_tu.len(),
                y_eob,
                block_mse,
                block_psnr,
                max_err
            );

            if block_mse > 10.0 {
                eprintln!("    Sample pixels (first row of block):");
                let r = by;
                let end_c = (bx + bw).min(w).min(bx + 16);
                let ours_row: Vec<u8> = (bx..end_c).map(|c| frame.y[r * w + c]).collect();
                let refs_row: Vec<u8> = (bx..end_c).map(|c| ref_y[r * w + c]).collect();
                let diffs: Vec<i32> = (bx..end_c)
                    .map(|c| frame.y[r * w + c] as i32 - ref_y[r * w + c] as i32)
                    .collect();
                eprintln!("      ours: {:?}", ours_row);
                eprintln!("      ref:  {:?}", refs_row);
                eprintln!("      diff: {:?}", diffs);
                if bh > 1 && by + 1 < h {
                    let r2 = by + 1;
                    let ours_row2: Vec<u8> = (bx..end_c).map(|c| frame.y[r2 * w + c]).collect();
                    let refs_row2: Vec<u8> = (bx..end_c).map(|c| ref_y[r2 * w + c]).collect();
                    let diffs2: Vec<i32> = (bx..end_c)
                        .map(|c| frame.y[r2 * w + c] as i32 - ref_y[r2 * w + c] as i32)
                        .collect();
                    eprintln!("    Row+1:");
                    eprintln!("      ours: {:?}", ours_row2);
                    eprintln!("      ref:  {:?}", refs_row2);
                    eprintln!("      diff: {:?}", diffs2);
                }
            }
        }

        eprintln!("\n=== PER-ROW Y MSE (all 128 rows) ===");
        let mut worst_rows: Vec<(usize, f64)> = Vec::new();
        for row in 0..h {
            let row_mse: f64 = (0..w)
                .map(|c| {
                    let d = frame.y[row * w + c] as f64 - ref_y[row * w + c] as f64;
                    d * d
                })
                .sum::<f64>()
                / w as f64;
            if row_mse > 0.0 {
                worst_rows.push((row, row_mse));
            }
        }
        worst_rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for &(row, row_mse) in worst_rows.iter().take(20) {
            let row_psnr = if row_mse > 0.0 {
                10.0 * (255.0f64 * 255.0 / row_mse).log10()
            } else {
                100.0
            };
            eprintln!(
                "  Row {:3}: MSE={:.2}  PSNR={:.1}dB",
                row, row_mse, row_psnr
            );
        }
        if worst_rows.len() > 20 {
            eprintln!(
                "  ... and {} more rows with non-zero MSE",
                worst_rows.len() - 20
            );
        }

        eprintln!("\n=== PER-BLOCK UV ANALYSIS (worst 20) ===");
        let uv_w = w / 2;
        let uv_h = h / 2;
        let mut uv_error_blocks: Vec<(usize, f64, f64, &str)> = Vec::new();
        for (i, block) in blocks.iter().enumerate() {
            let uv_bx = block.x / 2;
            let uv_by = block.y / 2;
            let uv_bw = block.width / 2;
            let uv_bh = block.height / 2;
            if uv_bw < 4 || uv_bh < 4 {
                continue;
            }

            let mut u_mse_sum = 0.0f64;
            let mut v_mse_sum = 0.0f64;
            let mut count = 0usize;
            for r in uv_by..(uv_by + uv_bh).min(uv_h) {
                for c in uv_bx..(uv_bx + uv_bw).min(uv_w) {
                    let ou = frame.u[r * uv_w + c] as f64;
                    let ru = ref_u[r * uv_w + c] as f64;
                    u_mse_sum += (ou - ru).powi(2);
                    let ov = frame.v[r * uv_w + c] as f64;
                    let rv = ref_v[r * uv_w + c] as f64;
                    v_mse_sum += (ov - rv).powi(2);
                    count += 1;
                }
            }
            let u_mse = if count > 0 {
                u_mse_sum / count as f64
            } else {
                0.0
            };
            let v_mse = if count > 0 {
                v_mse_sum / count as f64
            } else {
                0.0
            };
            let cfl_type = if block.uv_mode == 13 {
                "CFL"
            } else {
                "non-CFL"
            };
            if u_mse > 0.01 || v_mse > 0.01 {
                uv_error_blocks.push((i, u_mse, v_mse, cfl_type));
            }
        }
        uv_error_blocks.sort_by(|a, b| (b.1 + b.2).partial_cmp(&(a.1 + a.2)).unwrap());
        for &(i, u_mse, v_mse, cfl_type) in uv_error_blocks.iter().take(20) {
            let block = &blocks[i];
            let uv_mode_str = match block.uv_mode {
                0 => "DC",
                1 => "V",
                2 => "H",
                3 => "D45",
                4 => "D135",
                5 => "D113",
                6 => "D157",
                7 => "D203",
                8 => "D67",
                9 => "SMOOTH",
                10 => "SMOOTH_V",
                11 => "SMOOTH_H",
                12 => "PAETH",
                13 => "CFL",
                _ => "?",
            };
            let tree_str = match block.tree_type {
                tile::TreeType::Shared => "S",
                tile::TreeType::Luma => "L",
                tile::TreeType::Chroma => "C",
            };
            eprintln!(
                "  Block[{:3}] ({:3},{:3}) {:3}x{:3} tree={} uv_mode={:<8} {} cfl_idx={} mh_dir={} alpha=({},{}) | U_MSE={:.1} V_MSE={:.1} u_tu={} v_tu={}",
                i,
                block.x,
                block.y,
                block.width,
                block.height,
                tree_str,
                uv_mode_str,
                cfl_type,
                block.cfl_idx,
                block.mhccp_dir,
                block.cfl_alpha_u,
                block.cfl_alpha_v,
                u_mse,
                v_mse,
                block.u_tu.len(),
                block.v_tu.len()
            );
        }
        eprintln!("Total UV error blocks: {}", uv_error_blocks.len());
    }
}
