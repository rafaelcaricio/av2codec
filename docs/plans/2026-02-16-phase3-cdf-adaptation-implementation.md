# Phase 3: CDF Adaptation Engine — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the AV2 CDF adaptation mechanism so the entropy decoder can correctly decode bitstreams with `disable_cdf_update=false` (like avmenc output).

**Architecture:** Add a standalone `update_cdf()` function matching the AV2 reference, create a `FrameCdfContext` struct holding mutable CDF copies, and wire it into the decoder's tile parsing. The encoder keeps using static CDFs with `disable_cdf_update=true` for now — encoder-side CDF adaptation is a future enhancement.

**Tech Stack:** Rust, no new dependencies

---

### Task 1: Implement `update_cdf` standalone function

**Files:**
- Modify: `src/entropy.rs`

**Context:** The AV2 CDF update algorithm adapts probability tables after each symbol decode/encode. Each CDF array has `nsymbs + 2` elements: the ICDF values, an adaptation counter, and a PARA index. The algorithm shifts ICDF values toward 0 for the decoded symbol and toward 32768 for others, at a rate determined by the counter and PARA table.

**Step 1: Add constants and the PARA table to `src/entropy.rs`**

Add after the existing `PROB_INC_TBL`:

```rust
pub const NUM_PARA_COMBINATIONS: usize = 125;
pub const NUM_PARA_INTERVALS: usize = 3;

#[rustfmt::skip]
pub static NSYMBS2SPEED: [i32; 17] = [
    0, 0, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
];

#[rustfmt::skip]
pub static PARA_ADJUSTMENT_LIST: [[i32; NUM_PARA_INTERVALS]; NUM_PARA_COMBINATIONS] = [
    [ 0,  0,  0], [ 0,  0, -1], [ 0,  0, -2], [ 0,  0,  1], [ 0,  0,  1],
    [ 0, -1,  0], [ 0, -1, -1], [ 0, -1, -2], [ 0, -1,  1], [ 0, -1,  1],
    [ 0, -2,  0], [ 0, -2, -1], [ 0, -2, -2], [ 0, -2,  1], [ 0, -2,  1],
    [ 0,  1,  0], [ 0,  1, -1], [ 0,  1, -2], [ 0,  1,  1], [ 0,  1,  1],
    [ 0,  1,  0], [ 0,  1, -1], [ 0,  1, -2], [ 0,  1,  1], [ 0,  1,  1],
    [-1,  0,  0], [-1,  0, -1], [-1,  0, -2], [-1,  0,  1], [-1,  0,  1],
    [-1, -1,  0], [-1, -1, -1], [-1, -1, -2], [-1, -1,  1], [-1, -1,  1],
    [-1, -2,  0], [-1, -2, -1], [-1, -2, -2], [-1, -2,  1], [-1, -2,  1],
    [-1,  1,  0], [-1,  1, -1], [-1,  1, -2], [-1,  1,  1], [-1,  1,  1],
    [-1,  1,  0], [-1,  1, -1], [-1,  1, -2], [-1,  1,  1], [-1,  1,  1],
    [-2,  0,  0], [-2,  0, -1], [-2,  0, -2], [-2,  0,  1], [-2,  0,  1],
    [-2, -1,  0], [-2, -1, -1], [-2, -1, -2], [-2, -1,  1], [-2, -1,  1],
    [-2, -2,  0], [-2, -2, -1], [-2, -2, -2], [-2, -2,  1], [-2, -2,  1],
    [-2,  1,  0], [-2,  1, -1], [-2,  1, -2], [-2,  1,  1], [-2,  1,  1],
    [-2,  1,  0], [-2,  1, -1], [-2,  1, -2], [-2,  1,  1], [-2,  1,  1],
    [ 1,  0,  0], [ 1,  0, -1], [ 1,  0, -2], [ 1,  0,  1], [ 1,  0,  1],
    [ 1, -1,  0], [ 1, -1, -1], [ 1, -1, -2], [ 1, -1,  1], [ 1, -1,  1],
    [ 1, -2,  0], [ 1, -2, -1], [ 1, -2, -2], [ 1, -2,  1], [ 1, -2,  1],
    [ 1,  1,  0], [ 1,  1, -1], [ 1,  1, -2], [ 1,  1,  1], [ 1,  1,  1],
    [ 1,  1,  0], [ 1,  1, -1], [ 1,  1, -2], [ 1,  1,  1], [ 1,  1,  1],
    [ 1,  0,  0], [ 1,  0, -1], [ 1,  0, -2], [ 1,  0,  1], [ 1,  0,  1],
    [ 1, -1,  0], [ 1, -1, -1], [ 1, -1, -2], [ 1, -1,  1], [ 1, -1,  1],
    [ 1, -2,  0], [ 1, -2, -1], [ 1, -2, -2], [ 1, -2,  1], [ 1, -2,  1],
    [ 1,  1,  0], [ 1,  1, -1], [ 1,  1, -2], [ 1,  1,  1], [ 1,  1,  1],
    [ 1,  1,  0], [ 1,  1, -1], [ 1,  1, -2], [ 1,  1,  1], [ 1,  1,  1],
];
```

