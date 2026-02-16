pub mod bitreader;
pub mod entropy;
pub mod frame;
pub mod obu;
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

        for raw_obu in &obus[1..] {
            if raw_obu.obu_type != 4 {
                continue;
            }
            let fp = frame::parse_frame_header(raw_obu.payload, seq.width, seq.height)?;
            let dequant = crate::encode::ac_qlookup(fp.base_qindex);
            let sb_cols = (seq.width / 64) as usize;
            let sb_rows = (seq.height / 64) as usize;

            let decoded_sbs = tile::decode_tile_data_multi_sb(fp.tile_data, sb_cols, sb_rows);

            let (y_plane, u_plane, v_plane) =
                reconstruct_frame(&decoded_sbs, sb_cols, sb_rows, seq.width, seq.height, dequant);
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
            let y_pred = crate::encode::predict_dc(&y_plane, w, y_x, y_y, 64);
            let y_residuals =
                transform::dequantize_and_inverse(&sb.y_levels, sb.y_eob, dequant, 2, true);
            crate::encode::reconstruct_block(&mut y_plane, w, y_x, y_y, 64, y_pred, &y_residuals);

            let uv_x = sb_col * 32;
            let uv_y = sb_row * 32;
            let u_pred = crate::encode::predict_dc(&u_plane, uv_w, uv_x, uv_y, 32);
            let u_residuals =
                transform::dequantize_and_inverse(&sb.u_levels, sb.u_eob, dequant, 1, false);
            crate::encode::reconstruct_block(&mut u_plane, uv_w, uv_x, uv_y, 32, u_pred, &u_residuals);

            let v_pred = crate::encode::predict_dc(&v_plane, uv_w, uv_x, uv_y, 32);
            let v_residuals =
                transform::dequantize_and_inverse(&sb.v_levels, sb.v_eob, dequant, 1, false);
            crate::encode::reconstruct_block(&mut v_plane, uv_w, uv_x, uv_y, 32, v_pred, &v_residuals);
        }
    }

    (y_plane, u_plane, v_plane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::encode_video_yuv;

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
}
