use crate::block::TxSize;
use crate::dct;
use crate::scan::{get_scan, DEFAULT_SCAN_32X32};

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

pub fn dequantize_and_inverse_var(
    levels: &[i32],
    eob: u16,
    dequant: i32,
    tx_size: TxSize,
) -> Vec<i32> {
    let pixel_count = tx_size.width() * tx_size.height();

    if eob == 0 {
        return vec![0i32; pixel_count];
    }

    let num_coeffs = tx_size.num_coeffs();
    let log_scale = tx_size.log_scale();
    let scan = get_scan(tx_size);

    let mut dqcoeffs = vec![0i32; num_coeffs];
    for scan_idx in 0..(eob as usize) {
        let coeff_idx = scan[scan_idx] as usize;
        dqcoeffs[coeff_idx] = dequantize_coeff(levels[scan_idx], log_scale, dequant);
    }

    match tx_size {
        TxSize::Tx4x4 => {
            let mut arr = [0i32; 16];
            arr.copy_from_slice(&dqcoeffs);
            dct::inverse_dct2_4x4(&arr).to_vec()
        }
        TxSize::Tx8x8 => {
            let mut arr = [0i32; 64];
            arr.copy_from_slice(&dqcoeffs);
            dct::inverse_dct2_8x8(&arr).to_vec()
        }
        TxSize::Tx16x16 => {
            let mut arr = [0i32; 256];
            arr.copy_from_slice(&dqcoeffs);
            dct::inverse_dct2_16x16(&arr).to_vec()
        }
        TxSize::Tx32x32 => {
            let mut arr = [0i32; 1024];
            arr.copy_from_slice(&dqcoeffs);
            dct::inverse_dct2_32x32(&arr).to_vec()
        }
        TxSize::Tx64x64 => {
            let mut arr = [0i32; 1024];
            arr.copy_from_slice(&dqcoeffs);
            dct::inverse_dct2_64x64_av2(&arr).to_vec()
        }
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

    #[test]
    fn var_4x4_zero_eob() {
        let levels = vec![0i32; 16];
        let result = dequantize_and_inverse_var(&levels, 0, 164, TxSize::Tx4x4);
        assert_eq!(result.len(), 16);
        assert!(result.iter().all(|&v| v == 0));
    }

    #[test]
    fn var_8x8_zero_eob() {
        let levels = vec![0i32; 64];
        let result = dequantize_and_inverse_var(&levels, 0, 164, TxSize::Tx8x8);
        assert_eq!(result.len(), 64);
        assert!(result.iter().all(|&v| v == 0));
    }

    #[test]
    fn var_16x16_zero_eob() {
        let levels = vec![0i32; 256];
        let result = dequantize_and_inverse_var(&levels, 0, 164, TxSize::Tx16x16);
        assert_eq!(result.len(), 256);
        assert!(result.iter().all(|&v| v == 0));
    }

    #[test]
    fn var_32x32_matches_legacy() {
        let mut levels_arr = [0i32; 1024];
        levels_arr[0] = 100;
        let legacy = dequantize_and_inverse(&levels_arr, 1, 164, 1, false);
        let levels_vec: Vec<i32> = levels_arr.to_vec();
        let new = dequantize_and_inverse_var(&levels_vec, 1, 164, TxSize::Tx32x32);
        assert_eq!(legacy.len(), new.len());
        for (i, (&a, &b)) in legacy.iter().zip(new.iter()).enumerate() {
            assert_eq!(a, b, "mismatch at {}", i);
        }
    }

    #[test]
    fn var_64x64_matches_legacy() {
        let mut levels_arr = [0i32; 1024];
        levels_arr[0] = 500;
        let legacy = dequantize_and_inverse(&levels_arr, 1, 164, 2, true);
        let levels_vec: Vec<i32> = levels_arr.to_vec();
        let new = dequantize_and_inverse_var(&levels_vec, 1, 164, TxSize::Tx64x64);
        assert_eq!(legacy.len(), new.len());
        for (i, (&a, &b)) in legacy.iter().zip(new.iter()).enumerate() {
            assert_eq!(a, b, "mismatch at {}", i);
        }
    }

    #[test]
    fn var_32x32_multi_coeff_matches_legacy() {
        let mut levels_arr = [0i32; 1024];
        levels_arr[0] = 100;
        levels_arr[1] = -50;
        levels_arr[2] = 30;
        levels_arr[3] = -10;
        let legacy = dequantize_and_inverse(&levels_arr, 4, 164, 1, false);
        let levels_vec: Vec<i32> = levels_arr.to_vec();
        let new = dequantize_and_inverse_var(&levels_vec, 4, 164, TxSize::Tx32x32);
        assert_eq!(legacy.len(), new.len());
        for (i, (&a, &b)) in legacy.iter().zip(new.iter()).enumerate() {
            assert_eq!(a, b, "mismatch at {}", i);
        }
    }

    #[test]
    fn var_4x4_dc_produces_uniform() {
        let mut levels = vec![0i32; 16];
        levels[0] = 100;
        let result = dequantize_and_inverse_var(&levels, 1, 164, TxSize::Tx4x4);
        assert_eq!(result.len(), 16);
        let first = result[0];
        assert_ne!(first, 0);
        for &v in &result {
            assert!((v - first).abs() <= 1);
        }
    }

    #[test]
    fn var_8x8_dc_produces_uniform() {
        let mut levels = vec![0i32; 64];
        levels[0] = 100;
        let result = dequantize_and_inverse_var(&levels, 1, 164, TxSize::Tx8x8);
        assert_eq!(result.len(), 64);
        let first = result[0];
        assert_ne!(first, 0);
        for &v in &result {
            assert!((v - first).abs() <= 1);
        }
    }

    #[test]
    fn var_16x16_dc_produces_uniform() {
        let mut levels = vec![0i32; 256];
        levels[0] = 100;
        let result = dequantize_and_inverse_var(&levels, 1, 164, TxSize::Tx16x16);
        assert_eq!(result.len(), 256);
        let first = result[0];
        assert_ne!(first, 0);
        for &v in &result {
            assert!((v - first).abs() <= 1);
        }
    }
}