**Step 2: Implement `update_cdf`**

Add the function to `src/entropy.rs`:

```rust
/// Update a mutable CDF table after decoding/encoding a symbol.
///
/// CDF layout: [icdf_0, icdf_1, ..., icdf_{nsymbs-1}, counter, para_idx]
/// - icdf values are inverse CDF (descending, last is 0)
/// - counter: adaptation speed tracker (0..32, saturates)
/// - para_idx: indexes into PARA_ADJUSTMENT_LIST for rate tuning
///
/// The `cdf` slice must have exactly `nsymbs + 2` elements.
pub fn update_cdf(cdf: &mut [u16], val: usize, nsymbs: usize) {
    debug_assert!(nsymbs < 17);
    debug_assert!(val < nsymbs);
    debug_assert_eq!(cdf.len(), nsymbs + 2);

    let counter = cdf[nsymbs] as i32;
    let para_idx = cdf[nsymbs + 1] as usize;

    let time_interval = if counter > 31 {
        2
    } else if counter > 15 {
        1
    } else {
        0
    };

    let rate = 3 + time_interval + NSYMBS2SPEED[nsymbs]
        + PARA_ADJUSTMENT_LIST[para_idx][time_interval as usize];

    let mut tmp: i32 = CDF_PROB_TOP as i32; // AVM_ICDF(0) = 32768

    for i in 0..(nsymbs - 1) {
        if i == val {
            tmp = 0;
        }
        let cur = cdf[i] as i32;
        if tmp < cur {
            cdf[i] = (cur - ((cur - tmp) >> rate)) as u16;
        } else {
            cdf[i] = (cur + ((tmp - cur) >> rate)) as u16;
        }
    }

    if cdf[nsymbs] < 32 {
        cdf[nsymbs] += 1;
    }
}
```

**Step 3: Write unit tests for `update_cdf`**

Add to the `#[cfg(test)] mod tests` block in `src/entropy.rs`:

