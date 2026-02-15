# Phase 1: Multi-Frame All-Intra Encoding — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Encode 50 solid-color all-intra frames (2s @ 25fps) into an AV2 bitstream decodable by avmdec with correct pixel values.

**Architecture:** Switch from `single_picture_header_flag=1` (still picture) to `=0` (video mode). Sequence header emitted once, followed by N CLK OBUs. Y4M parser reads input frames. DC coefficient coding produces correct pixel reconstruction.

**Tech Stack:** Rust (edition 2024), no external dependencies. avmdec at `../avm_build/avmdec` for validation.

---

### Task 1: Y4M Parser

**Files:**
- Create: `src/y4m.rs`
- Modify: `src/lib.rs` (add `pub mod y4m;`)

**Step 1: Write the failing test**

Add to bottom of `src/y4m.rs`:

```rust
pub struct Y4mHeader {
    pub width: u32,
    pub height: u32,
    pub framerate_num: u32,
    pub framerate_den: u32,
}

pub struct Y4mFrame {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

pub struct Y4mReader {
    header: Y4mHeader,
    data: Vec<u8>,
    offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_y4m_header() {
        let path = "tests/fixtures/solid_green_64x64.y4m";
        let reader = Y4mReader::open(path).unwrap();
        assert_eq!(reader.header().width, 64);
        assert_eq!(reader.header().height, 64);
    }

    #[test]
    fn read_single_frame() {
        let path = "tests/fixtures/solid_green_64x64.y4m";
        let mut reader = Y4mReader::open(path).unwrap();
        let frame = reader.next_frame().unwrap();
        assert_eq!(frame.y.len(), 64 * 64);
        assert_eq!(frame.u.len(), 32 * 32);
        assert_eq!(frame.v.len(), 32 * 32);
    }

    #[test]
    fn solid_green_pixel_values() {
        let path = "tests/fixtures/solid_green_64x64.y4m";
        let mut reader = Y4mReader::open(path).unwrap();
        let frame = reader.next_frame().unwrap();
        // All Y pixels should be the same value (solid color)
        let y_val = frame.y[0];
        assert!(frame.y.iter().all(|&v| v == y_val));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test y4m -- --nocapture`
Expected: FAIL — methods not implemented

**Step 3: Write minimal implementation**

Implement `Y4mReader::open()`, `header()`, and `next_frame()` in `src/y4m.rs`:

- `open(path)`: Read file, parse header line (`YUV4MPEG2 W64 H64 F25:1 Ip C420jpeg`), store remaining data
- `header()`: Return `&Y4mHeader`
- `next_frame()`: Look for `FRAME\n` marker, read Y plane (w×h bytes), U plane (w/2×h/2), V plane (w/2×h/2), return `Option<Y4mFrame>`

Header parsing: split by space, match `W<n>`, `H<n>`, `F<num>:<den>`, ignore other tags.

**Step 4: Run test to verify it passes**

Run: `cargo test y4m -- --nocapture`
Expected: PASS (3 tests)

**Step 5: Commit**

```
git add src/y4m.rs src/lib.rs
git commit -m "feat(y4m): add Y4M file parser"
```

---

### Task 2: Sequence Header — Video Mode

**Files:**
- Modify: `src/bitstream/sequence.rs`

This is the largest task. We switch `single_picture_header_flag` from 1 to 0 and add all required video-mode fields.

**Step 1: Write the failing test**

Replace `sequence_header_bit_pattern_64x64` test with a new video-mode test:

```rust
#[test]
fn video_mode_sequence_header_starts_correctly() {
    let config = SequenceHeaderConfig { width: 64, height: 64 };
    let payload = write_sequence_header(&config);
    let bits: Vec<u8> = payload.iter()
        .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1))
        .collect();
    let mut pos = 0;
    let mut rb = |n: usize| -> u32 {
        let mut val = 0u32;
        for _ in 0..n { val = (val << 1) | bits[pos] as u32; pos += 1; }
        val
    };
    assert_eq!(rb(1), 1);  // UVLC(0)
    assert_eq!(rb(5), 0);  // profile = 0
    assert_eq!(rb(1), 0);  // single_picture_header_flag = 0 (VIDEO MODE)
    assert_eq!(rb(3), 0);  // seq_lcr_id = 0
    assert_eq!(rb(1), 0);  // still_picture = 0
    assert_eq!(rb(5), 0);  // level = 0
    // (no seq_tier since level < SEQ_LEVEL_4_0)
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test video_mode_sequence_header -- --nocapture`
Expected: FAIL — bit pattern still has single_picture_header_flag=1

**Step 3: Rewrite `write_sequence_header`**

Major changes to `src/bitstream/sequence.rs`:

1. Change `single_picture_header_flag` from `true` to `false`
2. After the flag, write: `seq_lcr_id` (3 bits, 0), `still_picture` (1 bit, 0)
3. After chroma/bitdepth, write: `seq_max_display_model_info_present_flag` (1 bit, 0), `decoder_model_info_present_flag` (1 bit, 0)
4. Write: `max_tlayer_id` (2 bits, 0), `max_mlayer_id` (3 bits, 0)
5. No layer dependency flags (both max_*layer_id == 0 → defaults to no dependency)
6. Rewrite `write_inter_group` for video mode — must include:
   - 4 motion mode bits (all 0: INTERINTRA, WARP_CAUSAL, WARP_DELTA, WARP_EXTEND)
   - `enable_masked_compound` (0)
   - `enable_ref_frame_mvs` (0)
   - `order_hint_bits_minus_1` (3 bits, value 5 → 6-bit order hints, covers 0-63)
   - `enable_refmvbank` (0)
   - DRL reorder (1 bit: disabled)
   - `enable_explicit_ref_frame_map` (0)
   - `dpb_size_flag` (0)
   - `number_of_bits_for_lt_frame_id` (3 bits, 0)
   - `def_max_drl_bits`: quniform(3, 0) — unchanged
   - `allow_frame_max_drl_bits` (0)
   - `def_max_bvp_drl_bits`: quniform(3, 0) — unchanged
   - `allow_frame_max_bvp_drl_bits` (0)
   - `num_same_ref_compound` (2 bits, 0)
   - `enable_tip_select` (0)
   - `enable_mv_traj` (0)
   - `enable_bawp` (0) — unchanged
   - `enable_cwp` (0)
   - `enable_imp_msk_bld` (0)
   - `enable_lf_sub_pu` (0)
   - `enable_opfl_refine` (2 bits, 0)
   - `enable_refinemv` (0)
   - `enable_bru` (0)
   - `enable_adaptive_mvd` (0)
   - `enable_mvd_sign_derive` (0)
   - `enable_flex_mvres` (0)
   - `enable_global_motion` (0)
   - `enable_short_refresh_frame_flags` (0)
7. Add `write_scc_group`: `force_sct_select` (0), `force_screen_content_tools` (0) — 2 bits total
8. In `write_transform_quant_entropy_group`: add `enable_inter_ddt` (0), `enable_avg_cdf` (0)
9. In `write_filter_group`: add `cdef_on_skip_txfm` selection — write 1 bit (0) for disabled, then 1 bit (0)

**Step 4: Run tests to verify they pass**

Run: `cargo test sequence -- --nocapture`
Expected: PASS — update payload size test expectations as needed

**Step 5: Commit**

```
git add src/bitstream/sequence.rs
git commit -m "feat(sequence): switch to video mode (single_picture_header_flag=0)"
```

---

### Task 3: Frame Header — Video Mode

