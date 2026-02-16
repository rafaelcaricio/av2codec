use std::fmt;

use crate::bitstream::{
    FrameHeaderConfig, ObuType, ObuWriter, SequenceHeaderConfig, write_clk_obu_payload,
    write_sequence_header,
};
use crate::dct;
use crate::scan::DEFAULT_SCAN_32X32;
use crate::tile;
use crate::tile::SbCoeffs;

#[derive(Debug)]
pub enum EncodeError {
    InvalidDimensions {
        width: u32,
        height: u32,
    },
    InvalidPlaneSize {
        plane: &'static str,
        expected: usize,
        got: usize,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::InvalidDimensions { width, height } => {
                write!(
                    f,
                    "invalid dimensions: {}x{} (must be multiples of 64)",
                    width, height
                )
            }
            EncodeError::InvalidPlaneSize {
                plane,
                expected,
                got,
            } => {
                write!(
                    f,
                    "{} plane size mismatch: expected {}, got {}",
                    plane, expected, got
                )
            }
        }
    }
}

impl std::error::Error for EncodeError {}

pub struct Packet {
    pub data: Vec<u8>,
    pub frame_number: u64,
}

pub struct Encoder {
    width: u32,
    height: u32,
    padded_width: u32,
    padded_height: u32,
    base_qindex: u8,
    dequant: i32,
    order_hint: u32,
    pending_packet: Option<Packet>,
}