```rust
#[test]
fn update_cdf_two_symbols_adapts_toward_symbol_0() {
    // 2-symbol CDF: [16384, 0] + counter=0 + para=0
    let mut cdf = vec![16384u16, 0, 0, 0];
    update_cdf(&mut cdf, 0, 2);
    // After seeing symbol 0, icdf[0] should decrease (more probability mass on symbol 0)
    assert!(cdf[0] < 16384, "cdf[0]={} should decrease", cdf[0]);
    assert_eq!(cdf[1], 0); // last icdf always 0
    assert_eq!(cdf[2], 1); // counter incremented
}

#[test]
fn update_cdf_two_symbols_adapts_toward_symbol_1() {
    let mut cdf = vec![16384u16, 0, 0, 0];
    update_cdf(&mut cdf, 1, 2);
    // After seeing symbol 1, icdf[0] should increase (less probability on symbol 0)
    assert!(cdf[0] > 16384, "cdf[0]={} should increase", cdf[0]);
    assert_eq!(cdf[2], 1);
}

#[test]
fn update_cdf_counter_saturates_at_32() {
    let mut cdf = vec![16384u16, 0, 31, 0];
    update_cdf(&mut cdf, 0, 2);
    assert_eq!(cdf[2], 32);
    update_cdf(&mut cdf, 0, 2);
    assert_eq!(cdf[2], 32); // stays at 32
}

#[test]
fn update_cdf_four_symbols() {
    // 4-symbol CDF: uniform [24576, 16384, 8192, 0] + counter=0 + para=0
    let mut cdf = vec![24576u16, 16384, 8192, 0, 0, 0];
    let original = cdf.clone();
    update_cdf(&mut cdf, 1, 4);
    // Symbol 0 prob should decrease (icdf[0] increases)
    assert!(cdf[0] >= original[0]);
    // Symbol 1 was decoded, icdf[1] should decrease after val crossing
    assert!(cdf[1] < original[1]);
    assert_eq!(cdf[3], 0); // last icdf stays 0
    assert_eq!(cdf[4], 1); // counter incremented
}

#[test]
fn update_cdf_repeated_converges() {
    // Keep decoding symbol 0 — CDF should converge toward all probability on symbol 0
    let mut cdf = vec![16384u16, 0, 0, 0];
    for _ in 0..100 {
        update_cdf(&mut cdf, 0, 2);
    }
    // icdf[0] should be very small (almost all probability on symbol 0)
    assert!(cdf[0] < 1000, "cdf[0]={} should converge near 0", cdf[0]);
}

#[test]
fn update_cdf_matches_reference_two_symbols() {
    // Verify exact values against reference C implementation
    // Start: [16384, 0, 0, 0] (uniform 2-symbol, counter=0, para=0)
    // nsymbs=2, nsymbs2speed[2]=1, counter=0 → time_interval=0
    // rate = 3 + 0 + 1 + para[0][0] = 3 + 0 + 1 + 0 = 4
    // val=0: tmp starts at 32768, at i=0: tmp becomes 0, cdf[0] -= (16384-0)>>4 = 1024
    // cdf[0] = 16384 - 1024 = 15360
    let mut cdf = vec![16384u16, 0, 0, 0];
    update_cdf(&mut cdf, 0, 2);
    assert_eq!(cdf[0], 15360);
    assert_eq!(cdf[2], 1);
}
```

**Step 4: Run tests**

Run: `cargo test --lib entropy::tests -- --nocapture`
Expected: All tests pass

**Step 5: Commit**

```bash
git add src/entropy.rs
git commit -m "feat(entropy): add update_cdf function with PARA adaptation support"
```

---

### Task 2: Add `decode_symbol_mut` to DaalaDecoder

**Files:**
- Modify: `src/decode/entropy.rs`

**Context:** The decoder needs a method that takes a mutable CDF slice, decodes a symbol, then calls `update_cdf` on the same slice. The existing `decode_symbol(&[u16])` stays for backward compatibility (static CDFs).

**Step 1: Add `decode_symbol_mut` method**

Add to the `impl<'a> DaalaDecoder<'a>` block in `src/decode/entropy.rs`:

```rust
pub fn decode_symbol_mut(&mut self, cdf: &mut [u16]) -> usize {
    let nsyms = cdf.len() - 2; // CDF has nsymbs + 2 elements
    let dif = self.dif;
    let r = self.rng;

    debug_assert!(dif >> (OD_EC_WINDOW_SIZE as u64 - 16) < r as u64);
    debug_assert!(r >= 0x8000);

    let c = (dif >> (OD_EC_WINDOW_SIZE as u64 - 16)) as u32;
    let mut v = r;
    let mut ret: usize = 0;
    let mut u;
    loop {
        u = v;
        v = prob_scale(cdf[ret], r, ret, nsyms);
        if c >= v {
            break;
        }
        ret += 1;
    }
    debug_assert!(v < u);
    debug_assert!(u <= r);

    let new_rng = u - v;
    let new_dif = dif - ((v as u64) << (OD_EC_WINDOW_SIZE as u64 - 16));
    self.normalize(new_dif, new_rng);

    crate::entropy::update_cdf(cdf, ret, nsyms);

    ret
}
```

**Step 2: Add `encode_symbol_mut` to DaalaCoder**

Add to the `impl DaalaCoder` block in `src/entropy.rs`:

```rust
pub fn encode_symbol_mut(&mut self, s: usize, cdf: &mut [u16]) {
    let nsyms = cdf.len() - 2;
    debug_assert!(s < nsyms);
    debug_assert_eq!(cdf[nsyms - 1], 0);

    let fl = if s > 0 {
        u32::from(cdf[s - 1])
    } else {
        CDF_PROB_TOP
    };
    let fh = u32::from(cdf[s]);
    self.encode_q15(fl, fh, s, nsyms);

    update_cdf(cdf, s, nsyms);
}
```