**Files:**
- Modify: `src/bitstream/frame.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn video_mode_frame_header_fields() {
    let config = FrameHeaderConfig { base_qindex: 50, order_hint: 0 };
    let mut w = BitWriter::new();
    w.write_bit(true); // first_tile_group
    write_uncompressed_header(&mut w, &config);
    // Video mode CLK: mfh_id, seq_header_id, long_term_id(0 bits),
    // immediate_output, frame_size_override, order_hint(6 bits), then rest
    assert!(w.total_bits_written() > 23); // More bits than still-picture mode
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test video_mode_frame_header -- --nocapture`
Expected: FAIL — FrameHeaderConfig doesn't have `order_hint` field

**Step 3: Update frame header**

Changes to `src/bitstream/frame.rs`:

1. Add `order_hint: u32` to `FrameHeaderConfig`
2. In `write_uncompressed_header`, after seq_header_id:
   - `long_term_id`: 0 bits (number_of_bits_for_lt_frame_id=0 from seq header)
   - `immediate_output_picture`: 1 bit (1 = show frame)
   - `frame_size_override_flag`: 1 bit (0)
   - `order_hint`: 6 bits (order_hint_bits_minus_1=5 → 6 bits)
   - `refresh_frame_flags`: 0 bits (implicit all-ones for CLK + max_mlayer_id=0)
3. Remove `write_screen_content_params` — force_screen_content_tools=0 at seq level means allow_screen_content_tools is implicit 0
4. Remove `write_intrabc_params` — allow_screen_content_tools=0 means intrabc is implicit 0
5. Keep: `disable_cdf_update`, `tile_info`, `quantization`, `segmentation`, `qm_params`, `loopfilter`, `tx_mode`, `reduced_tx_set`

**Step 4: Run tests to verify they pass**

Run: `cargo test frame -- --nocapture`
Expected: PASS — update bit count test expectations

**Step 5: Update encode.rs to pass order_hint**

Modify `encode_still_image` → `encode_frame` to accept frame index and pass it as `order_hint`.

**Step 6: Commit**

```
git add src/bitstream/frame.rs src/encode.rs
git commit -m "feat(frame): add video-mode CLK frame header with order_hint"
```

---

### Task 4: Multi-Frame Encoder

**Files:**
- Modify: `src/encode.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn encode_multiple_frames() {
    let frames: Vec<Vec<u8>> = (0..3).map(|_| vec![128; 64 * 64]).collect();
    let bs = encode_video(64, 64, 50, &frames);
    // Should have 1 seq header OBU + 3 CLK OBUs
    assert!(bs.len() > 19 * 3); // Bigger than 3x still-picture
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test encode_multiple -- --nocapture`
Expected: FAIL — `encode_video` doesn't exist

**Step 3: Implement `encode_video`**

```rust
pub fn encode_video(width: u32, height: u32, base_qindex: u8, y_planes: &[Vec<u8>]) -> Vec<u8> {
    let mut output = Vec::new();

    let seq_config = SequenceHeaderConfig { width, height };
    let seq_payload = write_sequence_header(&seq_config);
    let seq_obu = ObuWriter::pack_obu(ObuType::SequenceHeader, &seq_payload);
    output.extend_from_slice(&seq_obu);

    for (i, _y_plane) in y_planes.iter().enumerate() {
        let tile_data = encode_tile_data();
        let frame_config = FrameHeaderConfig {
            base_qindex,
            order_hint: i as u32,
        };
        let clk_payload = write_clk_obu_payload(&frame_config, &tile_data);
        let clk_obu = ObuWriter::pack_obu(ObuType::Clk, &clk_payload);
        output.extend_from_slice(&clk_obu);
    }

    output
}
```

Keep `encode_still_image` as a convenience wrapper calling `encode_video` with 1 frame.

**Step 4: Run tests to verify they pass**

Run: `cargo test encode -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
git add src/encode.rs
git commit -m "feat(encode): add multi-frame video encoding"
```

---

### Task 5: Validate Video-Mode Bitstream with avmdec

**Files:**
- Create: `tests/avmdec_integration.rs`

