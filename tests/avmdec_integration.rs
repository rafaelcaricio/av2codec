use std::fs;
use std::process::Command;

fn avmdec_available() -> bool {
    Command::new("../avm_build/avmdec")
        .arg("--help")
        .output()
        .is_ok()
}

#[test]
#[ignore]
fn avmdec_decodes_single_frame_midgray() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let bs = wav2c::encode::encode_still_image(64, 64, 50);
    let obu_path = "/tmp/wav2c_test_single.obu";
    let out_path = "/tmp/wav2c_test_single_out.yuv";
    fs::write(obu_path, &bs).unwrap();

    let output = Command::new("../avm_build/avmdec")
        .args(["--i420", "-o", out_path, obu_path])
        .output()
        .expect("avmdec not found");

    eprintln!("avmdec stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(
        output.status.success(),
        "avmdec failed with exit code: {:?}",
        output.status.code()
    );

    let decoded = fs::read(out_path).unwrap();
    let frame_size = 64 * 64 + 32 * 32 * 2;
    assert_eq!(decoded.len(), frame_size, "Expected 1 decoded I420 frame");

    let y_pixel = decoded[0];
    eprintln!("Decoded Y pixel: {}", y_pixel);
    assert_eq!(y_pixel, 128, "Mid-gray frame should decode to Y=128");
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

    let obu_path = "/tmp/wav2c_test_multi.obu";
    let out_path = "/tmp/wav2c_test_multi_out.yuv";
    fs::write(obu_path, &bs).unwrap();

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

    let decoded = fs::read(out_path).unwrap();
    let frame_size = 64 * 64 + 32 * 32 * 2;
    assert_eq!(decoded.len(), frame_size * 3, "Expected 3 decoded frames");
}

#[test]
#[ignore]
fn avmdec_decodes_solid_green_with_dc() {
    if !avmdec_available() {
        eprintln!("avmdec not found, skipping");
        return;
    }

    let y_plane = vec![149u8; 64 * 64];
    let u_plane = vec![43u8; 32 * 32];
    let v_plane = vec![21u8; 32 * 32];
    let bs = wav2c::encode::encode_video_yuv(64, 64, 50, &[(y_plane, u_plane, v_plane)]);

    let obu_path = "/tmp/wav2c_test_green.obu";
    let out_path = "/tmp/wav2c_test_green_out.yuv";
    fs::write(obu_path, &bs).unwrap();

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

    let decoded = fs::read(out_path).unwrap();
    let frame_size = 64 * 64 + 32 * 32 * 2;
    assert_eq!(decoded.len(), frame_size, "Expected 1 decoded frame");

    let y_pixel = decoded[0];
    eprintln!("Input Y=149, Decoded Y={}", y_pixel);
    let diff = (y_pixel as i32 - 149).abs();
    assert!(diff < 10, "Y pixel should be close to 149, got {}", y_pixel);
}