**Step 3: Write roundtrip test with mutable CDFs**

Add to tests in `src/decode/entropy.rs`:

```rust
#[test]
fn roundtrip_symbol_mut_adapts_cdf() {
    // Encode 3 symbols using mutable CDFs, decode with same adaptation
    let init_cdf = vec![16384u16, 0, 0, 0]; // 2-symbol + counter + para

    let mut enc_cdf = init_cdf.clone();
    let mut enc = DaalaCoder::new();
    enc.encode_symbol_mut(0, &mut enc_cdf);
    enc.encode_symbol_mut(0, &mut enc_cdf);
    enc.encode_symbol_mut(1, &mut enc_cdf);
    let buf = enc.done();

    let mut dec_cdf = init_cdf.clone();
    let mut dec = DaalaDecoder::new(&buf);
    assert_eq!(dec.decode_symbol_mut(&mut dec_cdf), 0);
    assert_eq!(dec.decode_symbol_mut(&mut dec_cdf), 0);
    assert_eq!(dec.decode_symbol_mut(&mut dec_cdf), 1);

    // Both CDFs should be identical after processing same symbols
    assert_eq!(enc_cdf, dec_cdf);
}

#[test]
fn roundtrip_symbol_mut_many_symbols() {
    let init_cdf = vec![24576u16, 16384, 8192, 0, 0, 0]; // 4-symbol

    let symbols = [0, 1, 2, 3, 0, 0, 1, 3, 2, 0, 1, 1, 0, 2, 3, 0];
    let mut enc_cdf = init_cdf.clone();
    let mut enc = DaalaCoder::new();
    for &s in &symbols {
        enc.encode_symbol_mut(s, &mut enc_cdf);
    }
    let buf = enc.done();

    let mut dec_cdf = init_cdf.clone();
    let mut dec = DaalaDecoder::new(&buf);
    for &expected in &symbols {
        assert_eq!(dec.decode_symbol_mut(&mut dec_cdf), expected);
    }
    assert_eq!(enc_cdf, dec_cdf);
}
```

**Step 4: Run tests**

Run: `cargo test --lib -- --nocapture`
Expected: All tests pass including new roundtrip tests

**Step 5: Commit**

```bash
git add src/entropy.rs src/decode/entropy.rs
git commit -m "feat(entropy): add decode_symbol_mut and encode_symbol_mut with CDF adaptation"
```

---

### Task 3: Create `FrameCdfContext` struct

**Files:**
- Create: `src/cdf_context.rs`
- Modify: `src/lib.rs` (add `pub mod cdf_context;`)

**Context:** The `FrameCdfContext` holds mutable copies of all CDF tables needed for tile decoding. It's initialized from default values at frame start. Each CDF array is extended by 2 elements (counter=0, para_idx from defaults).

For Phase 3 we only need the CDFs our current decoder uses. Future phases add partition CDFs, extended mode CDFs, etc.

**Step 1: Create `src/cdf_context.rs`**