**Step 1: Write the integration test**

```rust
use std::process::Command;
use std::fs;

#[test]
#[ignore] // Run with: cargo test -- --ignored
fn avmdec_decodes_multi_frame_bitstream() {
    let frames: Vec<Vec<u8>> = (0..3).map(|_| vec![128; 64 * 64]).collect();
    let bs = wav2c::encode::encode_video(64, 64, 50, &frames);

    let obu_path = "/tmp/wav2c_test_multi.obu";
    let out_path = "/tmp/wav2c_test_multi_out.yuv";
    fs::write(obu_path, &bs).unwrap();

    let output = Command::new("../avm_build/avmdec")
        .args(["--i420", "-o", out_path, obu_path])
        .output()
        .expect("avmdec not found");

    assert!(output.status.success(), "avmdec failed: {}",
        String::from_utf8_lossy(&output.stderr));

    let decoded = fs::read(out_path).unwrap();
    let frame_size = 64 * 64 + 32 * 32 * 2; // I420
    assert_eq!(decoded.len(), frame_size * 3, "Expected 3 decoded frames");
}
```

**Step 2: Run integration test**

Run: `cargo test avmdec_decodes_multi -- --ignored --nocapture`

If it fails, debug by examining avmdec stderr output. Common issues:
- Sequence header field ordering wrong
- Missing or extra bits in frame header
- order_hint_bits mismatch

**Step 3: Fix any issues found by avmdec**

Iterate on sequence.rs and frame.rs until avmdec accepts the bitstream.

**Step 4: Commit**

```
git add tests/avmdec_integration.rs
git commit -m "test: add avmdec integration test for multi-frame bitstream"
```

---

### Task 6: DC Coefficient Coding

**Files:**
- Modify: `src/tile.rs`
- Modify: `src/entropy.rs`

This task adds the ability to encode a non-zero DC coefficient (required for correct solid-color reconstruction).

**Step 1: Write the failing test**

In `src/tile.rs`:

```rust
#[test]
fn tile_data_with_dc_coefficient() {
    // Y=149 (green in BT.601), prediction=128, residual=21
    let data = encode_tile_data_with_dc(21);
    assert!(!data.is_empty());
    // Should be different from zero-coefficient tile
    let zero_data = encode_tile_data_with_dc(0);
    assert_ne!(data, zero_data);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test tile_data_with_dc -- --nocapture`
Expected: FAIL — function doesn't exist

**Step 3: Implement DC coefficient coding**

The encoding sequence for a single non-zero DC coefficient (EOB=1):

1. **txb_skip**: symbol 0 (not skipped) using `txb_skip_cdf`
2. **eob_pt**: symbol 0 (eob_pt=1, meaning 1 coefficient) using `eob_flag_cdf`
3. **coeff_base_eob**: symbol encoding the coefficient level using `coeff_base_eob_cdf`
4. **coeff_br (base range)**: if level > NUM_BASE_LEVELS, encode remainder
5. **dc_sign**: sign bit using `dc_sign_cdf`

Extract default CDF values from `av2/common/entropy_inits_coeffs.h` for:
- `av2_default_txb_skip_cdfs`
- `av2_default_eob_multi64_cdfs` (for 64x64 blocks)
- `av2_default_coeff_base_eob_multi_cdfs`
- `av2_default_dc_sign_cdfs`
- `av2_default_coeff_br_cdfs` (for base range coding)

Create `encode_tile_data_with_dc(y_dc_residual: i32)` that:
- If residual == 0: use current all-skip behavior
- If residual != 0: encode DC coefficient with proper CDF tables

For U/V planes of solid-color frames: compute chroma residual similarly.

