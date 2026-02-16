# Arbitrary-Size Encoding Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Accept any frame dimensions that are multiples of 4 by internally padding to the next multiple of 64.

**Architecture:** Add a `pad_plane` helper and a `round_up_to_64` utility. Modify `Encoder` to store both original and padded dimensions. Pad planes in `send_frame` before passing to `encode_frame_yuv`. Update `encode_video_yuv` similarly. All internal encoding uses padded dimensions.

**Tech Stack:** Rust, cargo test, avmdec for integration testing

---

### Task 1: Add `round_up_to_64` and `pad_plane` helpers

**Files:**
- Modify: `src/encode.rs` (add two functions before `FrameEncodeParams`)

**Step 1: Write failing tests**

Add to `src/encode.rs` tests module:

```rust
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
    assert_eq!(padded[4], 128);       // right padding, first row
    assert_eq!(padded[64 * 4], 128);  // bottom padding
}

#[test]
fn pad_plane_preserves_original_pixels() {
    let mut plane = vec![0u8; 100 * 100];
    for i in 0..100 * 100 {
        plane[i] = (i % 256) as u8;
    }
    let padded = pad_plane(&plane, 100, 100, 128, 128);
    for row in 0..100 {
        for col in 0..100 {
            assert_eq!(
                padded[row * 128 + col],
                plane[row * 100 + col],
                "mismatch at ({}, {})", row, col
            );
        }
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test round_up_to_64 pad_plane -v`
Expected: FAIL — functions not defined

**Step 3: Implement the functions**

Add to `src/encode.rs` (before `struct FrameEncodeParams`):

```rust
fn round_up_to_64(dim: u32) -> u32 {
    (dim + 63) & !63
}

fn pad_plane(src: &[u8], orig_w: u32, orig_h: u32, padded_w: u32, padded_h: u32) -> Vec<u8> {
    let (ow, oh, pw, ph) = (orig_w as usize, orig_h as usize, padded_w as usize, padded_h as usize);
    let mut dst = vec![128u8; pw * ph];
    for row in 0..oh {
        dst[row * pw..row * pw + ow].copy_from_slice(&src[row * ow..row * ow + ow]);
    }
    dst
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test round_up_to_64 pad_plane -v`
Expected: all 5 tests PASS

**Step 5: Commit**

```bash
git add src/encode.rs
git commit -m "feat(encode): add round_up_to_64 and pad_plane helpers"
```

---

### Task 2: Update `Encoder` struct for dual dimensions

**Files:**
- Modify: `src/encode.rs` — `Encoder` struct, `new`, `send_frame`, `headers`, `width`, `height`

**Step 1: Write failing test**

Add to tests module:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test encoder_new_accepts encoder_new_rejects_non_multiple_of_4 encoder_width_height_return_original -v`
Expected: `encoder_new_accepts_multiple_of_4` FAILS (100 rejected by current validation)

**Step 3: Modify `Encoder`**

Update the struct to store padded dimensions:

```rust
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
```

Update `Encoder::new` — change validation to require multiples of 4, minimum 4:

```rust
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
```

Update `headers` to use padded dimensions:

```rust
pub fn headers(&self) -> Vec<u8> {
    let seq_config = SequenceHeaderConfig {
        width: self.padded_width,
        height: self.padded_height,
    };
    let seq_payload = write_sequence_header(&seq_config);
    ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload)
}
```

Update `send_frame` to validate against original dimensions and pad:

```rust
pub fn send_frame(&mut self, y: &[u8], u: &[u8], v: &[u8]) -> Result<(), EncodeError> {
    let y_size = (self.width * self.height) as usize;
    let uv_size = (self.width / 2 * self.height / 2) as usize;

    if y.len() != y_size {
        return Err(EncodeError::InvalidPlaneSize { plane: "Y", expected: y_size, got: y.len() });
    }
    if u.len() != uv_size {
        return Err(EncodeError::InvalidPlaneSize { plane: "U", expected: uv_size, got: u.len() });
    }
    if v.len() != uv_size {
        return Err(EncodeError::InvalidPlaneSize { plane: "V", expected: uv_size, got: v.len() });
    }

    let padded_y = pad_plane(y, self.width, self.height, self.padded_width, self.padded_height);
    let padded_u = pad_plane(u, self.width / 2, self.height / 2, self.padded_width / 2, self.padded_height / 2);
    let padded_v = pad_plane(v, self.width / 2, self.height / 2, self.padded_width / 2, self.padded_height / 2);

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
    self.pending_packet = Some(Packet { data: clk_obu, frame_number });

    Ok(())
}
```

**Step 4: Fix existing tests that test dimension rejection**

Update `encoder_new_invalid_dimensions` — change `100, 100` case to `101, 101` (100 is now valid):

```rust
#[test]
fn encoder_new_invalid_dimensions() {
    assert!(Encoder::new(101, 100, 50).is_err());
    assert!(Encoder::new(0, 64, 50).is_err());
    assert!(Encoder::new(3, 64, 50).is_err());
}
```

**Step 5: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 6: Commit**

```bash
git add src/encode.rs
git commit -m "feat(encode): accept arbitrary dimensions (multiples of 4) via padding"
```

---

### Task 3: Update `encode_video_yuv` and `encode_still_image` batch APIs

**Files:**
- Modify: `src/encode.rs` — `encode_video_yuv`, `encode_still_image`, `encode_video`

**Step 1: Write failing test**

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test encode_video_yuv_non_aligned -v`
Expected: FAIL — panic from SB grid computation (100/64 = 1, misses pixels 64-99)

