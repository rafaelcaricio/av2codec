# Non-Directional Intra Prediction Modes — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add 5 non-directional intra prediction modes (DC, SMOOTH, SMOOTH_V, SMOOTH_H, PAETH) with brute-force SAD mode selection, dramatically improving visual quality on real images.

**Architecture:** Each block tries all 5 prediction modes, picks the one with lowest SAD, then encodes the residual. Mode index (0-4) is signaled in the tile bitstream. All modes use positions 0-4 in the decoder's fixed mode list, so no dynamic mode list construction is needed and CDF context stays at 0.

**Tech Stack:** Rust, AV2 bitstream format, reference decoder avmdec at `../avm_build/avmdec`

---

## Task Dependencies

```
Task 1 (enum + weights) ─┐
Task 2 (ref pixels) ─────┼─→ Task 3 (prediction fns) ─→ Task 4 (mode selection) ─┐
                          │                                                        │
                          └─→ Task 5 (refactor residual/recon + SbCoeffs) ─────────┼─→ Task 6 (wire pipeline) ─→ Task 7 (integration)
```

Tasks 1 and 2 are independent. Task 5 is independent of 3-4.

---

### Task 1: Add IntraMode Enum and Smooth Weight Tables

**Files:**
- Modify: `src/encode.rs`

Add the `IntraMode` enum and smooth weight const tables. These are foundational types used by all subsequent tasks.

**Code to add** (after the `FrameEncodeParams` struct, around line 217):

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum IntraMode {
    DcPred = 0,
    SmoothPred = 1,
    SmoothVPred = 2,
    SmoothHPred = 3,
    PaethPred = 4,
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
```

**Tests to add:**

```rust
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
```

**Verify:** `cargo test --lib` — all existing 153 tests + 3 new pass.

---

### Task 2: Add Reference Pixel Extraction

**Files:**
- Modify: `src/encode.rs`

Add `get_ref_pixels()` which extracts above row, left column, and top-left pixel from the reconstruction buffer, handling boundary cases exactly as the decoder does (reconintra.c:1110-1260).

**Code to add:**

```rust
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
```

**Tests to add:**

```rust
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
```

**Verify:** `cargo test --lib` — all tests pass.

---

### Task 3: Add Prediction Functions

**Files:**
- Modify: `src/encode.rs`

Add 5 prediction functions, each returning a `Vec<u8>` of `bsize * bsize` predicted pixels.

**Code to add:**

```rust
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
```

**Tests to add:**

```rust
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
    assert_eq!(pred[0], 255);
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
    assert_eq!(pred[0], 255);
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
    // base = 200 + 100 - 200 = 100
    // p_left = |100-100| = 0, p_top = |100-200| = 100, p_tl = |100-200| = 100
    // p_left <= p_top && p_left <= p_tl → left[r] = 100
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
    // base = 100 + 200 - 200 = 100
    // p_left = |100-200| = 100, p_top = |100-100| = 0, p_tl = |100-200| = 100
    // p_top <= p_tl → above[c] = 100
    assert!(pred.iter().all(|&v| v == 100));
}
```

**Verify:** `cargo test --lib` — all tests pass.

---

### Task 4: Add Mode Selection

**Files:**
- Modify: `src/encode.rs`

Add SAD computation and brute-force mode selection.

**Code to add:**

```rust
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
```

**Tests to add:**

```rust
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
        left: {
            let mut l = vec![0u8; 32];
            l[31] = 0;
            l
        },
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
```

**Verify:** `cargo test --lib` — all tests pass.

---

### Task 5: Refactor Residual/Reconstruct + Update SbCoeffs + Tile Encoding

**Files:**
- Modify: `src/encode.rs` (refactor compute_residual_block, reconstruct_block)
- Modify: `src/tile.rs` (add y_mode/uv_mode to SbCoeffs, encode mode from SbCoeffs)

This task changes existing interfaces. All existing tests must still pass.

**Changes to `src/encode.rs`:**

Change `compute_residual_block` signature:
```rust
// OLD: pub fn compute_residual_block(pixels: &[u8], width: usize, height: usize, prediction: u8) -> Vec<i32>
// NEW:
pub fn compute_residual_block(pixels: &[u8], pred_block: &[u8]) -> Vec<i32> {
    pixels
        .iter()
        .zip(pred_block.iter())
        .map(|(&p, &q)| p as i32 - q as i32)
        .collect()
}
```

Change `reconstruct_block` signature:
```rust
// OLD: pub fn reconstruct_block(recon: &mut [u8], stride: usize, x: usize, y: usize, bsize: usize, pred: u8, inv_residual: &[i32])
// NEW:
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
```

**Update all callers in `encode_plane_block_64` and `encode_plane_block_32`** — temporarily pass `vec![pred; bsize*bsize]` to preserve current behavior until Task 6 wires in mode selection.

**Update existing tests:**
- `compute_residual_block_midgray`: `compute_residual_block(&pixels, &vec![128u8; 64])`
- `compute_residual_block_values`: `compute_residual_block(&pixels, &vec![128u8; 4])`
- `compute_residual_block_custom_prediction`: `compute_residual_block(&pixels, &vec![150u8; 4])`
- `reconstruct_block_basic`: pass `&[128u8; 4]` instead of `128u8`
- `reconstruct_block_clamps`: pass `&[128u8; 4]` instead of `128u8`

**Changes to `src/tile.rs`:**

Add y_mode and uv_mode to SbCoeffs:
```rust
pub struct SbCoeffs {
    pub y_levels: [i32; 1024],
    pub y_eob: u16,
    pub u_levels: [i32; 1024],
    pub u_eob: u16,
    pub v_levels: [i32; 1024],
    pub v_eob: u16,
    pub y_mode: u8,
    pub uv_mode: u8,
}
```

Update `encode_tile_data_multi_sb` to use `sb.y_mode` and `sb.uv_mode`:
```rust
// OLD:
ec.encode_symbol(0, &Y_MODE_IDX_ICDF);
ec.encode_symbol(0, &UV_MODE_ICDF);
// NEW:
ec.encode_symbol(sb.y_mode as usize, &Y_MODE_IDX_ICDF);
ec.encode_symbol(sb.uv_mode as usize, &UV_MODE_ICDF);
```

Update all SbCoeffs constructors in tests to include `y_mode: 0, uv_mode: 0`.

Update `encode_tile_data_with_coeffs` similarly — add `y_mode: u8, uv_mode: u8` parameters (default callers pass 0).

**Verify:** `cargo test` — all 153+ existing tests pass unchanged (all modes are still 0 = DC).

---

### Task 6: Wire Mode Selection into Encoding Pipeline

**Files:**
- Modify: `src/encode.rs`

Replace hardcoded DC prediction in `encode_plane_block_64` and `encode_plane_block_32` with mode selection. This is where quality improvement happens.

**Updated `encode_plane_block_64`:**
```rust
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
```

**Updated `encode_plane_block_32`** — same pattern, returns `([i32; 1024], u16, IntraMode)`.

**Updated `encode_frame_yuv`** — captures modes and passes to SbCoeffs:
```rust
let (y_levels, y_eob, y_mode) = encode_plane_block_64(...);
let (u_levels, u_eob, u_mode) = encode_plane_block_32(...);
let (v_levels, v_eob, _v_mode) = encode_plane_block_32(...);