The quantization formula for DC (8-bit, from research):
```
ac_qlookup_QTX = [64, 40, 41, 43, 44, 45, 47, 48, 49, 51, 52,
                  54, 55, 57, 59, 60, 62, 64, 66, 68, 70, 72, 74, 76, 78];
// For qindex q: Q = ac_qlookup_QTX[((q-1)%24)+1] << ((q-1)/24) if q >= 25
// For q < 25: Q = ac_qlookup_QTX[q]
```

**Step 4: Run tests to verify they pass**

Run: `cargo test tile -- --nocapture`
Expected: PASS

**Step 5: Commit**

```
git add src/tile.rs src/entropy.rs
git commit -m "feat(tile): add DC coefficient coding for solid-color frames"
```

---

### Task 7: Wire DC Coding into Encoder

**Files:**
- Modify: `src/encode.rs`
- Modify: `src/tile.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn encode_frame_with_pixel_data() {
    let y_plane = vec![149u8; 64 * 64]; // Solid green Y
    let u_plane = vec![43u8; 32 * 32];  // Solid green U
    let v_plane = vec![21u8; 32 * 32];  // Solid green V
    let bs = encode_video_yuv(64, 64, 50, &[(y_plane, u_plane, v_plane)]);
    assert!(!bs.is_empty());
}
```

**Step 2: Implement `encode_video_yuv`**

Update the encoder to:
1. Accept full YUV frames (Y, U, V planes)
2. Compute DC prediction value (128 for luma, 128 for chroma — no neighbors)
3. Compute residual: pixel_value - 128
4. Pass residual to tile encoder

**Step 3: Run tests**

Run: `cargo test encode_frame_with_pixel -- --nocapture`
Expected: PASS

**Step 4: Commit**

```
git add src/encode.rs src/tile.rs
git commit -m "feat(encode): wire DC coefficient coding with pixel input"
```

---

### Task 8: CLI Binary

**Files:**
- Create: `src/bin/wav2c.rs`

**Step 1: Create binary**

```rust
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 || args[2] != "-o" {
        eprintln!("Usage: wav2c input.y4m -o output.obu [--qp N]");
        std::process::exit(1);
    }

    let input = &args[1];
    let output = &args[3];
    let qp: u8 = args.iter()
        .position(|a| a == "--qp")
        .map(|i| args[i + 1].parse().unwrap())
        .unwrap_or(50);

    let mut reader = wav2c::y4m::Y4mReader::open(input).unwrap();
    let header = reader.header();
    let (w, h) = (header.width, header.height);

    let mut frames = Vec::new();
    while let Some(frame) = reader.next_frame() {
        frames.push((frame.y, frame.u, frame.v));
    }

    eprintln!("Encoding {} frames {}x{} qp={}", frames.len(), w, h, qp);
    let bitstream = wav2c::encode::encode_video_yuv(w, h, qp, &frames);
    fs::write(output, &bitstream).unwrap();
    eprintln!("Wrote {} bytes to {}", bitstream.len(), output);
}
```

**Step 2: Build and test manually**

```bash
cargo build
./target/debug/wav2c tests/fixtures/solid_green_64x64.y4m -o /tmp/test.obu
../avm_build/avmdec --i420 -o /tmp/test_dec.yuv /tmp/test.obu
```

**Step 3: Commit**

```
git add src/bin/wav2c.rs
git commit -m "feat(cli): add wav2c command-line encoder"
```

---

### Task 9: Generate Multi-Frame Test Fixture

**Files:**
- Create: `tests/fixtures/solid_green_50frames_64x64.y4m` (generated by ffmpeg)

**Step 1: Generate 50-frame Y4M**

```bash
ffmpeg -f lavfi -i color=c=0x00FF00:s=64x64:r=25:d=2 \
  -pix_fmt yuv420p tests/fixtures/solid_green_50frames_64x64.y4m
```

**Step 2: Verify with avmdec roundtrip**

```bash
./target/debug/wav2c tests/fixtures/solid_green_50frames_64x64.y4m -o /tmp/green50.obu
../avm_build/avmdec --i420 -o /tmp/green50_dec.yuv /tmp/green50.obu
echo "Exit: $?"
```

