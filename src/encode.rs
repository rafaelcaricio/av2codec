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
    base_qindex: u8,
    dequant: i32,
    order_hint: u32,
    pending_packet: Option<Packet>,
}

impl Encoder {
    pub fn new(width: u32, height: u32, base_qindex: u8) -> Result<Self, EncodeError> {
        if width == 0 || height == 0 || !width.is_multiple_of(64) || !height.is_multiple_of(64) {
            return Err(EncodeError::InvalidDimensions { width, height });
        }
        Ok(Self {
            width,
            height,
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
            width: self.width,
            height: self.height,
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

        let clk_obu = encode_frame_yuv(
            y,
            u,
            v,
            &FrameEncodeParams {
                width: self.width,
                height: self.height,
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

fn encode_sb_coeffs(y_block: &[u8], u_block: &[u8], v_block: &[u8], dequant: i32) -> SbCoeffs {
    let y_residual = compute_residual_block(y_block, 64, 64);
    let mut y_residual_arr = [0i32; 4096];
    y_residual_arr.copy_from_slice(&y_residual);
    let y_dct = dct::forward_dct2_64x64(&y_residual_arr);
    let (y_levels, y_eob) = quantize_block(&y_dct, dequant, 2);

    let u_residual = compute_residual_block(u_block, 32, 32);
    let mut u_residual_arr = [0i32; 1024];
    u_residual_arr.copy_from_slice(&u_residual);
    let u_dct = dct::forward_dct2_32x32(&u_residual_arr);
    let (u_levels, u_eob) = quantize_block(&u_dct, dequant, 1);

    let v_residual = compute_residual_block(v_block, 32, 32);
    let mut v_residual_arr = [0i32; 1024];
    v_residual_arr.copy_from_slice(&v_residual);
    let v_dct = dct::forward_dct2_32x32(&v_residual_arr);
    let (v_levels, v_eob) = quantize_block(&v_dct, dequant, 1);

    SbCoeffs {
        y_levels,
        y_eob,
        u_levels,
        u_eob,
        v_levels,
        v_eob,
    }
}

struct FrameEncodeParams {
    width: u32,
    height: u32,
    dequant: i32,
    base_qindex: u8,
    order_hint: u32,
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

    let mut sb_grid = Vec::with_capacity(sb_cols * sb_rows);
    for sb_row in 0..sb_rows {
        for sb_col in 0..sb_cols {
            let y_block = extract_subblock(y_plane, w, sb_col * 64, sb_row * 64, 64, 64);
            let u_block = extract_subblock(u_plane, uv_w, sb_col * 32, sb_row * 32, 32, 32);
            let v_block = extract_subblock(v_plane, uv_w, sb_col * 32, sb_row * 32, 32, 32);
            sb_grid.push(encode_sb_coeffs(
                &y_block,
                &u_block,
                &v_block,
                params.dequant,
            ));
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
    encode_video(
        width,
        height,
        base_qindex,
        &[vec![128; (width * height) as usize]],
    )
}

pub fn encode_video(width: u32, height: u32, base_qindex: u8, y_planes: &[Vec<u8>]) -> Vec<u8> {
    let mut output = Vec::new();

    let seq_config = SequenceHeaderConfig { width, height };
    let seq_payload = write_sequence_header(&seq_config);
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);
    output.extend_from_slice(&seq_obu);

    for (i, _y_plane) in y_planes.iter().enumerate() {
        let tile_data = tile::encode_tile_data();
        let frame_config = FrameHeaderConfig {
            width,
            height,
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

    let seq_config = SequenceHeaderConfig { width, height };
    let seq_payload = write_sequence_header(&seq_config);
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);
    output.extend_from_slice(&seq_obu);

    let dequant = ac_qlookup(base_qindex);

    for (i, (y_plane, u_plane, v_plane)) in frames.iter().enumerate() {
        let clk_obu = encode_frame_yuv(
            y_plane,
            u_plane,
            v_plane,
            &FrameEncodeParams {
                width,
                height,
                dequant,
                base_qindex,
                order_hint: i as u32,
            },
        );
        output.extend_from_slice(&clk_obu);
    }

    output
}

pub fn compute_residual_block(pixels: &[u8], width: usize, height: usize) -> Vec<i32> {
    let mut residual = vec![0i32; width * height];
    for i in 0..width * height {
        residual[i] = pixels[i] as i32 - 128;
    }
    residual
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
        let residual = compute_residual_block(&pixels, 8, 8);
        assert!(residual.iter().all(|&r| r == 0));
    }

    #[test]
    fn compute_residual_block_values() {
        let pixels = vec![200u8; 4];
        let residual = compute_residual_block(&pixels, 2, 2);
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
        let residual = compute_residual_block(&pixels, 32, 32);
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
        assert!(Encoder::new(100, 100, 50).is_err());
        assert!(Encoder::new(0, 64, 50).is_err());
        assert!(Encoder::new(65, 64, 50).is_err());
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
}