**Step 3: Update batch APIs to pad**

Update `encode_video_yuv`:

```rust
pub fn encode_video_yuv(
    width: u32,
    height: u32,
    base_qindex: u8,
    frames: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
) -> Vec<u8> {
    let mut output = Vec::new();

    let pw = round_up_to_64(width);
    let ph = round_up_to_64(height);

    let seq_config = SequenceHeaderConfig { width: pw, height: ph };
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
```

Update `encode_still_image` and `encode_video` similarly — they use padded dimensions for headers and plane sizes. `encode_still_image` generates a midgray plane at padded size. `encode_video` generates planes at padded size.

**Step 4: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 5: Commit**

```bash
git add src/encode.rs
git commit -m "feat(encode): pad frames in batch encode APIs for arbitrary dimensions"
```

---

### Task 4: Update CLI to accept non-aligned Y4M input

**Files:**
- Modify: `src/bin/wav2c.rs` — no changes needed if Y4M reader provides correct dimensions and `encode_video_yuv` handles padding. Verify this works.

**Step 1: Create test Y4M with non-aligned dimensions**

```bash
ffmpeg -y -f lavfi -i "color=c=red:s=100x100:d=0.04" -pix_fmt yuv420p /tmp/test_100x100.y4m
```

**Step 2: Encode with wav2c**

```bash
cargo run -- /tmp/test_100x100.y4m /tmp/test_100x100.obu --qindex 50
```

Expected: succeeds, prints `Encoding 1 frame(s) at 100x100 qindex=50`

**Step 3: Decode with avmdec**

```bash
../avm_build/avmdec --codec=av2 -o /tmp/test_100x100_out.y4m /tmp/test_100x100.obu
```

Expected: succeeds, output is 128x128 (padded)

**Step 4: Verify output visually**

```bash
ffmpeg -i /tmp/test_100x100_out.y4m /tmp/test_100x100_out.png
```

The output should be a 128x128 image with red in the top-left 100x100 region and gray (128) padding on the right/bottom edges.

**Step 5: Commit (if any changes were needed)**

---

### Task 5: Add integration test for non-aligned dimensions

**Files:**
- Modify: `tests/avmdec_integration.rs`

**Step 1: Write integration test**

```rust
#[test]
#[ignore]
fn avmdec_decodes_100x100_solid_red() {
    let avmdec = find_avmdec();
    let w = 100u32;
    let h = 100u32;
    let y_plane = vec![82u8; (w * h) as usize];   // red Y
    let u_plane = vec![90u8; (w / 2 * h / 2) as usize];  // red U
    let v_plane = vec![240u8; (w / 2 * h / 2) as usize]; // red V

    let bs = wav2c::encode::encode_video_yuv(w, h, 50, &[(y_plane, u_plane, v_plane)]);
    fs::write("/tmp/wav2c_test_100x100.obu", &bs).unwrap();

    let decoded = decode_with_avmdec(
        &avmdec,
        "/tmp/wav2c_test_100x100.obu",
        "/tmp/wav2c_test_100x100_out.yuv",
        128, 128,  // padded dimensions in output
    );

    // Verify the top-left 100x100 region of Y is close to 82
    let padded_w = 128usize;
    for row in 0..100 {
        for col in 0..100 {
            let pixel = decoded[row * padded_w + col];
            let diff = (pixel as i32 - 82).unsigned_abs();
            assert!(diff < 10, "Y pixel ({},{}) = {}, expected ~82", row, col, pixel);
        }
    }
}
```

**Step 2: Run integration test**

Run: `cargo test --test avmdec_integration avmdec_decodes_100x100 -- --ignored --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add src/encode.rs tests/avmdec_integration.rs
git commit -m "test: add integration test for non-aligned 100x100 frame encoding"
```

---

## Task Dependencies

```
Task 1 (helpers) → Task 2 (Encoder) → Task 3 (batch APIs) → Task 4 (CLI verify) → Task 5 (integration test)
```

All tasks are sequential — each builds on the previous.

## Verification Checklist

1. `cargo test` — all existing + new unit tests pass
2. `cargo clippy --tests -- -D warnings` — clean
3. CLI encodes 100x100 Y4M → avmdec decodes successfully
4. CLI encodes 1920x1080 Y4M → avmdec decodes successfully
5. All existing integration tests still pass