impl Encoder {
    pub fn new(width: u32, height: u32, base_qindex: u8) -> Result<Self, EncodeError> {
        if width < 4 || height < 4 || !width.is_multiple_of(4) || !height.is_multiple_of(4) {
            return Err(EncodeError::InvalidDimensions { width, height });
        }
        Ok(Self {
            width,
            height,
            padded_width: round_up_to_64(width),
            padded_height: round_up_to_64(height),
            base_qindex,
            dequant: ac_qlookup(base_qindex),
            order_hint: 0,
            pending_packet: None,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn headers(&self) -> Vec<u8> {
        let seq_config = SequenceHeaderConfig {
            width: self.padded_width,
            height: self.padded_height,
        };
        let seq_payload = write_sequence_header(&seq_config);
        ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload)
    }

    pub fn send_frame(&mut self, y: &[u8], u: &[u8], v: &[u8]) -> Result<(), EncodeError> {
        let y_size = (self.width * self.height) as usize;
        let uv_size = (self.width / 2 * self.height / 2) as usize;

        if y.len() != y_size {
            return Err(EncodeError::InvalidPlaneSize {
                plane: "Y",
                expected: y_size,
                got: y.len(),
            });
        }
        if u.len() != uv_size {
            return Err(EncodeError::InvalidPlaneSize {
                plane: "U",
                expected: uv_size,
                got: u.len(),
            });
        }
        if v.len() != uv_size {
            return Err(EncodeError::InvalidPlaneSize {
                plane: "V",
                expected: uv_size,
                got: v.len(),
            });
        }

        let padded_y = pad_plane(y, self.width, self.height, self.padded_width, self.padded_height);
        let padded_u = pad_plane(
            u,
            self.width / 2,
            self.height / 2,
            self.padded_width / 2,
            self.padded_height / 2,
        );
        let padded_v = pad_plane(
            v,
            self.width / 2,
            self.height / 2,
            self.padded_width / 2,
            self.padded_height / 2,
        );

        let clk_obu = encode_frame_yuv(
            &padded_y,
            &padded_u,
            &padded_v,
            &FrameEncodeParams {
                width: self.padded_width,
                height: self.padded_height,
                dequant: self.dequant,
                base_qindex: self.base_qindex,
                order_hint: self.order_hint,
            },
        );

        let frame_number = self.order_hint as u64;
        self.order_hint += 1;
        self.pending_packet = Some(Packet {
            data: clk_obu,
            frame_number,
        });

        Ok(())
    }

    pub fn receive_packet(&mut self) -> Option<Packet> {
        self.pending_packet.take()
    }

    pub fn flush(&mut self) {
        // No-op for all-intra encoder — no buffered frames
    }
}

fn extract_subblock(
    plane: &[u8],
    frame_w: usize,
    x: usize,
    y: usize,
    bw: usize,
    bh: usize,
) -> Vec<u8> {
    let mut block = vec![0u8; bw * bh];
    for row in 0..bh {
        let src_offset = (y + row) * frame_w + x;
        let dst_offset = row * bw;
        block[dst_offset..dst_offset + bw].copy_from_slice(&plane[src_offset..src_offset + bw]);
    }
    block
}

fn round_up_to_64(dim: u32) -> u32 {
    (dim + 63) & !63
}

fn pad_plane(src: &[u8], orig_w: u32, orig_h: u32, padded_w: u32, padded_h: u32) -> Vec<u8> {
    let (ow, oh, pw, ph) = (
        orig_w as usize,
        orig_h as usize,
        padded_w as usize,
        padded_h as usize,
    );
    let mut dst = vec![128u8; pw * ph];
    for row in 0..oh {
        dst[row * pw..row * pw + ow].copy_from_slice(&src[row * ow..row * ow + ow]);
    }
    dst
}

struct FrameEncodeParams {
    width: u32,
    height: u32,
    dequant: i32,
    base_qindex: u8,
    order_hint: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum IntraMode {
    DcPred = 0,
    SmoothPred = 1,
    SmoothVPred = 2,
    SmoothHPred = 3,
    PaethPred = 4,
}

impl IntraMode {
    pub fn from_index(idx: u8) -> Self {
        match idx {
            0 => IntraMode::DcPred,
            1 => IntraMode::SmoothPred,
            2 => IntraMode::SmoothVPred,
            3 => IntraMode::SmoothHPred,
            4 => IntraMode::PaethPred,
            _ => IntraMode::DcPred,
        }
    }
}

const SMOOTH_WEIGHTS_32: [u16; 32] = [
    255, 240, 225, 210, 196, 182, 169, 157, 145, 133, 122, 111, 101, 92, 83, 74,
    66, 59, 52, 45, 39, 34, 29, 25, 21, 17, 14, 12, 10, 9, 8, 8,
];

const SMOOTH_WEIGHTS_64: [u16; 64] = [
    255, 248, 240, 233, 225, 218, 210, 203, 196, 189, 182, 176, 169, 163, 156, 150,
    144, 138, 133, 127, 121, 116, 111, 106, 101, 96, 91, 86, 82, 77, 73, 69,
    65, 61, 57, 54, 50, 47, 44, 41, 38, 35, 32, 29, 27, 25, 22, 20,
    18, 16, 15, 13, 12, 10, 9, 8, 7, 6, 6, 5, 5, 4, 4, 4,
];

fn smooth_weights(bsize: usize) -> &'static [u16] {
    match bsize {
        32 => &SMOOTH_WEIGHTS_32,
        64 => &SMOOTH_WEIGHTS_64,
        _ => panic!("unsupported block size for smooth weights: {}", bsize),
    }
}

struct RefPixels {
    above: Vec<u8>,
    left: Vec<u8>,
    top_left: u8,
    has_above: bool,
    has_left: bool,
}

fn get_ref_pixels(recon: &[u8], stride: usize, x: usize, y: usize, bsize: usize) -> RefPixels {
    let has_above = y > 0;
    let has_left = x > 0;

    match (has_above, has_left) {
        (false, false) => RefPixels {
            above: vec![127; bsize],
            left: vec![129; bsize],
            top_left: 128,
            has_above: false,
            has_left: false,
        },
        (true, false) => {
            let mut above = vec![0u8; bsize];
            for i in 0..bsize {
                above[i] = recon[(y - 1) * stride + x + i];
            }
            let tl = above[0];
            RefPixels {
                left: vec![tl; bsize],
                top_left: tl,
                above,
                has_above: true,
                has_left: false,
            }
        }
        (false, true) => {
            let mut left = vec![0u8; bsize];
            for j in 0..bsize {
                left[j] = recon[(y + j) * stride + x - 1];
            }
            let tl = left[0];
            RefPixels {
                above: vec![tl; bsize],
                top_left: tl,
                left,
                has_above: false,
                has_left: true,
            }
        }
        (true, true) => {
            let mut above = vec![0u8; bsize];
            for i in 0..bsize {
                above[i] = recon[(y - 1) * stride + x + i];
            }
            let mut left = vec![0u8; bsize];
            for j in 0..bsize {
                left[j] = recon[(y + j) * stride + x - 1];
            }
            let tl = recon[(y - 1) * stride + x - 1];
            RefPixels {
                above,
                left,
                top_left: tl,
                has_above: true,
                has_left: true,
            }
        }
    }
}

pub fn predict_dc(recon: &[u8], stride: usize, x: usize, y: usize, bsize: usize) -> u8 {
    let refs = get_ref_pixels(recon, stride, x, y, bsize);
    match (refs.has_above, refs.has_left) {
        (false, false) => 128,
        (true, false) => {
            let sum: u32 = refs.above.iter().map(|&v| v as u32).sum();
            ((sum + bsize as u32 / 2) / bsize as u32) as u8
        }
        (false, true) => {
            let sum: u32 = refs.left.iter().map(|&v| v as u32).sum();
            ((sum + bsize as u32 / 2) / bsize as u32) as u8
        }
        (true, true) => {
            let sum: u32 = refs.above.iter().map(|&v| v as u32).sum::<u32>()
                + refs.left.iter().map(|&v| v as u32).sum::<u32>();
            ((sum + bsize as u32) / (2 * bsize as u32)) as u8
        }
    }
}

fn predict_dc_block(refs: &RefPixels, bsize: usize) -> Vec<u8> {
    let val = match (refs.has_above, refs.has_left) {
        (false, false) => 128u8,
        (true, false) => {
            let sum: u32 = refs.above.iter().map(|&v| v as u32).sum();
            ((sum + bsize as u32 / 2) / bsize as u32) as u8
        }
        (false, true) => {
            let sum: u32 = refs.left.iter().map(|&v| v as u32).sum();
            ((sum + bsize as u32 / 2) / bsize as u32) as u8
        }
        (true, true) => {
            let sum: u32 = refs.above.iter().map(|&v| v as u32).sum::<u32>()
                + refs.left.iter().map(|&v| v as u32).sum::<u32>();
            ((sum + bsize as u32) / (2 * bsize as u32)) as u8
        }
    };
    vec![val; bsize * bsize]
}

fn predict_smooth_block(refs: &RefPixels, bsize: usize) -> Vec<u8> {
    let weights = smooth_weights(bsize);
    let below_pred = refs.left[bsize - 1] as u32;
    let right_pred = refs.above[bsize - 1] as u32;
    let mut pred = vec![0u8; bsize * bsize];
    for r in 0..bsize {
        for c in 0..bsize {
            let w_h = weights[r] as u32;
            let w_w = weights[c] as u32;
            let above = refs.above[c] as u32;
            let left = refs.left[r] as u32;
            let val = w_h * above + (256 - w_h) * below_pred
                + w_w * left + (256 - w_w) * right_pred;
            pred[r * bsize + c] = ((val + 256) >> 9) as u8;
        }
    }
    pred
}

fn predict_smooth_v_block(refs: &RefPixels, bsize: usize) -> Vec<u8> {
    let weights = smooth_weights(bsize);
    let below_pred = refs.left[bsize - 1] as u32;
    let mut pred = vec![0u8; bsize * bsize];
    for r in 0..bsize {
        let w = weights[r] as u32;
        for c in 0..bsize {
            let above = refs.above[c] as u32;
            let val = w * above + (256 - w) * below_pred;
            pred[r * bsize + c] = ((val + 128) >> 8) as u8;
        }
    }
    pred
}

fn predict_smooth_h_block(refs: &RefPixels, bsize: usize) -> Vec<u8> {
    let weights = smooth_weights(bsize);
    let right_pred = refs.above[bsize - 1] as u32;
    let mut pred = vec![0u8; bsize * bsize];
    for r in 0..bsize {
        let left = refs.left[r] as u32;
        for c in 0..bsize {
            let w = weights[c] as u32;
            let val = w * left + (256 - w) * right_pred;
            pred[r * bsize + c] = ((val + 128) >> 8) as u8;
        }
    }
    pred
}

fn predict_paeth_block(refs: &RefPixels, bsize: usize) -> Vec<u8> {
    let tl = refs.top_left as i32;
    let mut pred = vec![0u8; bsize * bsize];
    for r in 0..bsize {
        let left = refs.left[r] as i32;
        for c in 0..bsize {
            let above = refs.above[c] as i32;
            let base = above + left - tl;
            let p_left = (base - left).abs();
            let p_top = (base - above).abs();
            let p_tl = (base - tl).abs();
            pred[r * bsize + c] = if p_left <= p_top && p_left <= p_tl {
                refs.left[r]
            } else if p_top <= p_tl {
                refs.above[c]
            } else {
                refs.top_left
            };
        }
    }
    pred
}

fn predict_block(refs: &RefPixels, bsize: usize, mode: IntraMode) -> Vec<u8> {
    match mode {
        IntraMode::DcPred => predict_dc_block(refs, bsize),
        IntraMode::SmoothPred => predict_smooth_block(refs, bsize),
        IntraMode::SmoothVPred => predict_smooth_v_block(refs, bsize),
        IntraMode::SmoothHPred => predict_smooth_h_block(refs, bsize),
        IntraMode::PaethPred => predict_paeth_block(refs, bsize),
    }
}

pub fn predict_intra_block(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    bsize: usize,
    mode: IntraMode,
) -> Vec<u8> {
    let refs = get_ref_pixels(recon, stride, x, y, bsize);
    predict_block(&refs, bsize, mode)
}

const ALL_INTRA_MODES: [IntraMode; 5] = [
    IntraMode::DcPred,
    IntraMode::SmoothPred,
    IntraMode::SmoothVPred,
    IntraMode::SmoothHPred,
    IntraMode::PaethPred,
];

fn compute_sad(pixels: &[u8], pred: &[u8]) -> u64 {
    pixels
        .iter()
        .zip(pred.iter())
        .map(|(&p, &q)| (p as i32 - q as i32).unsigned_abs() as u64)
        .sum()
}

fn select_intra_mode(pixels: &[u8], refs: &RefPixels, bsize: usize) -> IntraMode {
    let mut best_mode = IntraMode::DcPred;
    let mut best_sad = u64::MAX;
    for &mode in &ALL_INTRA_MODES {
        let pred = predict_block(refs, bsize, mode);
        let sad = compute_sad(pixels, &pred);
        if sad < best_sad {
            best_sad = sad;
            best_mode = mode;
        }
    }
    best_mode
}

fn encode_plane_block_64(
    pixels: &[u8],
    plane_w: usize,
    recon: &mut [u8],
    x: usize,
    y: usize,
    dequant: i32,
) -> ([i32; 1024], u16, IntraMode) {
    let refs = get_ref_pixels(recon, plane_w, x, y, 64);
    let y_block = extract_subblock(pixels, plane_w, x, y, 64, 64);
    let mode = select_intra_mode(&y_block, &refs, 64);
    let pred_block = predict_block(&refs, 64, mode);

    let residual = compute_residual_block(&y_block, &pred_block);
    let mut residual_arr = [0i32; 4096];
    residual_arr.copy_from_slice(&residual);
    let dct_coeffs = dct::forward_dct2_64x64(&residual_arr);
    let (levels, eob) = quantize_block(&dct_coeffs, dequant, 2);

    let dqcoeffs = dequantize_block(&levels, eob, dequant, 2);
    let inv_residual = dct::inverse_dct2_64x64(&dqcoeffs);
    reconstruct_block(recon, plane_w, x, y, 64, &pred_block, &inv_residual);

    (levels, eob, mode)
}

fn encode_plane_block_32(
    pixels: &[u8],
    plane_w: usize,
    recon: &mut [u8],
    x: usize,
    y: usize,
    dequant: i32,
) -> ([i32; 1024], u16, IntraMode) {
    encode_plane_block_32_with_mode(pixels, plane_w, recon, x, y, dequant, None)
}

fn encode_plane_block_32_with_mode(
    pixels: &[u8],
    plane_w: usize,
    recon: &mut [u8],
    x: usize,
    y: usize,
    dequant: i32,
    forced_mode: Option<IntraMode>,
) -> ([i32; 1024], u16, IntraMode) {
    let refs = get_ref_pixels(recon, plane_w, x, y, 32);
    let uv_block = extract_subblock(pixels, plane_w, x, y, 32, 32);
    let mode = forced_mode.unwrap_or_else(|| select_intra_mode(&uv_block, &refs, 32));
    let pred_block = predict_block(&refs, 32, mode);

    let residual = compute_residual_block(&uv_block, &pred_block);
    let mut residual_arr = [0i32; 1024];
    residual_arr.copy_from_slice(&residual);
    let dct_coeffs = dct::forward_dct2_32x32(&residual_arr);
    let (levels, eob) = quantize_block(&dct_coeffs, dequant, 1);

    let dqcoeffs = dequantize_block(&levels, eob, dequant, 1);
    let inv_residual = dct::inverse_dct2_32x32(&dqcoeffs);
    reconstruct_block(recon, plane_w, x, y, 32, &pred_block, &inv_residual);

    (levels, eob, mode)
}

fn encode_frame_yuv(
    y_plane: &[u8],
    u_plane: &[u8],
    v_plane: &[u8],
    params: &FrameEncodeParams,
) -> Vec<u8> {
    let w = params.width as usize;
    let h = params.height as usize;
    let sb_cols = w / 64;
    let sb_rows = h / 64;
    let uv_w = w / 2;
    let uv_h = h / 2;

    let mut recon_y = vec![0u8; w * h];
    let mut recon_u = vec![0u8; uv_w * uv_h];
    let mut recon_v = vec![0u8; uv_w * uv_h];

    let mut sb_grid = Vec::with_capacity(sb_cols * sb_rows);
    for sb_row in 0..sb_rows {
        for sb_col in 0..sb_cols {
            let (y_levels, y_eob, y_mode) = encode_plane_block_64(
                y_plane,
                w,
                &mut recon_y,
                sb_col * 64,
                sb_row * 64,
                params.dequant,
            );

            let (u_levels, u_eob, u_mode) = encode_plane_block_32(
                u_plane,
                uv_w,
                &mut recon_u,
                sb_col * 32,
                sb_row * 32,
                params.dequant,
            );

            let (v_levels, v_eob, _v_mode) = encode_plane_block_32_with_mode(
                v_plane,
                uv_w,
                &mut recon_v,
                sb_col * 32,
                sb_row * 32,
                params.dequant,
                Some(u_mode),
            );

            sb_grid.push(SbCoeffs {
                y_levels,
                y_eob,
                u_levels,
                u_eob,
                v_levels,
                v_eob,
                y_mode: y_mode as u8,
                uv_mode: u_mode as u8,
            });
        }
    }

    let tile_data = tile::encode_tile_data_multi_sb(sb_cols, sb_rows, &sb_grid);
    let frame_config = FrameHeaderConfig {
        width: params.width,
        height: params.height,
        base_qindex: params.base_qindex,
        order_hint: params.order_hint,
    };
    let clk_payload = write_clk_obu_payload(&frame_config, &tile_data);
    ObuWriter::pack_obu(ObuType::Clk, &clk_payload)
}

pub fn encode_still_image(width: u32, height: u32, base_qindex: u8) -> Vec<u8> {
    let pw = round_up_to_64(width);
    let ph = round_up_to_64(height);
    encode_video(
        width,
        height,
        base_qindex,
        &[vec![128; (pw * ph) as usize]],
    )
}

pub fn encode_video(width: u32, height: u32, base_qindex: u8, y_planes: &[Vec<u8>]) -> Vec<u8> {
    let mut output = Vec::new();

    let pw = round_up_to_64(width);
    let ph = round_up_to_64(height);

    let seq_config = SequenceHeaderConfig {
        width: pw,
        height: ph,
    };
    let seq_payload = write_sequence_header(&seq_config);
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);
    output.extend_from_slice(&seq_obu);

    for (i, _y_plane) in y_planes.iter().enumerate() {
        let tile_data = tile::encode_tile_data();
        let frame_config = FrameHeaderConfig {
            width: pw,
            height: ph,
            base_qindex,
            order_hint: i as u32,
        };
        let clk_payload = write_clk_obu_payload(&frame_config, &tile_data);
        let clk_obu = ObuWriter::pack_obu(ObuType::Clk, &clk_payload);
        output.extend_from_slice(&clk_obu);
    }

    output
}

pub fn encode_video_yuv(
    width: u32,
    height: u32,
    base_qindex: u8,
    frames: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
) -> Vec<u8> {
    let mut output = Vec::new();

    let pw = round_up_to_64(width);
    let ph = round_up_to_64(height);

    let seq_config = SequenceHeaderConfig {
        width: pw,
        height: ph,
    };
    let seq_payload = write_sequence_header(&seq_config);
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);
    output.extend_from_slice(&seq_obu);