```rust
use crate::cdf_tables::*;

fn make_cdf(icdf: &[u16], para: u16) -> Vec<u16> {
    let mut cdf = icdf.to_vec();
    cdf.push(0);    // adaptation counter
    cdf.push(para); // PARA index
    cdf
}

pub struct FrameCdfContext {
    pub do_split: Vec<u16>,
    pub y_mode_set: Vec<u16>,
    pub y_mode_idx: Vec<u16>,
    pub uv_mode: Vec<u16>,
    pub txb_skip_y: Vec<u16>,
    pub txb_skip_u: [Vec<u16>; 3],
    pub txb_skip_v: [Vec<u16>; 3],
    pub txb_skip_v_u_nz: [Vec<u16>; 3],
    pub dc_sign_y: [Vec<u16>; 3],
    pub dc_sign_uv: Vec<u16>,
    pub eob_multi_1024: [Vec<u16>; 3],
    pub eob_extra: Vec<u16>,
    pub coeff_base_lf_y: [[Vec<u16>; 33]; 2],
    pub coeff_base_lf_eob_y: [[Vec<u16>; 4]; 2],
    pub coeff_base_y: [[Vec<u16>; 20]; 2],
    pub coeff_base_eob_y: [[Vec<u16>; 4]; 2],
    pub coeff_br_lf_y: [Vec<u16>; 14],
    pub coeff_br_y: [Vec<u16>; 7],
    pub coeff_base_lf_uv: [Vec<u16>; 12],
    pub coeff_base_lf_eob_uv: [Vec<u16>; 4],
    pub coeff_base_uv: [Vec<u16>; 12],
    pub coeff_base_eob_uv: [Vec<u16>; 4],
    pub coeff_br_lf_uv: [Vec<u16>; 4],
    pub coeff_br_uv: [Vec<u16>; 4],
}

const DO_SPLIT_ICDF: [u16; 2] = [12276, 0];
const Y_MODE_SET_ICDF: [u16; 4] = [3905, 1746, 1044, 0];
const Y_MODE_IDX_ICDF: [u16; 8] = [17593, 12693, 11040, 8670, 6363, 5113, 3908, 0];
const UV_MODE_ICDF: [u16; 8] = [23405, 11811, 9903, 8015, 6357, 4785, 2340, 0];
const TXB_SKIP_Y_ICDF: [u16; 2] = [29968, 0];

impl FrameCdfContext {
    pub fn new() -> Self {
        Self {
            do_split: make_cdf(&DO_SPLIT_ICDF, 0),
            y_mode_set: make_cdf(&Y_MODE_SET_ICDF, 0),
            y_mode_idx: make_cdf(&Y_MODE_IDX_ICDF, 0),
            uv_mode: make_cdf(&UV_MODE_ICDF, 0),
            txb_skip_y: make_cdf(&TXB_SKIP_Y_ICDF, 0),
            txb_skip_u: std::array::from_fn(|i| make_cdf(&TXB_SKIP_U_CTX_ICDF[i], 0)),
            txb_skip_v: std::array::from_fn(|i| make_cdf(&TXB_SKIP_V_CTX_ICDF[i], 0)),
            txb_skip_v_u_nz: std::array::from_fn(|i| make_cdf(&TXB_SKIP_V_U_NONZERO_CTX_ICDF[i], 0)),
            dc_sign_y: std::array::from_fn(|i| make_cdf(&DC_SIGN_Y_ICDF[i], 0)),
            dc_sign_uv: make_cdf(&DC_SIGN_UV_ICDF, 0),
            eob_multi_1024: std::array::from_fn(|i| make_cdf(&EOB_MULTI_1024_ICDF[i], 0)),
            eob_extra: make_cdf(&EOB_EXTRA_ICDF, 0),
            coeff_base_lf_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_LF_Y_ICDF[t][c], 0))
            }),
            coeff_base_lf_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_LF_EOB_Y_ICDF[t][c], 0))
            }),
            coeff_base_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_Y_ICDF[t][c], 0))
            }),
            coeff_base_eob_y: std::array::from_fn(|t| {
                std::array::from_fn(|c| make_cdf(&COEFF_BASE_EOB_Y_ICDF[t][c], 0))
            }),
            coeff_br_lf_y: std::array::from_fn(|i| make_cdf(&COEFF_BR_LF_Y_ICDF[i], 0)),
            coeff_br_y: std::array::from_fn(|i| make_cdf(&COEFF_BR_Y_ICDF[i], 0)),
            coeff_base_lf_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_LF_UV_ICDF[i], 0)),
            coeff_base_lf_eob_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_LF_EOB_UV_ICDF[i], 0)),
            coeff_base_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_UV_ICDF[i], 0)),
            coeff_base_eob_uv: std::array::from_fn(|i| make_cdf(&COEFF_BASE_EOB_UV_ICDF[i], 0)),
            coeff_br_lf_uv: std::array::from_fn(|i| make_cdf(&COEFF_BR_LF_UV_ICDF[i], 0)),
            coeff_br_uv: std::array::from_fn(|i| make_cdf(&COEFF_BR_UV_ICDF[i], 0)),
        }
    }
}
```

**Step 2: Register the module**

Add `pub mod cdf_context;` to `src/lib.rs`.

**Step 3: Write a test that `FrameCdfContext` initializes correctly**