**Step 3: Commit**

```
git add tests/fixtures/solid_green_50frames_64x64.y4m
git commit -m "test: add 50-frame solid green test fixture"
```

---

### Task 10: Full Integration Tests

**Files:**
- Modify: `tests/avmdec_integration.rs`

**Step 1: Add pixel-verification tests**

```rust
#[test]
#[ignore]
fn avmdec_50_frame_solid_green_correct_pixels() {
    // Encode
    let mut reader = wav2c::y4m::Y4mReader::open(
        "tests/fixtures/solid_green_50frames_64x64.y4m").unwrap();
    let header = reader.header();
    let (w, h) = (header.width, header.height);
    let mut frames = Vec::new();
    let mut input_y_values = Vec::new();
    while let Some(frame) = reader.next_frame() {
        input_y_values.push(frame.y[0]);
        frames.push((frame.y, frame.u, frame.v));
    }
    assert_eq!(frames.len(), 50);

    let bs = wav2c::encode::encode_video_yuv(w, h, 50, &frames);
    std::fs::write("/tmp/wav2c_green50.obu", &bs).unwrap();

    // Decode
    let output = std::process::Command::new("../avm_build/avmdec")
        .args(["--i420", "-o", "/tmp/wav2c_green50_dec.yuv", "/tmp/wav2c_green50.obu"])
        .output().expect("avmdec not found");
    assert!(output.status.success());

    // Verify pixels
    let decoded = std::fs::read("/tmp/wav2c_green50_dec.yuv").unwrap();
    let frame_size = (w * h + w * h / 2) as usize;
    assert_eq!(decoded.len(), frame_size * 50);

    // Check first Y pixel of each frame matches expected reconstruction
    for i in 0..50 {
        let y_pixel = decoded[i * frame_size];
        // DC prediction with quantization may not be exact, but should be close
        let diff = (y_pixel as i32 - input_y_values[i] as i32).abs();
        assert!(diff < 10, "Frame {} Y pixel {}, expected ~{}, diff={}",
            i, y_pixel, input_y_values[i], diff);
    }
}
```

**Step 2: Run full integration test**

Run: `cargo test avmdec_50_frame -- --ignored --nocapture`
Expected: PASS — all 50 frames decode with correct pixel values

**Step 3: Commit**

```
git add tests/avmdec_integration.rs
git commit -m "test: add 50-frame pixel-verification integration test"
```

---

### Task 11: Update README and Phase Roadmap

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md` (update phase status)

Update README to reflect Phase 1 capabilities: multi-frame video encoding, Y4M input, CLI tool, solid-color DC prediction.

Update CLAUDE.md: `Current phases: 0 [DONE], 1 [DONE]`

**Commit:**
```
git commit -m "docs: update README and roadmap for Phase 1 completion"
```

---

## Task Dependencies

```
Task 1 (Y4M parser)          → independent
Task 2 (Seq header video)    → independent
Task 3 (Frame header video)  → depends on Task 2 (order_hint_bits)
Task 4 (Multi-frame encoder) → depends on Tasks 2, 3
Task 5 (avmdec validation)   → depends on Task 4
Task 6 (DC coeff coding)     → independent (can parallelize with Tasks 2-5)
Task 7 (Wire DC into encoder)→ depends on Tasks 4, 6
Task 8 (CLI binary)          → depends on Tasks 1, 7
Task 9 (Test fixture)        → depends on Task 8
Task 10 (Integration tests)  → depends on Tasks 8, 9
Task 11 (Docs)               → depends on Task 10
```

## Parallelization Opportunities

- Tasks 1, 2, 6 can all be developed in parallel (independent modules)
- After Tasks 2+3 merge, Task 4+5 validate the bitstream structure
- After Task 6 merges, Task 7 connects coefficient coding to encoder
- Tasks 8-11 are sequential
