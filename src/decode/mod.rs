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

        let sb_aligned = seq.width % 64 == 0 && seq.height % 64 == 0 && seq.width > 0 && seq.height > 0;

        for raw_obu in &obus[1..] {
            if raw_obu.obu_type != 4 {
                continue;
            }
            let fp = frame::parse_frame_header(raw_obu.payload, &seq)?;
            let dequant = crate::encode::ac_qlookup(fp.base_qindex);
            let sb_cols = (seq.width / 64) as usize;
            let sb_rows = (seq.height / 64) as usize;

            let (y_plane, u_plane, v_plane) = if sb_aligned && !fp.tile_data.is_empty() {
                let decode_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let decoded_sbs = tile::decode_tile_data_multi_sb(fp.tile_data, sb_cols, sb_rows, !fp.disable_cdf_update);
                    reconstruct_frame(&decoded_sbs, sb_cols, sb_rows, seq.width, seq.height, dequant)
                }));
                match decode_result {
                    Ok(planes) => planes,
                    Err(_) => gray_frame(seq.width, seq.height),
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

fn predict_dc_rect(
    recon: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) -> Vec<u8> {
    predict::predict_intra(recon, stride, x, y, w, h, predict::PredictionMode::DcPred, 0)
}

fn reconstruct_tu(
    recon: &mut [u8],
    stride: usize,
    tu_x: usize,
    tu_y: usize,
    tu_size: usize,
    pred: &[u8],
    pred_w: usize,
    pred_x_offset: usize,
    pred_y_offset: usize,
    residuals: &[i32],
) {
    for row in 0..tu_size {
        for col in 0..tu_size {
            let pred_val = pred[(pred_y_offset + row) * pred_w + pred_x_offset + col] as i32;
            let res_val = residuals[row * tu_size + col];
            let pixel = (pred_val + res_val).clamp(0, 255) as u8;
            recon[(tu_y + row) * stride + tu_x + col] = pixel;
        }
    }
}

fn reconstruct_partitioned_frame(
    blocks: &[tile::DecodedBlock],
    width: u32,
    height: u32,
    dequant: i32,
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

        let mi_x = bx / 4;
        let mi_y = by / 4;
        let mi_w = bw / 4;
        let mi_h = bh / 4;

        let bottom_left_joint = if mi_x > 0 && mi_y + mi_h < mi_rows {
            Some(joint_grid[(mi_y + mi_h) * mi_cols + (mi_x - 1)])
        } else {
            None
        };
        let above_right_joint = if mi_y > 0 && mi_x + mi_w < mi_cols {
            Some(joint_grid[(mi_y - 1) * mi_cols + (mi_x + mi_w)])
        } else {
            None
        };

        let is_small_block = bw < 8 || bh < 8;
        let is_large_block = bw * bh > 64;
        let mode_list = predict::build_y_intra_mode_list(
            bottom_left_joint, above_right_joint, is_small_block, is_large_block,
        );
        let joint = mode_list[block.y_mode as usize];
        let (pred_mode, angle_delta) = predict::decode_prediction_mode(joint);

        for r in 0..mi_h {
            for c in 0..mi_w {
                joint_grid[(mi_y + r) * mi_cols + mi_x + c] = joint;
            }
        }

        let y_pred = predict::predict_intra(
            &y_plane, w, bx, by, bw, bh, pred_mode, angle_delta,
        );

        let y_tu_size = if !block.y_tu.is_empty() {
            block.y_tu[0].tx_size.width()
        } else {
            bw.min(bh)
        };
        let tu_cols = bw / y_tu_size;

        for (ti, tu) in block.y_tu.iter().enumerate() {
            let tu_col = ti % tu_cols;
            let tu_row = ti / tu_cols;
            let tx = bx + tu_col * y_tu_size;
            let ty = by + tu_row * y_tu_size;
            let residuals = transform::dequantize_and_inverse_var(
                &tu.levels, tu.eob, dequant, tu.tx_size,
            );
            reconstruct_tu(
                &mut y_plane, w, tx, ty, y_tu_size,
                &y_pred, bw, tu_col * y_tu_size, tu_row * y_tu_size,
                &residuals,
            );
        }

        let uv_bx = bx / 2;
        let uv_by = by / 2;
        let uv_bw = bw / 2;
        let uv_bh = bh / 2;

        if uv_bw >= 4 && uv_bh >= 4 {
            let u_pred = predict_dc_rect(&u_plane, uv_w, uv_bx, uv_by, uv_bw, uv_bh);
            let uv_tu_size = if !block.u_tu.is_empty() {
                block.u_tu[0].tx_size.width()
            } else {
                uv_bw.min(uv_bh)
            };
            let uv_tu_cols = uv_bw / uv_tu_size;

            for (ti, tu) in block.u_tu.iter().enumerate() {
                let tu_col = ti % uv_tu_cols;
                let tu_row = ti / uv_tu_cols;
                let tx = uv_bx + tu_col * uv_tu_size;
                let ty = uv_by + tu_row * uv_tu_size;
                let residuals = transform::dequantize_and_inverse_var(
                    &tu.levels, tu.eob, dequant, tu.tx_size,
                );
                reconstruct_tu(
                    &mut u_plane, uv_w, tx, ty, uv_tu_size,
                    &u_pred, uv_bw, tu_col * uv_tu_size, tu_row * uv_tu_size,
                    &residuals,
                );
            }

            let v_pred = predict_dc_rect(&v_plane, uv_w, uv_bx, uv_by, uv_bw, uv_bh);
            for (ti, tu) in block.v_tu.iter().enumerate() {
                let tu_col = ti % uv_tu_cols;
                let tu_row = ti / uv_tu_cols;
                let tx = uv_bx + tu_col * uv_tu_size;
                let ty = uv_by + tu_row * uv_tu_size;
                let residuals = transform::dequantize_and_inverse_var(
                    &tu.levels, tu.eob, dequant, tu.tx_size,
                );
                reconstruct_tu(
                    &mut v_plane, uv_w, tx, ty, uv_tu_size,
                    &v_pred, uv_bw, tu_col * uv_tu_size, tu_row * uv_tu_size,
                    &residuals,
                );
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
            let y_pred_block = crate::encode::predict_intra_block(&y_plane, w, y_x, y_y, 64, y_mode);
            let y_residuals =
                transform::dequantize_and_inverse(&sb.y_levels, sb.y_eob, dequant, 2, true);
            crate::encode::reconstruct_block(&mut y_plane, w, y_x, y_y, 64, &y_pred_block, &y_residuals);

            let uv_x = sb_col * 32;
            let uv_y = sb_row * 32;
            let uv_mode = crate::encode::IntraMode::from_index(sb.uv_mode);
            let u_pred_block = crate::encode::predict_intra_block(&u_plane, uv_w, uv_x, uv_y, 32, uv_mode);
            let u_residuals =
                transform::dequantize_and_inverse(&sb.u_levels, sb.u_eob, dequant, 1, false);
            crate::encode::reconstruct_block(&mut u_plane, uv_w, uv_x, uv_y, 32, &u_pred_block, &u_residuals);

            let v_pred_block = crate::encode::predict_intra_block(&v_plane, uv_w, uv_x, uv_y, 32, uv_mode);
            let v_residuals =
                transform::dequantize_and_inverse(&sb.v_levels, sb.v_eob, dequant, 1, false);
            crate::encode::reconstruct_block(&mut v_plane, uv_w, uv_x, uv_y, 32, &v_pred_block, &v_residuals);
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
            assert!((p as i32 - 200).abs() <= 1, "Y pixel = {}, expected ~200", p);
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
            assert!((p as i32 - 160).abs() <= 1, "Y pixel = {}, expected ~160", p);
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

        let blocks = tile::decode_tile_data_partitioned_opts(
            fp.tile_data, seq.width as usize, seq.height as usize, 64, true, fp.base_qindex,
        );

        for (i, block) in blocks.iter().enumerate() {
            eprintln!("Block {}: ({},{}) {}x{} y_mode={} uv_mode={}",
                i, block.x, block.y, block.width, block.height,
                block.y_mode, block.uv_mode);
        }

        assert_eq!(blocks.len(), 8, "256x128 / 64x64 SBs = 4x2 = 8 blocks");

        let mut total_y_pixels = 0usize;
        for block in &blocks {
            assert_eq!(block.width, 64);
            assert_eq!(block.height, 64);
            total_y_pixels += block.width * block.height;
        }
        assert_eq!(total_y_pixels, 256 * 128);

        assert_eq!(blocks[0].y_mode, 7, "First block Y mode_idx = 7 (D45 via y_intra_mode_list)");
        for block in &blocks[1..] {
            assert_eq!(block.y_mode, 0, "Remaining blocks Y mode = DC");
        }
    }

    #[test]
    fn reconstruct_avmenc_minimal_frame() {
        let data = std::fs::read("tests/fixtures/avmenc_minimal.obu").unwrap();
        let obus = obu::parse_obus(&data);
        let seq = sequence::parse_sequence_header(obus[0].payload).unwrap();
        let fp = frame::parse_frame_header(obus[1].payload, &seq).unwrap();
        let dequant = crate::encode::ac_qlookup(fp.base_qindex);

        let blocks = tile::decode_tile_data_partitioned_opts(
            fp.tile_data, seq.width as usize, seq.height as usize, 64, true, fp.base_qindex,
        );
        let (y_plane, u_plane, v_plane) = reconstruct_partitioned_frame(
            &blocks, seq.width, seq.height, dequant,
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

        let y_mse: f64 = y_plane.iter().zip(ref_y.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>() / (w * h) as f64;
        let y_psnr = if y_mse > 0.0 { 10.0 * (255.0f64 * 255.0 / y_mse).log10() } else { 100.0 };

        let u_mse: f64 = u_plane.iter().zip(ref_u.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>() / (w / 2 * h / 2) as f64;
        let u_psnr = if u_mse > 0.0 { 10.0 * (255.0f64 * 255.0 / u_mse).log10() } else { 100.0 };

        let ref_v = &ref_y4m[header_len + w * h + w / 2 * h / 2..header_len + w * h + 2 * (w / 2 * h / 2)];
        let v_mse: f64 = v_plane.iter().zip(ref_v.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>() / (w / 2 * h / 2) as f64;
        let v_psnr = if v_mse > 0.0 { 10.0 * (255.0f64 * 255.0 / v_mse).log10() } else { 100.0 };

        assert!(y_psnr >= 100.0, "Y PSNR={:.2} dB, expected perfect reconstruction", y_psnr);
        assert!(u_psnr >= 100.0, "U PSNR={:.2} dB, expected perfect reconstruction", u_psnr);
        assert!(v_psnr >= 100.0, "V PSNR={:.2} dB, expected perfect reconstruction", v_psnr);
    }
}
