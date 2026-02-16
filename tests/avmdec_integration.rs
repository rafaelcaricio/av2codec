use std::fs;
use std::process::Command;

fn avmdec_available() -> bool {
    Command::new("../avm_build/avmdec")
        .arg("--help")
        .output()
        .is_ok()
}

fn decode_with_avmdec(obu_path: &str, out_path: &str) -> Vec<u8> {
    let output = Command::new("../avm_build/avmdec")
        .args(["--i420", "-o", out_path, obu_path])
        .output()
        .expect("avmdec not found");

    eprintln!("avmdec stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(
        output.status.success(),
        "avmdec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    fs::read(out_path).unwrap()
}

fn psnr(original: &[u8], decoded: &[u8]) -> f64 {
    assert_eq!(original.len(), decoded.len());
    let mse: f64 = original
        .iter()
        .zip(decoded.iter())
        .map(|(&a, &b)| {
            let d = a as f64 - b as f64;
            d * d
        })
        .sum::<f64>()
        / original.len() as f64;
    if mse == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0_f64 * 255.0 / mse).log10()
}

fn encode_y4m_fixture(fixture_path: &str, out_path: &str, qindex: u8) {
    let status = Command::new("cargo")
        .args([
            "run",
            "--",
            fixture_path,
            out_path,
            "--qindex",
            &qindex.to_string(),
        ])
        .status()
        .expect("failed to run wav2c CLI");
    assert!(status.success(), "wav2c CLI failed");
}

#[test]
#[ignore]
fn avmdec_decodes_single_frame_midgray() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let bs = wav2c::encode::encode_still_image(64, 64, 50);
    fs::write("/tmp/wav2c_test_single.obu", &bs).unwrap();

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_single.obu",
        "/tmp/wav2c_test_single_out.yuv",
    );

    let frame_size = 64 * 64 + 32 * 32 * 2;
    assert_eq!(decoded.len(), frame_size, "Expected 1 decoded I420 frame");
    assert_eq!(decoded[0], 128, "Mid-gray frame should decode to Y=128");
}

#[test]
#[ignore]
fn avmdec_decodes_multi_frame_midgray() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let frames: Vec<Vec<u8>> = (0..3).map(|_| vec![128; 64 * 64]).collect();
    let bs = wav2c::encode::encode_video(64, 64, 50, &frames);
    fs::write("/tmp/wav2c_test_multi.obu", &bs).unwrap();

    let decoded = decode_with_avmdec("/tmp/wav2c_test_multi.obu", "/tmp/wav2c_test_multi_out.yuv");

    let frame_size = 64 * 64 + 32 * 32 * 2;
    assert_eq!(decoded.len(), frame_size * 3, "Expected 3 decoded frames");

    for f in 0..3 {
        let y_start = f * frame_size;
        assert_eq!(decoded[y_start], 128, "Frame {} Y should be 128", f);
    }
}

#[test]
#[ignore]
fn avmdec_decodes_solid_green_all_planes() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let y_plane = vec![149u8; 64 * 64];
    let u_plane = vec![43u8; 32 * 32];
    let v_plane = vec![21u8; 32 * 32];
    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &[(y_plane, u_plane, v_plane)]);
    fs::write("/tmp/wav2c_test_green.obu", &bs).unwrap();

    let decoded = decode_with_avmdec("/tmp/wav2c_test_green.obu", "/tmp/wav2c_test_green_out.yuv");

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    assert_eq!(decoded.len(), y_size + 2 * uv_size);

    let y_pixel = decoded[0];
    let u_pixel = decoded[y_size];
    let v_pixel = decoded[y_size + uv_size];

    eprintln!(
        "Input Y=149 U=43 V=21, Decoded Y={} U={} V={}",
        y_pixel, u_pixel, v_pixel
    );

    assert_eq!(y_pixel, 149, "Y pixel should be 149, got {}", y_pixel);
    assert_eq!(u_pixel, 43, "U pixel should be 43, got {}", u_pixel);
    assert_eq!(v_pixel, 21, "V pixel should be 21, got {}", v_pixel);
}

#[test]
#[ignore]
fn avmdec_decodes_solid_red_all_planes() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let y_val: u8 = 81;
    let u_val: u8 = 90;
    let v_val: u8 = 240;
    let y_plane = vec![y_val; 64 * 64];
    let u_plane = vec![u_val; 32 * 32];
    let v_plane = vec![v_val; 32 * 32];
    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &[(y_plane, u_plane, v_plane)]);
    fs::write("/tmp/wav2c_test_red.obu", &bs).unwrap();

    let decoded = decode_with_avmdec("/tmp/wav2c_test_red.obu", "/tmp/wav2c_test_red_out.yuv");

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    assert_eq!(decoded.len(), y_size + 2 * uv_size);

    let dy = decoded[0];
    let du = decoded[y_size];
    let dv = decoded[y_size + uv_size];

    eprintln!(
        "Input Y={} U={} V={}, Decoded Y={} U={} V={}",
        y_val, u_val, v_val, dy, du, dv
    );

    assert_eq!(dy, y_val, "Y pixel mismatch");
    assert_eq!(du, u_val, "U pixel mismatch");
    assert_eq!(dv, v_val, "V pixel mismatch");
}