    let dequant = ac_qlookup(base_qindex);

    for (i, (y_plane, u_plane, v_plane)) in frames.iter().enumerate() {
        let padded_y = pad_plane(y_plane, width, height, pw, ph);
        let padded_u = pad_plane(u_plane, width / 2, height / 2, pw / 2, ph / 2);
        let padded_v = pad_plane(v_plane, width / 2, height / 2, pw / 2, ph / 2);

        let clk_obu = encode_frame_yuv(
            &padded_y,
            &padded_u,
            &padded_v,
            &FrameEncodeParams {
                width: pw,
                height: ph,
                dequant,
                base_qindex,
                order_hint: i as u32,
            },
        );
        output.extend_from_slice(&clk_obu);
    }

    output
}

pub fn compute_residual_block(pixels: &[u8], pred_block: &[u8]) -> Vec<i32> {
    pixels
        .iter()
        .zip(pred_block.iter())
        .map(|(&p, &q)| p as i32 - q as i32)
        .collect()
}

pub fn quantize_coeff(coeff: i64, log_scale: u32, dequant: i32) -> i32 {
    let scaled = coeff << (3 + log_scale);
    if dequant == 0 {
        return scaled as i32;
    }
    let dq = dequant as i64;
    let sign = if scaled < 0 { -1i64 } else { 1i64 };
    (sign * ((scaled.abs() + dq / 2) / dq)) as i32
}