Add to `src/cdf_context.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_initializes_with_correct_sizes() {
        let ctx = FrameCdfContext::new();
        // do_split: 2 icdf + counter + para = 4
        assert_eq!(ctx.do_split.len(), 4);
        // y_mode_idx: 8 icdf + counter + para = 10
        assert_eq!(ctx.y_mode_idx.len(), 10);
        // All counters start at 0
        assert_eq!(ctx.do_split[2], 0);
        assert_eq!(ctx.y_mode_idx[8], 0);
        // ICDF values match originals
        assert_eq!(ctx.do_split[0], 12276);
        assert_eq!(ctx.do_split[1], 0);
    }

    #[test]
    fn context_cdfs_are_mutable() {
        let mut ctx = FrameCdfContext::new();
        crate::entropy::update_cdf(&mut ctx.do_split, 0, 2);
        assert_eq!(ctx.do_split[2], 1); // counter incremented
        assert_ne!(ctx.do_split[0], 12276); // value adapted
    }
}
```

**Step 4: Run tests**

Run: `cargo test --lib cdf_context::tests -- --nocapture`
Expected: All pass

**Step 5: Commit**

```bash
git add src/cdf_context.rs src/lib.rs
git commit -m "feat: add FrameCdfContext with mutable CDF tables for adaptation"
```

---

### Task 4: Wire CDF adaptation into the decoder tile parser

**Files:**
- Modify: `src/decode/tile.rs`
- Modify: `src/decode/mod.rs`

**Context:** The decoder's `decode_tile_data_multi_sb` currently uses static `&[u16]` ICDF slices. We need to add a `use_cdf_update: bool` parameter. When true, it creates a `FrameCdfContext` and passes mutable CDFs to `decode_symbol_mut`. When false, it uses the existing static path.

This is the largest task. The tile decoder has ~20 `decode_symbol` calls that need to be routed through mutable CDFs when adaptation is enabled.

**Step 1: Add `use_cdf_update` parameter to decode functions**

Modify `decode_tile_data_multi_sb` signature in `src/decode/tile.rs`:

```rust
pub fn decode_tile_data_multi_sb(
    data: &[u8],
    sb_cols: usize,
    sb_rows: usize,
    allow_update_cdf: bool,
) -> Vec<DecodedSbCoeffs>
```

**Step 2: Create mutable CDF context and use it**

Inside the function, create the context conditionally:

```rust
let mut cdf_ctx = if allow_update_cdf {
    Some(crate::cdf_context::FrameCdfContext::new())
} else {
    None
};
```

Then for each `ec.decode_symbol(...)` call, branch:

```rust
// Before (static):
let _partition = ec.decode_symbol(&DO_SPLIT_ICDF);
// After (adaptive or static):
let _partition = if let Some(ref mut ctx) = cdf_ctx {
    ec.decode_symbol_mut(&mut ctx.do_split)
} else {
    ec.decode_symbol(&DO_SPLIT_ICDF)
};
```

Apply this pattern to every `decode_symbol` call in the tile decoder. The `PlaneDecodeCtx` struct's `txb_skip_icdf` field changes to carry a mutable reference when adaptation is enabled — or we inline the CDF selection.

The simplest approach: create a helper macro or inline function:

```rust
fn decode_sym(ec: &mut DaalaDecoder, static_icdf: &[u16], cdf: Option<&mut Vec<u16>>) -> usize {
    match cdf {
        Some(cdf) => ec.decode_symbol_mut(cdf),
        None => ec.decode_symbol(static_icdf),
    }
}
```

**Step 3: Update the coefficient decoder**

The `decode_plane_coeffs` function uses `PlaneDecodeCtx` which carries `txb_skip_icdf: &[u16]`. For adaptive mode, we need to pass mutable CDF references instead. The cleanest refactor: pass the `FrameCdfContext` as `Option<&mut FrameCdfContext>` through to coefficient decode functions, and select the CDF based on plane/context.

**Step 4: Update call site in `src/decode/mod.rs`**

Pass `false` (our own bitstreams use `disable_cdf_update=true`):

```rust
let decoded_sbs = tile::decode_tile_data_multi_sb(
    fp.tile_data, sb_cols, sb_rows, !fp.disable_cdf_update
);
```

This requires parsing `disable_cdf_update` from the frame header into `FrameParams`. Add the field:

In `src/decode/frame.rs`, add to `FrameParams`:
```rust
pub struct FrameParams<'a> {
    pub base_qindex: u8,
    pub order_hint: u32,
    pub tile_data: &'a [u8],
    pub disable_cdf_update: bool,
}
```

And populate it from the already-parsed `_disable_cdf_update` bit in both `parse_frame_header_sph0` and `parse_frame_header_sph1`.

**Step 5: Run all tests**

Run: `cargo test`
Expected: All 238+ tests pass — existing behavior unchanged (all our bitstreams use `disable_cdf_update=true`, so the static path is taken)

Run: `cargo clippy --tests`
Expected: 0 warnings

**Step 6: Commit**

```bash
git add src/decode/tile.rs src/decode/mod.rs src/decode/frame.rs
git commit -m "feat(decode): wire CDF adaptation into tile decoder"
```

---

### Task 5: Roundtrip test with CDF adaptation enabled

**Files:**
- Modify: `src/tile.rs` (encoder tile)
- Modify: `src/decode/mod.rs` (test)

**Context:** Verify that our encoder can produce bitstreams with `disable_cdf_update=false` that our decoder correctly decodes using CDF adaptation. This proves the full adaptation pipeline works end-to-end.

**Step 1: Add CDF adaptation to the encoder tile writer**

Mirror Task 4's changes on the encoder side in `src/tile.rs`. Add `allow_update_cdf: bool` parameter to `encode_tile_data_multi_sb` (and the single-SB version). When true, use `encode_symbol_mut` with a `FrameCdfContext`.

**Step 2: Add `disable_cdf_update` control to frame header writer**

In `src/bitstream/frame.rs`, add `disable_cdf_update: bool` to `FrameHeaderConfig` and write it as the appropriate bit in the frame header.

**Step 3: Write integration test**

Add to `src/decode/mod.rs` tests:

```rust
#[test]
fn roundtrip_cdf_adaptation_enabled() {
    // Encode with disable_cdf_update=false, decode with adaptation
    let y = vec![128u8; 64 * 64];
    let u = vec![128u8; 32 * 32];
    let v = vec![128u8; 32 * 32];
    // Use a modified encode path that sets disable_cdf_update=false
    // and uses CDF adaptation during encoding
    let bs = encode_video_yuv_with_cdf_update(64, 64, 50, &[(y.clone(), u.clone(), v.clone())]);
    let frames = Decoder::decode_all(&bs).unwrap();
    assert_eq!(frames.len(), 1);
    assert!(frames[0].y.iter().all(|&p| p == 128));
}
```

**Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass, including the new CDF adaptation roundtrip

**Step 5: Commit**

```bash
git add src/tile.rs src/bitstream/frame.rs src/decode/mod.rs
git commit -m "feat: full CDF adaptation roundtrip — encoder and decoder synchronized"
```

---

### Task 6: Validate with avmenc bitstream

**Files:**
- No code changes — validation only

**Step 1: Test decoding an avmenc bitstream**

```bash
cargo build --release
./target/release/wav2c_decode /tmp/eu_avmenc.obu -o /tmp/eu_wav2c_cdf.y4m
```

Expected: Still gray output (tile data parsing will fail on partition symbols we don't support yet), but the decoder should not panic. The CDF adaptation engine is exercised but partition parsing is the bottleneck — that's Phase 4.

**Step 2: Run the comparison script**

```bash
./scripts/compare_decoders.sh -e /tmp/eu_avmenc.obu /tmp/eu_original.png
```

Expected: Same visual result as before (gray wav2c panel), confirming no regression.

**Step 3: Run full test suite**

Run: `cargo test && cargo clippy --tests`
Expected: All tests pass, 0 warnings

**Step 4: Commit any test fixes if needed**

---

## Summary

| Task | Description | Key Deliverable |
|------|-------------|----------------|
| 1 | `update_cdf` function | Core adaptation algorithm with PARA support |
| 2 | `decode_symbol_mut` / `encode_symbol_mut` | Entropy coder integration |
| 3 | `FrameCdfContext` struct | Mutable CDF state management |
| 4 | Wire into decoder tile parser | Decoder uses adaptive CDFs when enabled |
| 5 | Encoder-side CDF adaptation + roundtrip test | Full encode→decode with adaptation |
| 6 | Validate with avmenc bitstream | Confirm no regression, foundation ready for Phase 4 |
