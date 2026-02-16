use crate::dct;
use crate::scan::DEFAULT_SCAN_32X32;

pub fn dequantize_coeff(level: i32, log_scale: u32, dequant: i32) -> i32 {
    let abs_level = level.unsigned_abs() as i64;
    let dq = dequant as i64;
    let dqcoeff = ((abs_level * dq + 4) >> 3) >> log_scale;
    if level < 0 {
        -(dqcoeff as i32)
    } else {
        dqcoeff as i32
    }
}

pub fn dequantize_block(
    levels: &[i32; 1024],
    eob: u16,
    dequant: i32,
    log_scale: u32,
) -> [i32; 1024] {
    let mut coeffs = [0i32; 1024];
    for scan_idx in 0..(eob as usize) {
        let coeff_idx = DEFAULT_SCAN_32X32[scan_idx] as usize;
        coeffs[coeff_idx] = dequantize_coeff(levels[scan_idx], log_scale, dequant);
    }
    coeffs
}

pub fn dequantize_and_inverse(
    levels: &[i32; 1024],
    eob: u16,
    dequant: i32,
    log_scale: u32,
    is_64x64: bool,
) -> Vec<i32> {
    if eob == 0 {
        if is_64x64 {
            return vec![0i32; 4096];
        } else {
            return vec![0i32; 1024];
        }
    }

    let coeffs = dequantize_block(levels, eob, dequant, log_scale);

    if is_64x64 {
        let residuals = dct::inverse_dct2_64x64(&coeffs);
        residuals.to_vec()
    } else {
        let residuals = dct::inverse_dct2_32x32(&coeffs);
        residuals.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dct;
    use crate::encode::{ac_qlookup, compute_residual_block, quantize_block};

    #[test]
    fn dequantize_dc_roundtrip() {
        let dqcoeff = dequantize_coeff(524, 2, 164);
        let expected = ((524i64 * 164 + 4) >> 3) >> 2;
        assert_eq!(dqcoeff as i64, expected);
    }

    #[test]
    fn dequantize_zero() {
        assert_eq!(dequantize_coeff(0, 2, 164), 0);
        assert_eq!(dequantize_coeff(0, 1, 164), 0);
        assert_eq!(dequantize_coeff(0, 0, 100), 0);
    }

    #[test]
    fn dequantize_negative() {
        let pos = dequantize_coeff(524, 2, 164);
        let neg = dequantize_coeff(-524, 2, 164);
        assert!(pos > 0);
        assert_eq!(neg, -pos);
    }

    #[test]
    fn dequantize_block_solid_32x32() {
        let pixel_val = 200u8;
        let pixels = vec![pixel_val; 32 * 32];
        let pred_block = vec![128u8; 32 * 32];
        let residual = compute_residual_block(&pixels, &pred_block);
        let mut residual_arr = [0i32; 1024];
        residual_arr.copy_from_slice(&residual);
        let dct_coeffs = dct::forward_dct2_32x32(&residual_arr);
        let dequant = ac_qlookup(50);
        let (levels, eob) = quantize_block(&dct_coeffs, dequant, 1);

        let reconstructed = dequantize_and_inverse(&levels, eob, dequant, 1, false);
        assert_eq!(reconstructed.len(), 1024);

        let expected_residual = pixel_val as i32 - 128;
        for (i, &r) in reconstructed.iter().enumerate() {
            let reconstructed_pixel = (r + 128).clamp(0, 255);
            assert!(
                (reconstructed_pixel - pixel_val as i32).abs() <= 1,
                "pixel {} = {}, expected ~{}",
                i,
                reconstructed_pixel,
                pixel_val
            );
            let _ = expected_residual;
        }
    }

    #[test]
    fn dequantize_block_solid_64x64() {
        let pixel_val = 149u8;
        let pixels = vec![pixel_val; 64 * 64];
        let pred_block = vec![128u8; 64 * 64];
        let residual = compute_residual_block(&pixels, &pred_block);
        let mut residual_arr = [0i32; 4096];
        residual_arr.copy_from_slice(&residual);
        let dct_coeffs = dct::forward_dct2_64x64(&residual_arr);
        let dequant = ac_qlookup(50);
        let (levels, eob) = quantize_block(&dct_coeffs, dequant, 2);

        let reconstructed = dequantize_and_inverse(&levels, eob, dequant, 2, true);
        assert_eq!(reconstructed.len(), 4096);

        for (i, &r) in reconstructed.iter().enumerate() {
            let reconstructed_pixel = (r + 128).clamp(0, 255);
            assert!(
                (reconstructed_pixel - pixel_val as i32).abs() <= 1,
                "pixel {} = {}, expected ~{}",
                i,
                reconstructed_pixel,
                pixel_val
            );
        }
    }

    #[test]
    fn dequantize_block_zero_eob() {
        let levels = [0i32; 1024];
        let residuals_32 = dequantize_and_inverse(&levels, 0, 164, 1, false);
        assert_eq!(residuals_32.len(), 1024);
        assert!(residuals_32.iter().all(|&r| r == 0));

        let residuals_64 = dequantize_and_inverse(&levels, 0, 164, 2, true);
        assert_eq!(residuals_64.len(), 4096);
        assert!(residuals_64.iter().all(|&r| r == 0));
    }
}