pub fn quantize_block(
    dct_coeffs: &[i32; 1024],
    dequant: i32,
    log_scale: u32,
) -> ([i32; 1024], u16) {
    let mut levels = [0i32; 1024];
    let mut eob = 0u16;

    for scan_idx in 0..1024 {
        let coeff_idx = DEFAULT_SCAN_32X32[scan_idx] as usize;
        let coeff = dct_coeffs[coeff_idx] as i64;
        let level = quantize_coeff(coeff, log_scale, dequant);
        levels[scan_idx] = level;
        if level != 0 {
            eob = scan_idx as u16 + 1;
        }
    }

    (levels, eob)
}

pub fn ac_qlookup(qindex: u8) -> i32 {
    #[rustfmt::skip]
    const AC_QLOOKUP_QTX: [i32; 25] = [
        64, 40, 41, 43, 44, 45, 47, 48, 49, 51, 52,
        54, 55, 57, 59, 60, 62, 64, 66, 68, 70, 72, 74, 76, 78,
    ];

    let q = qindex as i32;
    if q < 25 {
        AC_QLOOKUP_QTX[q as usize]
    } else {
        AC_QLOOKUP_QTX[(((q - 1) % 24) + 1) as usize] << ((q - 1) / 24)
    }
}

pub fn dequantize_coeff(level: i32, log_scale: u32, dequant: i32) -> i32 {
    let abs_level = level.unsigned_abs() as i64;
    let sign = if level < 0 { -1i64 } else { 1i64 };
    let dqcoeff = (((abs_level * dequant as i64 + 4) >> 3) >> log_scale) as i32;
    (sign * dqcoeff as i64) as i32
}

pub fn dequantize_block(
    levels: &[i32; 1024],
    eob: u16,
    dequant: i32,
    log_scale: u32,
) -> [i32; 1024] {
    let mut dqcoeffs = [0i32; 1024];
    for scan_idx in 0..eob as usize {
        let coeff_idx = DEFAULT_SCAN_32X32[scan_idx] as usize;
        dqcoeffs[coeff_idx] = dequantize_coeff(levels[scan_idx], log_scale, dequant);
    }
    dqcoeffs
}