#[test]
#[ignore]
fn avmdec_decodes_multi_frame_yuv() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let colors: [(u8, u8, u8); 3] = [(149, 43, 21), (81, 90, 240), (128, 128, 128)];
    let frames: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = colors
        .iter()
        .map(|&(y, u, v)| (vec![y; 64 * 64], vec![u; 32 * 32], vec![v; 32 * 32]))
        .collect();

    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &frames);
    fs::write("/tmp/wav2c_test_multi_yuv.obu", &bs).unwrap();

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_multi_yuv.obu",
        "/tmp/wav2c_test_multi_yuv_out.yuv",
    );

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    let frame_size = y_size + 2 * uv_size;
    assert_eq!(decoded.len(), frame_size * 3, "Expected 3 decoded frames");

    for (f, &(ey, eu, ev)) in colors.iter().enumerate() {
        let base = f * frame_size;
        let dy = decoded[base];
        let du = decoded[base + y_size];
        let dv = decoded[base + y_size + uv_size];

        eprintln!(
            "Frame {}: expected Y={} U={} V={}, got Y={} U={} V={}",
            f, ey, eu, ev, dy, du, dv
        );

        assert_eq!(dy, ey, "Frame {} Y mismatch", f);
        assert_eq!(du, eu, "Frame {} U mismatch", f);
        assert_eq!(dv, ev, "Frame {} V mismatch", f);
    }
}

#[test]
#[ignore]
fn cli_encodes_y4m_fixture() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let status = Command::new("cargo")
        .args([
            "run",
            "--",
            "tests/fixtures/solid_green_64x64.y4m",
            "/tmp/wav2c_cli_fixture.obu",
            "--qindex",
            "50",
        ])
        .status()
        .expect("failed to run wav2c CLI");
    assert!(status.success(), "wav2c CLI failed");

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_cli_fixture.obu",
        "/tmp/wav2c_cli_fixture_out.yuv",
    );

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    assert_eq!(decoded.len(), y_size + 2 * uv_size);

    let y_pixel = decoded[0];
    eprintln!("CLI fixture: Decoded Y={}", y_pixel);
    let diff = (y_pixel as i32 - 149).abs();
    assert!(diff < 10, "Y pixel should be close to 149, got {}", y_pixel);
}

#[test]
#[ignore]
fn cli_encodes_multi_frame_y4m() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let status = Command::new("cargo")
        .args([
            "run",
            "--",
            "tests/fixtures/multi_gray_64x64_3f.y4m",
            "/tmp/wav2c_cli_multi_fix.obu",
            "--qindex",
            "50",
        ])
        .status()
        .expect("failed to run wav2c CLI");
    assert!(status.success(), "wav2c CLI failed");

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_cli_multi_fix.obu",
        "/tmp/wav2c_cli_multi_fix_out.yuv",
    );

    let frame_size = 64 * 64 + 32 * 32 * 2;
    assert_eq!(decoded.len(), frame_size * 3, "Expected 3 decoded frames");

    for f in 0..3 {
        let y_start = f * frame_size;
        assert_eq!(
            decoded[y_start], 126,
            "Frame {} Y should be 126 (ffmpeg gray)",
            f
        );
    }
}

#[test]
#[ignore]
fn avmdec_decodes_gradient() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    encode_y4m_fixture(
        "tests/fixtures/gradient_64x64.y4m",
        "/tmp/wav2c_test_gradient.obu",
        50,
    );

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_gradient.obu",
        "/tmp/wav2c_test_gradient_out.yuv",
    );

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    assert_eq!(decoded.len(), y_size + 2 * uv_size);

    let y_plane = &decoded[..y_size];
    let y_min = *y_plane.iter().min().unwrap();
    let y_max = *y_plane.iter().max().unwrap();
    eprintln!("Gradient decoded: Y range [{}, {}]", y_min, y_max);
    assert!(
        y_max - y_min > 20,
        "Gradient should have non-trivial Y range, got [{}, {}]",
        y_min,
        y_max
    );
}

#[test]
#[ignore]
fn avmdec_decodes_testsrc2() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    encode_y4m_fixture(
        "tests/fixtures/testsrc2_64x64.y4m",
        "/tmp/wav2c_test_testsrc2.obu",
        50,
    );

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_testsrc2.obu",
        "/tmp/wav2c_test_testsrc2_out.yuv",
    );

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    assert_eq!(decoded.len(), y_size + 2 * uv_size);

    let y_plane = &decoded[..y_size];
    let y_min = *y_plane.iter().min().unwrap();
    let y_max = *y_plane.iter().max().unwrap();
    eprintln!("Testsrc2 decoded: Y range [{}, {}]", y_min, y_max);
    assert!(
        y_max - y_min > 30,
        "Test pattern should have significant Y range, got [{}, {}]",
        y_min,
        y_max
    );
}