sb_grid.push(SbCoeffs {
    y_levels, y_eob,
    u_levels, u_eob,
    v_levels, v_eob,
    y_mode: y_mode as u8,
    uv_mode: u_mode as u8,
});
```

**Remove the old `predict_dc` function** (replaced by `get_ref_pixels` + `predict_dc_block`).

**Verify:** `cargo test` — all tests pass. For solid-color blocks, DC is still selected (SAD=0 for DC, same for SMOOTH/PAETH when refs=pixel). For non-trivial content, better modes may be chosen.

**Verify:** `cargo clippy --tests -- -D warnings` — clean.

---

### Task 7: Integration Test and Verification

**Files:**
- Modify: `tests/avmdec_integration.rs`

**Step 1:** Run existing integration tests to confirm nothing broke:
```bash
cargo test --test avmdec_integration
```

**Step 2:** Manually test with a real photo to verify PSNR improvement:
```bash
ffmpeg -i /tmp/eu_original.png -vf "scale=768:768,format=yuv420p" -f yuv4mpegpipe /tmp/test_photo.y4m
cargo run -- /tmp/test_photo.y4m /tmp/test_photo.obu
../avm_build/avmdec -i /tmp/test_photo.obu -o /tmp/test_photo_out.y4m --rawvideo
```

Compare PSNR — expect significant improvement over DC-only (previous: Y=26.6dB for the 768x768 photo).

**Step 3:** Add integration test for mode diversity — encode a gradient image and verify avmdec decodes it:

```rust
#[test]
fn avmdec_decodes_gradient_with_smooth_modes() {
    // Create 128x128 with vertical gradient
    let w = 128u32;
    let h = 128u32;
    let mut y_plane = vec![0u8; (w * h) as usize];
    for row in 0..h {
        let val = ((row as f32 / h as f32) * 255.0) as u8;
        for col in 0..w {
            y_plane[(row * w + col) as usize] = val;
        }
    }
    let u_plane = vec![128u8; (w / 2 * h / 2) as usize];
    let v_plane = vec![128u8; (w / 2 * h / 2) as usize];

    let bs = wav2c::encode::encode_video_yuv(w, h, 50, &[(y_plane, u_plane, v_plane)]);
    // Write to temp file and decode with avmdec
    // ... (same pattern as existing avmdec tests)
    // Verify successful decode
}
```

**Verify:** All integration tests pass with avmdec.

---

## Verification Checklist

1. `cargo test` — all tests pass (existing + new)
2. `cargo clippy --tests -- -D warnings` — clean
3. Existing integration tests still pass with avmdec
4. Real photo encode → decode shows improved PSNR
5. Gradient/edge content benefits from SMOOTH/PAETH modes