pub fn reconstruct_block(
    recon: &mut [u8],
    stride: usize,
    x: usize,
    y: usize,
    bsize: usize,
    pred_block: &[u8],
    inv_residual: &[i32],
) {
    for row in 0..bsize {
        for col in 0..bsize {
            let pixel = (pred_block[row * bsize + col] as i32
                + inv_residual[row * bsize + col])
                .clamp(0, 255) as u8;
            recon[(y + row) * stride + x + col] = pixel;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quantize_dc(residual: i32, log_scale: u32, dequant: i32) -> i32 {
        let dc_coeff = 128i64 * residual as i64;
        quantize_coeff(dc_coeff, log_scale, dequant)
    }

    #[test]
    fn bitstream_starts_with_seq_header_obu() {
        let bs = encode_still_image(64, 64, 50);
        assert!(bs.len() > 13);
        let obu_type = (bs[1] >> 2) & 0x1F;
        assert_eq!(obu_type, 1, "first OBU should be SequenceHeader");
    }

    #[test]
    fn bitstream_contains_two_obus() {
        let bs = encode_still_image(64, 64, 50);
        let seq_content_size = bs[0] as usize;
        let second_obu_start = 1 + seq_content_size;
        let remaining = &bs[second_obu_start..];
        assert!(
            !remaining.is_empty(),
            "should have CLK OBU after seq header"
        );
        let clk_type = (remaining[1] >> 2) & 0x1F;
        assert_eq!(clk_type, 4, "second OBU should be CLK");
    }

    #[test]
    fn bitstream_reasonable_size() {
        let bs = encode_still_image(64, 64, 50);
        assert!(bs.len() <= 50, "bitstream too large: {} bytes", bs.len());
    }

    #[test]
    fn encode_multiple_frames() {
        let frames: Vec<Vec<u8>> = (0..3).map(|_| vec![128; 64 * 64]).collect();
        let bs = encode_video(64, 64, 50, &frames);
        let seq_header_obu_size = 1 + 1 + 17; // leb128 + header + payload
        let single_clk_obu_size = 1 + 1 + 4 + 4; // leb128 + header + frame_header + tile_data (approx)
        assert!(
            bs.len() > seq_header_obu_size + single_clk_obu_size,
            "bitstream too small: {} bytes",
            bs.len()
        );
    }

    #[test]
    fn encode_video_has_correct_obu_count() {
        let frames: Vec<Vec<u8>> = (0..5).map(|_| vec![128; 64 * 64]).collect();
        let bs = encode_video(64, 64, 50, &frames);
        let seq_content_size = bs[0] as usize;
        let mut pos = 1 + seq_content_size;
        let mut clk_count = 0;
        while pos < bs.len() {
            let obu_size = bs[pos] as usize;
            let obu_type = (bs[pos + 1] >> 2) & 0x1F;
            assert_eq!(obu_type, 4, "expected CLK OBU");
            clk_count += 1;
            pos += 1 + obu_size;
        }
        assert_eq!(clk_count, 5);
    }

    #[test]
    fn encode_frame_with_pixel_data() {
        let y_plane = vec![149u8; 64 * 64];
        let u_plane = vec![43u8; 32 * 32];
        let v_plane = vec![21u8; 32 * 32];
        let bs = encode_video_yuv(64, 64, 50, &[(y_plane, u_plane, v_plane)]);
        assert!(!bs.is_empty());
    }

    #[test]
    fn ac_qlookup_qindex_0() {
        assert_eq!(ac_qlookup(0), 64);
    }

    #[test]
    fn ac_qlookup_qindex_50() {
        let q = ac_qlookup(50);
        assert!(q > 0);
    }

    #[test]
    fn quantize_dc_zero_residual() {
        assert_eq!(quantize_dc(0, 2, 164), 0);
    }

    #[test]
    fn quantize_dc_positive_residual() {
        let level = quantize_dc(21, 2, 164);
        assert!(level > 0);
        assert_eq!(level, 524);
    }

    #[test]
    fn quantize_dc_matches_decoder_chain() {
        let level = quantize_dc(21, 2, 164);
        let dqcoeff = ((level as i64 * 164 + 4) >> 3) >> 2;
        let val = (64 * dqcoeff + 32) >> 6;
        let pixel_residual = (64 * val + 4096) >> 13;
        assert_eq!(pixel_residual, 21);
    }

    #[test]
    fn compute_residual_block_midgray() {
        let pixels = vec![128u8; 64];
        let residual = compute_residual_block(&pixels, &[128u8; 64]);
        assert!(residual.iter().all(|&r| r == 0));
    }

    #[test]
    fn compute_residual_block_values() {
        let pixels = vec![200u8; 4];
        let residual = compute_residual_block(&pixels, &[128u8; 4]);
        assert!(residual.iter().all(|&r| r == 72));
    }

    #[test]
    fn quantize_block_all_zero() {
        let dct_coeffs = [0i32; 1024];
        let (levels, eob) = quantize_block(&dct_coeffs, 164, 1);
        assert_eq!(eob, 0);
        assert!(levels.iter().all(|&l| l == 0));
    }

    #[test]
    fn quantize_block_dc_only() {
        let mut dct_coeffs = [0i32; 1024];
        dct_coeffs[0] = 128 * 21;
        let (levels, eob) = quantize_block(&dct_coeffs, 164, 1);
        assert_eq!(eob, 1);
        assert!(levels[0] != 0);
        assert!(levels[1..].iter().all(|&l| l == 0));
    }

    #[test]
    fn quantize_block_preserves_dc_value() {
        let dequant = ac_qlookup(50);
        let dc_from_old = quantize_dc(21, 2, dequant);
        let dc_coeff = 128 * 21;
        let dc_from_new = quantize_coeff(dc_coeff as i64, 2, dequant);
        assert_eq!(dc_from_old, dc_from_new);
    }

    #[test]
    fn solid_color_dct_produces_dc_only() {
        let pixels = vec![200u8; 32 * 32];
        let residual = compute_residual_block(&pixels, &vec![128u8; 32 * 32]);
        let mut residual_arr = [0i32; 1024];
        residual_arr.copy_from_slice(&residual);
        let dct_coeffs = crate::dct::forward_dct2_32x32(&residual_arr);
        let dequant = ac_qlookup(50);
        let (levels, eob) = quantize_block(&dct_coeffs, dequant, 1);
        assert_eq!(eob, 1);
        assert!(levels[0] != 0);
    }

    #[test]
    fn encoder_new_valid_dimensions() {
        let enc = Encoder::new(64, 64, 50);
        assert!(enc.is_ok());
    }

    #[test]
    fn encoder_new_128x128_valid() {
        let enc = Encoder::new(128, 128, 50);
        assert!(enc.is_ok());
    }

    #[test]
    fn encoder_new_invalid_dimensions() {
        assert!(Encoder::new(101, 100, 50).is_err());
        assert!(Encoder::new(0, 64, 50).is_err());
        assert!(Encoder::new(3, 64, 50).is_err());
    }

    #[test]
    fn encoder_new_accepts_multiple_of_4() {
        assert!(Encoder::new(100, 100, 50).is_ok());
        assert!(Encoder::new(1920, 1080, 50).is_ok());
        assert!(Encoder::new(4, 4, 50).is_ok());
        assert!(Encoder::new(640, 480, 50).is_ok());
    }

    #[test]
    fn encoder_new_rejects_non_multiple_of_4() {
        assert!(Encoder::new(101, 100, 50).is_err());
        assert!(Encoder::new(100, 3, 50).is_err());
        assert!(Encoder::new(0, 100, 50).is_err());
        assert!(Encoder::new(2, 2, 50).is_err());
    }

    #[test]
    fn encoder_width_height_return_original() {
        let enc = Encoder::new(100, 100, 50).unwrap();
        assert_eq!(enc.width(), 100);
        assert_eq!(enc.height(), 100);
    }

    #[test]
    fn encoder_headers_returns_sequence_header_obu() {
        let enc = Encoder::new(64, 64, 50).unwrap();
        let headers = enc.headers();
        assert!(!headers.is_empty());
        let obu_type = (headers[1] >> 2) & 0x1F;
        assert_eq!(obu_type, 1, "headers should be a SequenceHeader OBU");
    }

    #[test]
    fn encoder_send_frame_produces_packet() {
        let mut enc = Encoder::new(64, 64, 50).unwrap();
        let y = vec![149u8; 64 * 64];
        let u = vec![43u8; 32 * 32];
        let v = vec![21u8; 32 * 32];
        enc.send_frame(&y, &u, &v).unwrap();
        let pkt = enc.receive_packet();
        assert!(pkt.is_some());
        let pkt = pkt.unwrap();
        assert_eq!(pkt.frame_number, 0);
        assert!(!pkt.data.is_empty());
    }

    #[test]
    fn encoder_receive_without_send_returns_none() {
        let mut enc = Encoder::new(64, 64, 50).unwrap();
        assert!(enc.receive_packet().is_none());
    }

    #[test]
    fn encoder_multi_frame_increments_frame_number() {
        let mut enc = Encoder::new(64, 64, 50).unwrap();
        let y = vec![128u8; 64 * 64];
        let u = vec![128u8; 32 * 32];
        let v = vec![128u8; 32 * 32];

        for expected in 0..3u64 {
            enc.send_frame(&y, &u, &v).unwrap();
            let pkt = enc.receive_packet().unwrap();
            assert_eq!(pkt.frame_number, expected);
        }
    }

    #[test]
    fn encoder_flush_is_noop() {
        let mut enc = Encoder::new(64, 64, 50).unwrap();
        enc.flush();
        assert!(enc.receive_packet().is_none());
    }

    #[test]
    fn encoder_rejects_wrong_plane_size() {
        let mut enc = Encoder::new(64, 64, 50).unwrap();
        let y = vec![128u8; 100];
        let u = vec![128u8; 32 * 32];
        let v = vec![128u8; 32 * 32];
        assert!(enc.send_frame(&y, &u, &v).is_err());
    }

    #[test]
    fn encoder_streaming_matches_batch() {
        let y = vec![149u8; 64 * 64];
        let u = vec![43u8; 32 * 32];
        let v = vec![21u8; 32 * 32];

        let batch_bs = encode_video_yuv(64, 64, 50, &[(y.clone(), u.clone(), v.clone())]);
        let seq_content_size = batch_bs[0] as usize;
        let batch_clk = &batch_bs[1 + seq_content_size..];

        let mut enc = Encoder::new(64, 64, 50).unwrap();
        enc.send_frame(&y, &u, &v).unwrap();
        let pkt = enc.receive_packet().unwrap();

        assert_eq!(
            pkt.data, batch_clk,
            "streaming and batch should produce identical CLK OBUs"
        );
    }

    #[test]
    fn encoder_128x128_solid_color() {
        let mut enc = Encoder::new(128, 128, 50).unwrap();
        let y = vec![200u8; 128 * 128];
        let u = vec![100u8; 64 * 64];
        let v = vec![150u8; 64 * 64];
        enc.send_frame(&y, &u, &v).unwrap();
        let pkt = enc.receive_packet().unwrap();
        assert!(!pkt.data.is_empty());
        assert_eq!(pkt.frame_number, 0);
    }

    #[test]
    fn encoder_256x128_solid_color() {
        let mut enc = Encoder::new(256, 128, 50).unwrap();
        let y = vec![180u8; 256 * 128];
        let u = vec![60u8; 128 * 64];
        let v = vec![200u8; 128 * 64];
        enc.send_frame(&y, &u, &v).unwrap();
        let pkt = enc.receive_packet().unwrap();
        assert!(!pkt.data.is_empty());
    }

    #[test]
    fn encoder_128x128_produces_larger_bitstream() {
        let bs_64 = encode_video_yuv(
            64,
            64,
            50,
            &[(
                vec![149u8; 64 * 64],
                vec![43u8; 32 * 32],
                vec![21u8; 32 * 32],
            )],
        );
        let bs_128 = encode_video_yuv(
            128,
            128,
            50,
            &[(
                vec![149u8; 128 * 128],
                vec![43u8; 64 * 64],
                vec![21u8; 64 * 64],
            )],
        );
        assert!(
            bs_128.len() > bs_64.len(),
            "128x128 ({} bytes) should be larger than 64x64 ({} bytes)",
            bs_128.len(),
            bs_64.len()
        );
    }

    #[test]
    fn encoder_128x128_sb_grid_count() {
        let w = 128usize;
        let h = 128usize;
        let sb_cols = w / 64;
        let sb_rows = h / 64;
        assert_eq!(sb_cols * sb_rows, 4);
    }

    #[test]
    fn encoder_256x128_sb_grid_count() {
        let w = 256usize;
        let h = 128usize;
        let sb_cols = w / 64;
        let sb_rows = h / 64;
        assert_eq!(sb_cols * sb_rows, 8);
    }

    #[test]
    fn extract_subblock_full_frame() {
        let plane: Vec<u8> = (0..16).collect();
        let block = extract_subblock(&plane, 4, 0, 0, 4, 4);
        assert_eq!(block, plane);
    }

    #[test]
    fn extract_subblock_offset() {
        let plane: Vec<u8> = (0..64).collect();
        let block = extract_subblock(&plane, 8, 2, 1, 3, 2);
        assert_eq!(block, vec![10, 11, 12, 18, 19, 20]);
    }

    #[test]
    fn dequantize_coeff_roundtrip() {
        let dequant = ac_qlookup(50);
        let residual = 21i32;
        let dc_coeff = 128i64 * residual as i64;
        let level = quantize_coeff(dc_coeff, 2, dequant);
        let dqcoeff = dequantize_coeff(level, 2, dequant);
        let val = (64i64 * dqcoeff as i64 + 32) >> 6;
        let pixel_residual = ((64 * val + 4096) >> 13) as i32;
        assert_eq!(pixel_residual, residual);
    }

    #[test]
    fn dequantize_block_dc_only() {
        let dequant = ac_qlookup(50);
        let mut levels = [0i32; 1024];
        levels[0] = 524;
        let dqcoeffs = dequantize_block(&levels, 1, dequant, 2);
        assert_ne!(dqcoeffs[0], 0);
        assert!(dqcoeffs[1..].iter().all(|&c| c == 0));
    }

    #[test]
    fn reconstruct_block_basic() {
        let mut recon = vec![0u8; 4];
        let inv_residual = [10i32, 20, -5, 30];
        reconstruct_block(&mut recon, 2, 0, 0, 2, &[128u8; 4], &inv_residual);
        assert_eq!(recon, vec![138, 148, 123, 158]);
    }

    #[test]
    fn reconstruct_block_clamps() {
        let mut recon = vec![0u8; 4];
        let inv_residual = [200i32, -200, 0, 0];
        reconstruct_block(&mut recon, 2, 0, 0, 2, &[128u8; 4], &inv_residual);
        assert_eq!(recon[0], 255);
        assert_eq!(recon[1], 0);
    }

    #[test]
    fn compute_residual_block_custom_prediction() {
        let pixels = vec![200u8; 4];
        let residual = compute_residual_block(&pixels, &[150u8; 4]);
        assert!(residual.iter().all(|&r| r == 50));
    }

    #[test]
    fn encode_video_yuv_non_aligned() {
        let w = 100u32;
        let h = 100u32;
        let y = vec![200u8; (w * h) as usize];
        let u = vec![128u8; (w / 2 * h / 2) as usize];
        let v = vec![128u8; (w / 2 * h / 2) as usize];
        let bs = encode_video_yuv(w, h, 50, &[(y, u, v)]);
        assert!(!bs.is_empty());
    }

    #[test]
    fn round_up_to_64_exact() {
        assert_eq!(round_up_to_64(64), 64);
        assert_eq!(round_up_to_64(128), 128);
    }

    #[test]
    fn round_up_to_64_rounds_up() {
        assert_eq!(round_up_to_64(4), 64);
        assert_eq!(round_up_to_64(100), 128);
        assert_eq!(round_up_to_64(1080), 1088);
        assert_eq!(round_up_to_64(1920), 1920);
    }

    #[test]
    fn pad_plane_no_padding_needed() {
        let plane = vec![200u8; 64 * 64];
        let padded = pad_plane(&plane, 64, 64, 64, 64);
        assert_eq!(padded, plane);
    }

    #[test]
    fn pad_plane_pads_right_and_bottom() {
        let plane = vec![200u8; 4 * 4];
        let padded = pad_plane(&plane, 4, 4, 64, 64);
        assert_eq!(padded.len(), 64 * 64);
        assert_eq!(padded[0], 200);
        assert_eq!(padded[3], 200);
        assert_eq!(padded[4], 128);
        assert_eq!(padded[64 * 4], 128);
    }

    #[test]
    fn pad_plane_preserves_original_pixels() {
        let mut plane = vec![0u8; 100 * 100];
        for (i, pixel) in plane.iter_mut().enumerate() {
            *pixel = (i % 256) as u8;
        }
        let padded = pad_plane(&plane, 100, 100, 128, 128);
        for row in 0..100 {
            for col in 0..100 {
                assert_eq!(
                    padded[row * 128 + col],
                    plane[row * 100 + col],
                    "mismatch at ({}, {})",
                    row,
                    col
                );
            }
        }
    }

    #[test]
    fn intra_mode_values() {
        assert_eq!(IntraMode::DcPred as u8, 0);
        assert_eq!(IntraMode::SmoothPred as u8, 1);
        assert_eq!(IntraMode::SmoothVPred as u8, 2);
        assert_eq!(IntraMode::SmoothHPred as u8, 3);
        assert_eq!(IntraMode::PaethPred as u8, 4);
    }

    #[test]
    fn smooth_weights_32_endpoints() {
        assert_eq!(SMOOTH_WEIGHTS_32[0], 255);
        assert_eq!(SMOOTH_WEIGHTS_32[31], 8);
    }

    #[test]
    fn smooth_weights_64_endpoints() {
        assert_eq!(SMOOTH_WEIGHTS_64[0], 255);
        assert_eq!(SMOOTH_WEIGHTS_64[63], 4);
    }

    #[test]
    fn ref_pixels_no_neighbors() {
        let recon = vec![0u8; 64 * 64];
        let refs = get_ref_pixels(&recon, 64, 0, 0, 32);
        assert!(refs.above.iter().all(|&v| v == 127));
        assert!(refs.left.iter().all(|&v| v == 129));
        assert_eq!(refs.top_left, 128);
        assert!(!refs.has_above);
        assert!(!refs.has_left);
    }

    #[test]
    fn ref_pixels_above_only() {
        let mut recon = vec![0u8; 128 * 128];
        for i in 0..64 {
            recon[63 * 128 + i] = 200;
        }
        let refs = get_ref_pixels(&recon, 128, 0, 64, 64);
        assert!(refs.above.iter().all(|&v| v == 200));
        assert!(refs.left.iter().all(|&v| v == 200));
        assert_eq!(refs.top_left, 200);
        assert!(refs.has_above);
        assert!(!refs.has_left);
    }

    #[test]
    fn ref_pixels_left_only() {
        let mut recon = vec![0u8; 128 * 128];
        for j in 0..64 {
            recon[j * 128 + 63] = 150;
        }
        let refs = get_ref_pixels(&recon, 128, 64, 0, 64);
        assert!(refs.left.iter().all(|&v| v == 150));
        assert!(refs.above.iter().all(|&v| v == 150));
        assert_eq!(refs.top_left, 150);
        assert!(!refs.has_above);
        assert!(refs.has_left);
    }

    #[test]
    fn ref_pixels_both_neighbors() {
        let mut recon = vec![0u8; 128 * 128];
        for i in 0..64 {
            recon[63 * 128 + 64 + i] = 200;
        }
        for j in 0..64 {
            recon[(64 + j) * 128 + 63] = 100;
        }
        recon[63 * 128 + 63] = 180;
        let refs = get_ref_pixels(&recon, 128, 64, 64, 64);
        assert!(refs.above.iter().all(|&v| v == 200));
        assert!(refs.left.iter().all(|&v| v == 100));
        assert_eq!(refs.top_left, 180);
        assert!(refs.has_above);
        assert!(refs.has_left);
    }

    #[test]
    fn predict_dc_block_no_neighbors() {
        let refs = RefPixels {
            above: vec![127; 32],
            left: vec![129; 32],
            top_left: 128,
            has_above: false,
            has_left: false,
        };
        let pred = predict_dc_block(&refs, 32);
        assert!(pred.iter().all(|&v| v == 128));
    }

    #[test]
    fn predict_dc_block_both_neighbors() {
        let refs = RefPixels {
            above: vec![200; 32],
            left: vec![100; 32],
            top_left: 150,
            has_above: true,
            has_left: true,
        };
        let pred = predict_dc_block(&refs, 32);
        assert!(pred.iter().all(|&v| v == 150));
    }

    #[test]
    fn predict_smooth_no_neighbors_approx_128() {
        let refs = RefPixels {
            above: vec![127; 32],
            left: vec![129; 32],
            top_left: 128,
            has_above: false,
            has_left: false,
        };
        let pred = predict_smooth_block(&refs, 32);
        for &v in &pred {
            assert!((v as i32 - 128).abs() <= 1, "smooth pred {} too far from 128", v);
        }
    }

    #[test]
    fn predict_smooth_v_vertical_gradient() {
        let refs = RefPixels {
            above: vec![255; 32],
            left: vec![0; 32],
            top_left: 255,
            has_above: true,
            has_left: true,
        };
        let pred = predict_smooth_v_block(&refs, 32);
        assert!(pred[0] > pred[31 * 32], "should decrease top to bottom");
        assert!(pred[0] >= 254, "top row should be near 255, got {}", pred[0]);
    }

    #[test]
    fn predict_smooth_h_horizontal_gradient() {
        let refs = RefPixels {
            above: vec![0; 32],
            left: vec![255; 32],
            top_left: 255,
            has_above: true,
            has_left: true,
        };
        let pred = predict_smooth_h_block(&refs, 32);
        assert!(pred[0] > pred[31], "should decrease left to right");
        assert!(pred[0] >= 254, "left col should be near 255, got {}", pred[0]);
    }

    #[test]
    fn predict_paeth_flat_returns_dc() {
        let refs = RefPixels {
            above: vec![128; 32],
            left: vec![128; 32],
            top_left: 128,
            has_above: true,
            has_left: true,
        };
        let pred = predict_paeth_block(&refs, 32);
        assert!(pred.iter().all(|&v| v == 128));
    }

    #[test]
    fn predict_paeth_vertical_edge() {
        let refs = RefPixels {
            above: vec![200; 32],
            left: vec![100; 32],
            top_left: 200,
            has_above: true,
            has_left: true,
        };
        let pred = predict_paeth_block(&refs, 32);
        assert!(pred.iter().all(|&v| v == 100));
    }

    #[test]
    fn predict_paeth_horizontal_edge() {
        let refs = RefPixels {
            above: vec![100; 32],
            left: vec![200; 32],
            top_left: 200,
            has_above: true,
            has_left: true,
        };
        let pred = predict_paeth_block(&refs, 32);
        assert!(pred.iter().all(|&v| v == 100));
    }

    #[test]
    fn select_mode_flat_block_picks_dc() {
        let pixels = vec![150u8; 32 * 32];
        let refs = RefPixels {
            above: vec![150; 32],
            left: vec![150; 32],
            top_left: 150,
            has_above: true,
            has_left: true,
        };
        assert_eq!(select_intra_mode(&pixels, &refs, 32), IntraMode::DcPred);
    }

    #[test]
    fn select_mode_vertical_gradient_prefers_smooth_v() {
        let refs = RefPixels {
            above: vec![255; 32],
            left: vec![0; 32],
            top_left: 255,
            has_above: true,
            has_left: true,
        };
        let pred_sv = predict_smooth_v_block(&refs, 32);
        let mode = select_intra_mode(&pred_sv, &refs, 32);
        assert_eq!(mode, IntraMode::SmoothVPred);
    }

    #[test]
    fn compute_sad_identical() {
        let a = vec![100u8; 64];
        assert_eq!(compute_sad(&a, &a), 0);
    }

    #[test]
    fn compute_sad_known_value() {
        let a = vec![100u8; 4];
        let b = vec![110u8; 4];
        assert_eq!(compute_sad(&a, &b), 40);
    }
}