#[test]
#[ignore]
fn avmdec_gradient_psnr() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let mut reader = wav2c::y4m::Y4mReader::open("tests/fixtures/gradient_64x64.y4m").unwrap();
    let frame = reader.next_frame().unwrap();
    let original_y = frame.y.clone();
    let original_u = frame.u.clone();
    let original_v = frame.v.clone();

    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &[(frame.y, frame.u, frame.v)]);
    fs::write("/tmp/wav2c_test_gradient_psnr.obu", &bs).unwrap();

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_gradient_psnr.obu",
        "/tmp/wav2c_test_gradient_psnr_out.yuv",
    );

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    let decoded_y = &decoded[..y_size];
    let decoded_u = &decoded[y_size..y_size + uv_size];
    let decoded_v = &decoded[y_size + uv_size..];

    let y_psnr = psnr(&original_y, decoded_y);
    let u_psnr = psnr(&original_u, decoded_u);
    let v_psnr = psnr(&original_v, decoded_v);

    eprintln!(
        "Gradient PSNR: Y={:.1}dB U={:.1}dB V={:.1}dB",
        y_psnr, u_psnr, v_psnr
    );

    assert!(
        y_psnr > 20.0,
        "Y PSNR should be > 20dB for gradient at qindex=50, got {:.1}dB",
        y_psnr
    );
}

#[test]
#[ignore]
fn avmdec_decodes_128x128_solid_green() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let y_plane = vec![149u8; 128 * 128];
    let u_plane = vec![43u8; 64 * 64];
    let v_plane = vec![21u8; 64 * 64];
    let bs = wav2c::encode::encode_video_yuv(128, 128, 50, &[(y_plane, u_plane, v_plane)]);
    fs::write("/tmp/wav2c_test_128x128_green.obu", &bs).unwrap();

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_128x128_green.obu",
        "/tmp/wav2c_test_128x128_green_out.yuv",
    );

    let y_size = 128 * 128;
    let uv_size = 64 * 64;
    assert_eq!(decoded.len(), y_size + 2 * uv_size);

    let y_pixel = decoded[0];
    let u_pixel = decoded[y_size];
    let v_pixel = decoded[y_size + uv_size];

    eprintln!(
        "128x128 Input Y=149 U=43 V=21, Decoded Y={} U={} V={}",
        y_pixel, u_pixel, v_pixel
    );

    assert_eq!(y_pixel, 149, "Y pixel should be 149, got {}", y_pixel);
    assert_eq!(u_pixel, 43, "U pixel should be 43, got {}", u_pixel);
    assert_eq!(v_pixel, 21, "V pixel should be 21, got {}", v_pixel);
}

#[test]
#[ignore]
fn avmdec_decodes_256x128_solid_red() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let y_plane = vec![81u8; 256 * 128];
    let u_plane = vec![90u8; 128 * 64];
    let v_plane = vec![240u8; 128 * 64];
    let bs = wav2c::encode::encode_video_yuv(256, 128, 50, &[(y_plane, u_plane, v_plane)]);
    fs::write("/tmp/wav2c_test_256x128_red.obu", &bs).unwrap();

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_256x128_red.obu",
        "/tmp/wav2c_test_256x128_red_out.yuv",
    );

    let y_size = 256 * 128;
    let uv_size = 128 * 64;
    assert_eq!(decoded.len(), y_size + 2 * uv_size);

    let dy = decoded[0];
    let du = decoded[y_size];
    let dv = decoded[y_size + uv_size];

    eprintln!(
        "256x128 Input Y=81 U=90 V=240, Decoded Y={} U={} V={}",
        dy, du, dv
    );

    assert_eq!(dy, 81, "Y pixel mismatch");
    assert_eq!(du, 90, "U pixel mismatch");
    assert_eq!(dv, 240, "V pixel mismatch");
}

#[test]
#[ignore]
fn avmdec_testsrc2_psnr() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let mut reader = wav2c::y4m::Y4mReader::open("tests/fixtures/testsrc2_64x64.y4m").unwrap();
    let frame = reader.next_frame().unwrap();
    let original_y = frame.y.clone();
    let original_u = frame.u.clone();
    let original_v = frame.v.clone();

    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &[(frame.y, frame.u, frame.v)]);
    fs::write("/tmp/wav2c_test_testsrc2_psnr.obu", &bs).unwrap();

    let decoded = decode_with_avmdec(
        "/tmp/wav2c_test_testsrc2_psnr.obu",
        "/tmp/wav2c_test_testsrc2_psnr_out.yuv",
    );

    let y_size = 64 * 64;
    let uv_size = 32 * 32;
    let decoded_y = &decoded[..y_size];
    let decoded_u = &decoded[y_size..y_size + uv_size];
    let decoded_v = &decoded[y_size + uv_size..];

    let y_psnr = psnr(&original_y, decoded_y);
    let u_psnr = psnr(&original_u, decoded_u);
    let v_psnr = psnr(&original_v, decoded_v);

    eprintln!(
        "Testsrc2 PSNR: Y={:.1}dB U={:.1}dB V={:.1}dB",
        y_psnr, u_psnr, v_psnr
    );

    assert!(
        y_psnr > 20.0,
        "Y PSNR should be > 20dB for testsrc2 at qindex=50, got {:.1}dB",
        y_psnr
    );
}
