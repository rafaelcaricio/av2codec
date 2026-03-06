# Phase 4: Recursive Partition Tree — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Parse avmenc's recursive partition tree so the decoder reads all symbols in correct order, producing recognizable (blurry) output.

**Architecture:** Add block size/partition infrastructure, scan tables and DCT kernels for all square sizes, partition CDFs, then rewrite the decoder tile parser to recursively descend the partition tree. Each leaf block gets mode+coefficient decoding at its actual size. Reconstruction uses DC prediction for all blocks (directional modes are Phase 6).

**Tech Stack:** Rust, AV2/AVM reference decoder tables

---

## Current State

- Decoder assumes flat 64×64 Y / 32×32 UV blocks per superblock
- Only 32×32 and 64×64 scan tables, DCT kernels, and dequant exist
- Partition parsing reads `do_split` but ignores the result
- `DecodedSbCoeffs` uses fixed `[i32; 1024]` arrays
- Output from avmenc bitstreams is gray (panic caught → fallback)

## Target State

- Recursive partition tree parsed correctly for all 10 partition types
- Variable-size blocks (4×4 through 64×64) decoded with correct scan/DCT/dequant
- DC prediction used for all blocks regardless of signaled mode
- avmenc test photo output: blurry but recognizable (face, shoulders visible)

## Dependency Chain

```
Task 1 (block infra) ─┬─→ Task 2 (scans) ─────────────┐
                       ├─→ Task 3 (DCT kernels) ────────┤
                       └─→ Task 5 (partition CDFs) ──┐  │
                                                      ├──┼─→ Task 6 (partition parser + coeff decoder)
Task 4 (variable dequant) depends on Tasks 2, 3 ─────┘  │
                                                         └─→ Task 7 (reconstruction + integration)
```

---

### Task 1: Block Size, Transform Size, and Partition Type Infrastructure

**Files:**
- Create: `src/block.rs`
- Modify: `src/lib.rs` (add `pub mod block;`)
- Test: inline `#[cfg(test)]` in `src/block.rs`

**What to build:**

A `BlockSize` enum covering all square and rectangular block sizes used by AV2 partitioning. A `TxSize` enum for square transform sizes. A `PartitionType` enum for all 10 partition types. Lookup tables for subsize mapping.

**Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_size_dimensions() {
        assert_eq!(BlockSize::Block4x4.width(), 4);
        assert_eq!(BlockSize::Block4x4.height(), 4);
        assert_eq!(BlockSize::Block64x64.width(), 64);
        assert_eq!(BlockSize::Block64x64.height(), 64);
        assert_eq!(BlockSize::Block32x16.width(), 32);
        assert_eq!(BlockSize::Block32x16.height(), 16);
    }

    #[test]
    fn block_size_bwl() {
        assert_eq!(BlockSize::Block4x4.bwl(), 2);
        assert_eq!(BlockSize::Block8x8.bwl(), 3);
        assert_eq!(BlockSize::Block16x16.bwl(), 4);
        assert_eq!(BlockSize::Block32x32.bwl(), 5);
        assert_eq!(BlockSize::Block64x64.bwl(), 6);
    }

    #[test]
    fn tx_size_from_block() {
        assert_eq!(TxSize::from_block_size(BlockSize::Block4x4), TxSize::Tx4x4);
        assert_eq!(TxSize::from_block_size(BlockSize::Block8x8), TxSize::Tx8x8);
        assert_eq!(TxSize::from_block_size(BlockSize::Block16x16), TxSize::Tx16x16);
        assert_eq!(TxSize::from_block_size(BlockSize::Block32x32), TxSize::Tx32x32);
        assert_eq!(TxSize::from_block_size(BlockSize::Block64x64), TxSize::Tx64x64);
    }

    #[test]
    fn tx_size_params() {
        assert_eq!(TxSize::Tx4x4.width(), 4);
        assert_eq!(TxSize::Tx4x4.log_scale(), 0);
        assert_eq!(TxSize::Tx8x8.log_scale(), 0);
        assert_eq!(TxSize::Tx16x16.log_scale(), 0);
        assert_eq!(TxSize::Tx32x32.log_scale(), 1);
        assert_eq!(TxSize::Tx64x64.log_scale(), 2);
    }

    #[test]
    fn tx_size_num_coeffs() {
        assert_eq!(TxSize::Tx4x4.num_coeffs(), 16);
        assert_eq!(TxSize::Tx8x8.num_coeffs(), 64);
        assert_eq!(TxSize::Tx16x16.num_coeffs(), 256);
        assert_eq!(TxSize::Tx32x32.num_coeffs(), 1024);
        assert_eq!(TxSize::Tx64x64.num_coeffs(), 1024);
    }

    #[test]
    fn subsize_split() {
        assert_eq!(subsize(PartitionType::Split, BlockSize::Block64x64), Some(BlockSize::Block32x32));
        assert_eq!(subsize(PartitionType::Split, BlockSize::Block32x32), Some(BlockSize::Block16x16));
        assert_eq!(subsize(PartitionType::Split, BlockSize::Block16x16), Some(BlockSize::Block8x8));
        assert_eq!(subsize(PartitionType::Split, BlockSize::Block8x8), Some(BlockSize::Block4x4));
        assert_eq!(subsize(PartitionType::Split, BlockSize::Block4x4), None);
    }

    #[test]
    fn subsize_none() {
        assert_eq!(subsize(PartitionType::None, BlockSize::Block64x64), Some(BlockSize::Block64x64));
        assert_eq!(subsize(PartitionType::None, BlockSize::Block4x4), Some(BlockSize::Block4x4));
    }

    #[test]
    fn subsize_horz() {
        assert_eq!(subsize(PartitionType::Horz, BlockSize::Block64x64), Some(BlockSize::Block64x32));
        assert_eq!(subsize(PartitionType::Horz, BlockSize::Block32x32), Some(BlockSize::Block32x16));
        assert_eq!(subsize(PartitionType::Horz, BlockSize::Block16x16), Some(BlockSize::Block16x8));
        assert_eq!(subsize(PartitionType::Horz, BlockSize::Block8x8), Some(BlockSize::Block8x4));
    }

    #[test]
    fn subsize_vert() {
        assert_eq!(subsize(PartitionType::Vert, BlockSize::Block64x64), Some(BlockSize::Block32x64));
        assert_eq!(subsize(PartitionType::Vert, BlockSize::Block32x32), Some(BlockSize::Block16x32));
        assert_eq!(subsize(PartitionType::Vert, BlockSize::Block16x16), Some(BlockSize::Block8x16));
        assert_eq!(subsize(PartitionType::Vert, BlockSize::Block8x8), Some(BlockSize::Block4x8));
    }

    #[test]
    fn block_is_square() {
        assert!(BlockSize::Block4x4.is_square());
        assert!(BlockSize::Block64x64.is_square());
        assert!(!BlockSize::Block32x16.is_square());
        assert!(!BlockSize::Block8x4.is_square());
    }

    #[test]
    fn partition_type_all_variants() {
        assert_eq!(PartitionType::None as u8, 0);
        assert_eq!(PartitionType::Split as u8, 9);
    }
}
```

**Step 2: Run tests, verify they fail**

Run: `cargo test --lib block::tests -- -v 2>&1 | head -20`
Expected: compilation error (module doesn't exist yet)

**Step 3: Implement**

Create `src/block.rs` with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockSize {
    Block4x4,      // 0
    Block4x8,      // 1
    Block8x4,      // 2
    Block8x8,      // 3
    Block8x16,     // 4
    Block16x8,     // 5
    Block16x16,    // 6
    Block16x32,    // 7
    Block32x16,    // 8
    Block32x32,    // 9
    Block32x64,    // 10
    Block64x32,    // 11
    Block64x64,    // 12
}

impl BlockSize {
    pub fn width(self) -> usize { ... }
    pub fn height(self) -> usize { ... }
    pub fn bwl(self) -> u32 { /* log2(width) */ ... }
    pub fn bhl(self) -> u32 { /* log2(height) */ ... }
    pub fn is_square(self) -> bool { self.width() == self.height() }
    pub fn tx_size(self) -> TxSize { TxSize::from_block_size(self) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxSize {
    Tx4x4,
    Tx8x8,
    Tx16x16,
    Tx32x32,
    Tx64x64,
}

impl TxSize {
    pub fn from_block_size(bs: BlockSize) -> TxSize {
        // For square blocks: direct mapping
        // For rectangular blocks: use min(width, height) capped at 32
        // TX_64X64 uses 32x32 coefficient grid (1024 coeffs)
        ...
    }
    pub fn width(self) -> usize { ... }
    pub fn height(self) -> usize { ... }
    pub fn bwl(self) -> u32 { /* log2(width) */ ... }
    pub fn num_coeffs(self) -> usize {
        // TX_64X64 still uses 1024 coeffs (32x32 grid)
        match self {
            Tx4x4 => 16, Tx8x8 => 64, Tx16x16 => 256,
            Tx32x32 => 1024, Tx64x64 => 1024,
        }
    }
    pub fn log_scale(self) -> u32 {
        match self {
            Tx4x4 | Tx8x8 | Tx16x16 => 0,
            Tx32x32 => 1,
            Tx64x64 => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PartitionType {
    None = 0,
    Horz = 1,
    Vert = 2,
    Horz3 = 3,
    Vert3 = 4,
    Horz4A = 5,
    Horz4B = 6,
    Vert4A = 7,
    Vert4B = 8,
    Split = 9,
}

pub fn subsize(partition: PartitionType, bsize: BlockSize) -> Option<BlockSize> {
    // Lookup table matching AV2 subsize_lookup[partition][bsize]
    // Returns None for invalid combinations (e.g., splitting Block4x4)
    ...
}
```

Key constants from AV2 reference:
- `PARTITION_PLOFFSET = 4`
- `PARTITION_BLOCK_GROUPS = 16`
- `PARTITION_CONTEXTS = 64` (16 * 4)
- `SQUARE_SPLIT_CONTEXTS = 8` (2 * 4)

**Step 4: Run tests, verify pass**

Run: `cargo test --lib block::tests`
Expected: all tests pass

**Step 5: Run clippy + commit**

```bash
cargo clippy --tests -- -D warnings
git add src/block.rs src/lib.rs
git commit -m "feat: add block size, transform size, and partition type infrastructure"
```

---

### Task 2: Scan Tables for All Square Sizes

**Files:**
- Modify: `src/scan.rs`
- Test: inline `#[cfg(test)]` in `src/scan.rs`

**What to build:**

Add diagonal scan tables for TX_4X4 (16 entries), TX_8X8 (64 entries), TX_16X16 (256 entries). Add a `get_scan` dispatch function that returns the correct scan for a `TxSize`.

**Step 1: Write failing tests**

Add to `src/scan.rs` tests:

```rust
#[test]
fn scan_4x4_dc_is_first() {
    assert_eq!(DEFAULT_SCAN_4X4[0], 0);
}

#[test]
fn scan_4x4_all_unique() {
    let mut seen = [false; 16];
    for &val in &DEFAULT_SCAN_4X4 {
        assert!(!seen[val as usize], "dup: {}", val);
        seen[val as usize] = true;
    }
}

#[test]
fn scan_4x4_diagonal_order() {
    // Expected: 0, 4, 1, 8, 5, 2, 12, 9, 6, 3, 13, 10, 7, 14, 11, 15
    assert_eq!(DEFAULT_SCAN_4X4[1], 4);  // (1,0)
    assert_eq!(DEFAULT_SCAN_4X4[2], 1);  // (0,1)
}

#[test]
fn scan_8x8_dc_is_first() {
    assert_eq!(DEFAULT_SCAN_8X8[0], 0);
}

#[test]
fn scan_8x8_all_unique() {
    let mut seen = [false; 64];
    for &val in &DEFAULT_SCAN_8X8 {
        assert!(!seen[val as usize], "dup: {}", val);
        seen[val as usize] = true;
    }
}

#[test]
fn scan_16x16_dc_is_first() {
    assert_eq!(DEFAULT_SCAN_16X16[0], 0);
}

#[test]
fn scan_16x16_all_unique() {
    let mut seen = [false; 256];
    for &val in &DEFAULT_SCAN_16X16 {
        assert!(!seen[val as usize], "dup: {}", val);
        seen[val as usize] = true;
    }
}

#[test]
fn get_scan_dispatch() {
    use crate::block::TxSize;
    assert_eq!(get_scan(TxSize::Tx4x4).len(), 16);
    assert_eq!(get_scan(TxSize::Tx8x8).len(), 64);
    assert_eq!(get_scan(TxSize::Tx16x16).len(), 256);
    assert_eq!(get_scan(TxSize::Tx32x32).len(), 1024);
    assert_eq!(get_scan(TxSize::Tx64x64).len(), 1024);
}
```

**Step 2: Run tests, verify they fail**

Run: `cargo test --lib scan::tests -- -v 2>&1 | head -20`

**Step 3: Implement**

Add to `src/scan.rs`:

```rust
use crate::block::TxSize;

// Values from av2/common/scan.c
pub const DEFAULT_SCAN_4X4: [u16; 16] = [
    0, 4, 1, 8, 5, 2, 12, 9, 6, 3, 13, 10, 7, 14, 11, 15
];

pub const DEFAULT_SCAN_8X8: [u16; 64] = [
    0, 8, 1, 16, 9, 2, 24, 17, 10, 3, 32, 25, 18, 11, 4, 40,
    33, 26, 19, 12, 5, 48, 41, 34, 27, 20, 13, 6, 56, 49, 42, 35,
    28, 21, 14, 7, 57, 50, 43, 36, 29, 22, 15, 58, 51, 44, 37, 30,
    23, 59, 52, 45, 38, 31, 60, 53, 46, 39, 61, 54, 47, 62, 55, 63,
];

pub const DEFAULT_SCAN_16X16: [u16; 256] = [
    // Extract from av2/common/scan.c:609-628 (diagonal scan)
    ...
];

pub fn get_scan(tx_size: TxSize) -> &'static [u16] {
    match tx_size {
        TxSize::Tx4x4 => &DEFAULT_SCAN_4X4,
        TxSize::Tx8x8 => &DEFAULT_SCAN_8X8,
        TxSize::Tx16x16 => &DEFAULT_SCAN_16X16,
        TxSize::Tx32x32 | TxSize::Tx64x64 => &DEFAULT_SCAN_32X32,
    }
}
```

Extract the full 16×16 scan table from `../av2/av2/common/scan.c` lines 609-628.

**Step 4: Run tests, verify pass**

Run: `cargo test --lib scan::tests`

**Step 5: Run clippy + commit**

```bash
cargo clippy --tests -- -D warnings
git add src/scan.rs
git commit -m "feat: add scan tables for TX_4X4, TX_8X8, TX_16X16"
```

---

### Task 3: Inverse DCT-2 Kernels for 4×4, 8×8, 16×16

**Files:**
- Modify: `src/dct.rs`
- Test: inline `#[cfg(test)]` in `src/dct.rs`

**What to build:**

Integer inverse DCT-2 kernels for 4×4, 8×8, and 16×16 blocks. The kernels use the same structure as existing 32×32/64×64: 1D column transform → rounding → 1D row transform → final shift.

**Reference:** Kernel tables from `../av2/av2/common/txb_common.c`:
- `tx_kernel_dct2_size4[4][4]`
- `tx_kernel_dct2_size8[8][8]`
- `tx_kernel_dct2_size16[16][16]`

**Shift tables (inv_tx_shift):**
- TX_4X4: `{7, 10}` (shift1=7, shift2=10)
- TX_8X8: `{7, 11}` (shift1=7, shift2=11)
- TX_16X16: `{6, 13}` (shift1=6, shift2=13)

**Step 1: Write failing tests**

```rust
#[test]
fn inverse_dct2_4x4_dc_only() {
    let mut coeffs = [0i32; 16];
    coeffs[0] = 1000;
    let output = inverse_dct2_4x4(&coeffs);
    assert_eq!(output.len(), 16);
    // DC distributes evenly: all pixels should be equal (or near-equal)
    let first = output[0];
    for &v in &output {
        assert!((v - first).abs() <= 1, "DC not uniform: {} vs {}", v, first);
    }
}

#[test]
fn inverse_dct2_8x8_dc_only() {
    let mut coeffs = [0i32; 64];
    coeffs[0] = 1000;
    let output = inverse_dct2_8x8(&coeffs);
    assert_eq!(output.len(), 64);
    let first = output[0];
    for &v in &output {
        assert!((v - first).abs() <= 1, "DC not uniform: {} vs {}", v, first);
    }
}

#[test]
fn inverse_dct2_16x16_dc_only() {
    let mut coeffs = [0i32; 256];
    coeffs[0] = 1000;
    let output = inverse_dct2_16x16(&coeffs);
    assert_eq!(output.len(), 256);
    let first = output[0];
    for &v in &output {
        assert!((v - first).abs() <= 1, "DC not uniform: {} vs {}", v, first);
    }
}

#[test]
fn inverse_dct2_4x4_zero_input() {
    let coeffs = [0i32; 16];
    let output = inverse_dct2_4x4(&coeffs);
    assert!(output.iter().all(|&v| v == 0));
}

#[test]
fn inverse_dct2_8x8_zero_input() {
    let coeffs = [0i32; 64];
    let output = inverse_dct2_8x8(&coeffs);
    assert!(output.iter().all(|&v| v == 0));
}

#[test]
fn inverse_dct2_16x16_zero_input() {
    let coeffs = [0i32; 256];
    let output = inverse_dct2_16x16(&coeffs);
    assert!(output.iter().all(|&v| v == 0));
}
```

**Step 2: Run tests, verify they fail**

Run: `cargo test --lib dct::tests -- -v 2>&1 | head -20`

**Step 3: Implement**

Add to `src/dct.rs`:

Extract the 1D kernel tables from `../av2/av2/common/txb_common.c`:

```rust
const KERNEL_DCT2_4: [[i32; 4]; 4] = [
    // Extract from tx_kernel_dct2_size4
    ...
];

const KERNEL_DCT2_8: [[i32; 8]; 8] = [
    // Extract from tx_kernel_dct2_size8
    ...
];

const KERNEL_DCT2_16: [[i32; 16]; 16] = [
    // Extract from tx_kernel_dct2_size16
    ...
];

pub fn inverse_dct2_4x4(coeffs: &[i32; 16]) -> [i32; 16] {
    // inv_tx_shift = {7, 10}
    // Column transform → round → row transform → final shift
    inverse_dct2_nxn::<4>(coeffs, &KERNEL_DCT2_4, 7, 10)
}

pub fn inverse_dct2_8x8(coeffs: &[i32; 64]) -> [i32; 64] {
    // inv_tx_shift = {7, 11}
    inverse_dct2_nxn::<8>(coeffs, &KERNEL_DCT2_8, 7, 11)
}

pub fn inverse_dct2_16x16(coeffs: &[i32; 256]) -> [i32; 256] {
    // inv_tx_shift = {6, 13}
    inverse_dct2_nxn::<16>(coeffs, &KERNEL_DCT2_16, 6, 13)
}
```

Use a generic helper `inverse_dct2_nxn<const N: usize>` that performs:
1. For each column: multiply by kernel, accumulate, add rounding `(1 << (shift1-1))`, right-shift by `shift1`
2. For each row: multiply by kernel, accumulate, add rounding `(1 << (shift2-1))`, right-shift by `shift2`

This matches the pattern already used by `inverse_dct2_32x32` and `inverse_dct2_64x64`.

**Step 4: Run tests, verify pass**

Run: `cargo test --lib dct::tests`

**Step 5: Run clippy + commit**

```bash
cargo clippy --tests -- -D warnings
git add src/dct.rs
git commit -m "feat: add inverse DCT-2 kernels for 4x4, 8x8, 16x16"
```

---

### Task 4: Variable-Size Dequantization and Inverse Transform

**Files:**
- Modify: `src/decode/transform.rs`
- Test: inline `#[cfg(test)]` in `src/decode/transform.rs`

**What to build:**

Generalize `dequantize_and_inverse` to work with any square `TxSize`. The function should:
1. Select the correct scan table via `get_scan(tx_size)`
2. Use the correct `log_scale` from `TxSize`
3. Call the correct inverse DCT function
4. Return a `Vec<i32>` of the right length

**Step 1: Write failing tests**

```rust
#[test]
fn dequantize_and_inverse_4x4() {
    use crate::block::TxSize;
    let levels = vec![0i32; 16];
    let result = dequantize_and_inverse_var(&levels, 0, 164, TxSize::Tx4x4);
    assert_eq!(result.len(), 16);
    assert!(result.iter().all(|&v| v == 0));
}

#[test]
fn dequantize_and_inverse_8x8() {
    use crate::block::TxSize;
    let levels = vec![0i32; 64];
    let result = dequantize_and_inverse_var(&levels, 0, 164, TxSize::Tx8x8);
    assert_eq!(result.len(), 64);
}

#[test]
fn dequantize_and_inverse_16x16() {
    use crate::block::TxSize;
    let levels = vec![0i32; 256];
    let result = dequantize_and_inverse_var(&levels, 0, 164, TxSize::Tx16x16);
    assert_eq!(result.len(), 256);
}

#[test]
fn dequantize_and_inverse_32x32_matches_legacy() {
    use crate::block::TxSize;
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
fn dequantize_and_inverse_64x64_matches_legacy() {
    use crate::block::TxSize;
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
```

**Step 2: Run tests, verify they fail**

Run: `cargo test --lib decode::transform::tests -- -v 2>&1 | head -20`

**Step 3: Implement**

Add to `src/decode/transform.rs`:

```rust
use crate::block::TxSize;
use crate::scan::get_scan;

pub fn dequantize_and_inverse_var(
    levels: &[i32],
    eob: u16,
    dequant: i32,
    tx_size: TxSize,
) -> Vec<i32> {
    let num_coeffs = tx_size.num_coeffs();
    let log_scale = tx_size.log_scale();
    let scan = get_scan(tx_size);
    let pixel_count = tx_size.width() * tx_size.height();

    if eob == 0 {
        return vec![0i32; pixel_count];
    }

    // Dequantize using scan order
    let mut coeffs = vec![0i32; num_coeffs];
    for scan_idx in 0..(eob as usize) {
        let coeff_idx = scan[scan_idx] as usize;
        coeffs[coeff_idx] = dequantize_coeff(levels[scan_idx], log_scale, dequant);
    }

    // Inverse transform
    match tx_size {
        TxSize::Tx4x4 => {
            let mut arr = [0i32; 16];
            arr.copy_from_slice(&coeffs);
            crate::dct::inverse_dct2_4x4(&arr).to_vec()
        }
        TxSize::Tx8x8 => {
            let mut arr = [0i32; 64];
            arr.copy_from_slice(&coeffs);
            crate::dct::inverse_dct2_8x8(&arr).to_vec()
        }
        TxSize::Tx16x16 => {
            let mut arr = [0i32; 256];
            arr.copy_from_slice(&coeffs);
            crate::dct::inverse_dct2_16x16(&arr).to_vec()
        }
        TxSize::Tx32x32 => {
            let mut arr = [0i32; 1024];
            arr.copy_from_slice(&coeffs);
            crate::dct::inverse_dct2_32x32(&arr).to_vec()
        }
        TxSize::Tx64x64 => {
            let mut arr = [0i32; 1024];
            arr.copy_from_slice(&coeffs);
            crate::dct::inverse_dct2_64x64(&arr).to_vec()
        }
    }
}
```

Keep the existing `dequantize_and_inverse` for backward compatibility with existing tests.

**Step 4: Run tests, verify pass**

Run: `cargo test --lib decode::transform::tests`

**Step 5: Run clippy + commit**

```bash
cargo clippy --tests -- -D warnings
git add src/decode/transform.rs
git commit -m "feat: add variable-size dequantization and inverse transform"
```

---

### Task 5: Partition CDFs and EOB CDFs for All Block Sizes

**Files:**
- Modify: `src/cdf_tables.rs`
- Modify: `src/cdf_context.rs`
- Test: inline tests in both files

**What to build:**

1. Partition decision CDFs (5 binary CDF tables from `av2/common/entropy_inits_partition.h`):
   - `DO_SPLIT_CDF[2][PARTITION_CONTEXTS]` — 2 symbols × 64 contexts × 2 planes
   - `DO_SQUARE_SPLIT_CDF[2][SQUARE_SPLIT_CONTEXTS]` — 2 planes × 8 contexts
   - `RECT_TYPE_CDF[2][PARTITION_CONTEXTS]` — HORZ vs VERT
   - `DO_EXT_PARTITION_CDF[2][1][PARTITION_CONTEXTS]` — 2-way vs extended
   - `DO_UNEVEN_4WAY_CDF[2][1][PARTITION_CONTEXTS]` — 3-way vs 4-way

2. EOB CDFs for smaller transform sizes:
   - `EOB_MULTI_16_ICDF` — 5 symbols (TX_4X4)
   - `EOB_MULTI_64_ICDF` — 7 symbols (TX_8X8)
   - `EOB_MULTI_256_ICDF` — 9 symbols (TX_16X16)
   - `EOB_MULTI_512_ICDF` — 11 symbols (TX_32X32 and TX_64X64)

3. Add mutable CDF versions to `FrameCdfContext`

**Step 1: Write failing tests**

```rust
// In cdf_tables.rs tests:
#[test]
fn partition_cdf_dimensions() {
    assert_eq!(DO_SPLIT_PART_ICDF.len(), 2);      // 2 planes
    assert_eq!(DO_SPLIT_PART_ICDF[0].len(), 64);   // PARTITION_CONTEXTS
    assert_eq!(DO_SQUARE_SPLIT_ICDF.len(), 2);
    assert_eq!(DO_SQUARE_SPLIT_ICDF[0].len(), 8);   // SQUARE_SPLIT_CONTEXTS
}

#[test]
fn eob_multi_16_valid() {
    assert_eq!(EOB_MULTI_16_ICDF.len(), 3);         // 3 plane/context combos
    // Each entry has 5 symbols (eob groups for 16 coefficients)
    for icdf in &EOB_MULTI_16_ICDF {
        assert!(icdf.len() >= 4);
    }
}

// In cdf_context.rs tests:
#[test]
fn frame_cdf_context_has_partition_cdfs() {
    let ctx = FrameCdfContext::new();
    assert!(ctx.do_split_part.len() > 0);
    assert!(ctx.do_square_split.len() > 0);
}

#[test]
fn frame_cdf_context_has_eob_multi_16() {
    let ctx = FrameCdfContext::new();
    assert!(ctx.eob_multi_16.len() > 0);
}
```

**Step 2: Run tests, verify they fail**

**Step 3: Implement**

Extract ICDF values from `../av2/av2/common/entropy_inits_partition.h` and `entropy_inits_coeffs.h`.

For partition CDFs, extract the initial probability tables. The reference uses CDF format; we need to convert to ICDF format (our entropy coder uses ICDF).

For EOB CDFs, extract and convert similarly to how `EOB_MULTI_1024_ICDF` is already done.

Add to `FrameCdfContext`:
```rust
pub do_split_part: [[Vec<u16>; 64]; 2],
pub do_square_split: [[Vec<u16>; 8]; 2],
pub rect_type: [[Vec<u16>; 64]; 2],
pub do_ext_partition: [[Vec<u16>; 64]; 2],
pub do_uneven_4way: [[Vec<u16>; 64]; 2],
pub eob_multi_16: [Vec<u16>; 3],
pub eob_multi_64: [Vec<u16>; 3],
pub eob_multi_256: [Vec<u16>; 3],
pub eob_multi_512: [Vec<u16>; 3],
```

**Step 4: Run tests, verify pass**

Run: `cargo test --lib cdf_tables::tests && cargo test --lib cdf_context::tests`

**Step 5: Run clippy + commit**

```bash
cargo clippy --tests -- -D warnings
git add src/cdf_tables.rs src/cdf_context.rs
git commit -m "feat: add partition decision CDFs and EOB CDFs for all block sizes"
```

---

### Task 6: Recursive Partition Parser and Variable-Size Coefficient Decoder

**Files:**
- Major rewrite: `src/decode/tile.rs`
- Test: inline tests + roundtrip tests in `src/decode/tile.rs`

**What to build:**

This is the core task. Rewrite the tile decoder to:

1. **New data structures**: Replace `DecodedSbCoeffs` (fixed 1024-element arrays) with a tree of `DecodedBlock` structs that can represent variable-size blocks.

2. **Partition context**: Implement `partition_plane_context()` which computes the CDF context index from above/left neighbor partition state.

3. **Recursive partition parser**: `decode_partition(ec, bsize, row, col, ...)` that reads partition symbols and recursively descends.

4. **Variable-size coefficient decoder**: Generalize `decode_plane_full` to work with any `TxSize`, using the correct scan table and EOB CDF.

**New data structures:**

```rust
pub struct DecodedBlock {
    pub block_size: BlockSize,
    pub x: usize,          // pixel position within superblock
    pub y: usize,
    pub y_mode: u8,
    pub uv_mode: u8,
    pub y_levels: Vec<i32>,
    pub y_eob: u16,
    pub u_levels: Vec<i32>,
    pub u_eob: u16,
    pub v_levels: Vec<i32>,
    pub v_eob: u16,
}

pub struct DecodedSuperblock {
    pub blocks: Vec<DecodedBlock>,
}
```

**Partition context computation (from AV2 reference):**

The context depends on whether the above and left neighbors have been partitioned at the current level. For the initial implementation, use a simplified context model:
- Track a `partition_grid` of `u8` values per 4×4 unit
- Context = `(above_ctx + left_ctx) % PARTITION_PLOFFSET` + `bsize_group * PARTITION_PLOFFSET`

Where `bsize_group` maps block sizes to groups:
- 64×64 → group 3
- 32×32 → group 2
- 16×16 → group 1
- 8×8 → group 0

**Partition decision tree:**

```rust
fn decode_partition_type(
    ec: &mut DaalaDecoder,
    bsize: BlockSize,
    ctx: usize,
    plane: usize,
    cdf_ctx: &mut Option<&mut FrameCdfContext>,
) -> PartitionType {
    let can_split = bsize != BlockSize::Block4x4 && bsize.is_square();
    if !can_split {
        return PartitionType::None;
    }

    // 1. do_split: split or NONE?
    let do_split = decode_partition_symbol(ec, "do_split", plane, ctx, cdf_ctx);
    if do_split == 0 {
        return PartitionType::None;
    }

    // 2. do_square_split: quad split or rect?
    let sq_ctx = ... ; // simplified context for square split
    let do_sq_split = decode_partition_symbol(ec, "do_square_split", plane, sq_ctx, cdf_ctx);
    if do_sq_split == 1 {
        return PartitionType::Split;
    }

    // 3. rect_type: HORZ or VERT?
    let rect = decode_partition_symbol(ec, "rect_type", plane, ctx, cdf_ctx);

    // 4. do_ext_partition: 2-way or extended?
    if bsize.width() >= 16 { // extended partitions only for >= 16x16
        let do_ext = decode_partition_symbol(ec, "do_ext_partition", plane, ctx, cdf_ctx);
        if do_ext == 1 {
            // 5. do_uneven_4way + literal bit
            let do_4way = decode_partition_symbol(ec, "do_uneven_4way", plane, ctx, cdf_ctx);
            if do_4way == 0 {
                return if rect == 0 { PartitionType::Horz3 } else { PartitionType::Vert3 };
            } else {
                let ab = ec.decode_literal(1);
                return match (rect, ab) {
                    (0, 0) => PartitionType::Horz4A,
                    (0, 1) => PartitionType::Horz4B,
                    (1, 0) => PartitionType::Vert4A,
                    _ => PartitionType::Vert4B,
                };
            }
        }
    }

    if rect == 0 { PartitionType::Horz } else { PartitionType::Vert }
}
```

**Recursive descent:**

```rust
fn decode_partition(
    ec: &mut DaalaDecoder,
    bsize: BlockSize,
    row: usize,
    col: usize,
    partition_grid: &mut Vec<Vec<u8>>,
    cdf_ctx: &mut Option<&mut FrameCdfContext>,
    blocks: &mut Vec<DecodedBlock>,
    ... // context tracking arrays
) {
    let part_type = decode_partition_type(ec, bsize, ctx, 0, cdf_ctx);
    match part_type {
        PartitionType::None => {
            decode_block(ec, bsize, row, col, cdf_ctx, blocks, ...);
        }
        PartitionType::Split => {
            let sub = subsize(PartitionType::Split, bsize).unwrap();
            let half = bsize.width() / 2;
            decode_partition(ec, sub, row, col, ...);
            decode_partition(ec, sub, row, col + half, ...);
            decode_partition(ec, sub, row + half, col, ...);
            decode_partition(ec, sub, row + half, col + half, ...);
        }
        PartitionType::Horz => {
            let sub = subsize(PartitionType::Horz, bsize).unwrap();
            let half_h = bsize.height() / 2;
            decode_block(ec, sub, row, col, ...);
            decode_block(ec, sub, row + half_h, col, ...);
        }
        PartitionType::Vert => {
            let sub = subsize(PartitionType::Vert, bsize).unwrap();
            let half_w = bsize.width() / 2;
            decode_block(ec, sub, row, col, ...);
            decode_block(ec, sub, row, col + half_w, ...);
        }
        // ... handle Horz3, Vert3, Horz4A, Horz4B, Vert4A, Vert4B
    }
}
```

**Variable-size `decode_plane_coeffs`:**

Parameterize the existing `decode_plane_full` to accept `TxSize`:
- Use `tx_size.bwl()` instead of hardcoded `5`
- Use `tx_size.width()` for height
- Use `get_scan(tx_size)` instead of `DEFAULT_SCAN_32X32`
- Select the correct EOB CDF based on `tx_size`
- Allocate `levels` and `levels_buf` with `tx_size.num_coeffs()` entries

**Step 1: Write failing tests**

```rust
#[test]
fn decode_partition_none_block() {
    // Encode a single block with PartitionType::None at 64x64
    // Verify it decodes back as a single block
}

#[test]
fn decode_partition_split_creates_4_children() {
    // Encode a 64x64 with SPLIT → four 32x32 blocks
    // Verify 4 decoded blocks
}

#[test]
fn variable_size_coeff_decode_4x4() {
    // Encode coefficients for a 4x4 block
    // Verify roundtrip decode
}

#[test]
fn backward_compat_flat_sb() {
    // Existing roundtrip tests should still pass
    // (The old encode functions still emit flat 64x64 blocks)
}
```

**Step 2: Run tests, verify they fail**

**Step 3: Implement the full partition parser**

This is the largest implementation task. Key points:
- Keep the old `DecodedSbCoeffs` and `decode_tile_data_multi_sb` for backward compatibility
- Add new `decode_tile_data_partitioned` that returns `Vec<DecodedSuperblock>`
- The new path is used when decoding avmenc bitstreams (which have `disable_cdf_update=false`)

**Step 4: Run ALL tests (old + new)**

Run: `cargo test`
Expected: all old tests still pass + new partition tests pass

**Step 5: Run clippy + commit**

```bash
cargo clippy --tests -- -D warnings
git add src/decode/tile.rs
git commit -m "feat: add recursive partition tree parser and variable-size coefficient decoder"
```

---

### Task 7: Frame Reconstruction with Partition Tree and avmenc Integration

**Files:**
- Modify: `src/decode/mod.rs`
- Test: integration tests in `src/decode/mod.rs`

**What to build:**

1. New `reconstruct_frame_partitioned` that iterates over `DecodedSuperblock` trees instead of flat `DecodedSbCoeffs`.

2. For each `DecodedBlock`:
   - Compute DC prediction at the block's actual position and size
   - Call `dequantize_and_inverse_var` with the block's `TxSize`
   - Add residual to prediction
   - Write reconstructed pixels to the frame buffer

3. Route avmenc bitstreams (which have `disable_cdf_update=false` and partition trees) through the new path.

4. Integration test: decode avmenc test photo, verify non-gray output.

**Key reconstruction logic:**

```rust
fn reconstruct_block_var(
    plane: &mut [u8],
    plane_width: usize,
    block_x: usize,
    block_y: usize,
    block_w: usize,
    block_h: usize,
    prediction: &[u8],
    residuals: &[i32],
) {
    for row in 0..block_h {
        for col in 0..block_w {
            let pred = prediction[row * block_w + col] as i32;
            let res = residuals[row * block_w + col];
            let pixel = (pred + res).clamp(0, 255) as u8;
            plane[(block_y + row) * plane_width + (block_x + col)] = pixel;
        }
    }
}
```

For DC prediction at variable sizes, reuse the existing `predict_intra_block` with the block's actual dimensions.

**Step 1: Write failing tests**

```rust
#[test]
fn avmenc_decode_non_gray() {
    let data = std::fs::read("/tmp/eu_avmenc.obu").expect("avmenc test file");
    let frames = Decoder::decode_all(&data).unwrap();
    assert_eq!(frames.len(), 1);
    // Should NOT be all gray (128) anymore
    let avg_y: f64 = frames[0].y.iter().map(|&p| p as f64).sum::<f64>() / frames[0].y.len() as f64;
    assert!(
        (avg_y - 128.0).abs() > 5.0,
        "Frame is still gray (avg Y = {:.1}), partition parsing not working",
        avg_y,
    );
}

#[test]
fn avmenc_decode_produces_recognizable_output() {
    let data = std::fs::read("/tmp/eu_avmenc.obu").expect("avmenc test file");
    let frames = Decoder::decode_all(&data).unwrap();
    assert_eq!(frames.len(), 1);
    // Check that the output has reasonable pixel variance
    let y = &frames[0].y;
    let mean = y.iter().map(|&p| p as f64).sum::<f64>() / y.len() as f64;
    let variance = y.iter().map(|&p| {
        let d = p as f64 - mean;
        d * d
    }).sum::<f64>() / y.len() as f64;
    assert!(variance > 100.0, "Output has too little variance ({:.1}), likely not decoded correctly", variance);
}
```

**Step 2: Run tests, verify they fail**

**Step 3: Implement**

- Add `reconstruct_frame_partitioned` function
- Modify `Decoder::decode_all` to try the partitioned path first, falling back to flat path
- Generate test output: `./target/release/wav2c_decode /tmp/eu_avmenc.obu` and run comparison script

**Step 4: Run ALL tests**

Run: `cargo test`

**Step 5: Visual validation**

```bash
cargo build --release
./target/release/wav2c_decode /tmp/eu_avmenc.obu
scripts/compare_decoders.sh -e /tmp/eu_avmenc.obu photo.png
```

Expected: blurry but recognizable photo (face, shoulders visible)

**Step 6: Run clippy + commit**

```bash
cargo clippy --tests -- -D warnings
git add src/decode/mod.rs
git commit -m "feat: add partition-aware frame reconstruction with avmenc integration"
```

---

## Notes for Implementers

### LF vs HF Boundary for Smaller Blocks

The LF/HF boundary (`LF_2D_LIM`, `LF_2D_LIM_UV`) is defined relative to the block size. For our current implementation these values are for 32×32 coefficient grids. For smaller blocks:
- TX_4X4: everything is LF (row+col < LF_2D_LIM is always true for 4×4)
- TX_8X8: check if LF boundaries need adjustment
- TX_16X16: check reference decoder's `is_lf_2d` behavior at this size

Consult `decodetxb.c` `read_coeffs_tx_intra` for exact per-size behavior.

### TxSize Context Index (`txs_ctx_idx`)

The `txs_ctx_idx` used for coefficient base CDF selection differs by transform size:
- TX_32X32/TX_64X64 (32×32 grid): `txs_ctx_idx = 1` for Y, `0` for UV (current behavior)
- TX_16X16: likely `txs_ctx_idx = 0` for Y
- TX_8X8, TX_4X4: may use different CDF arrays entirely

Verify against `decodetxb.c` `get_txs_ctx` function.

### Extended Partition Subsize

For 3-way and 4-way partitions, the sub-blocks have different sizes. Example for HORZ_3 on 64×64:
- Top: 64×16 (quarter height)
- Middle: 64×32 (half height)
- Bottom: 64×16 (quarter height)

These produce rectangular blocks that need rectangular transforms (Phase 5). For Phase 4, we can use the largest square transform that fits, or treat them as multiple smaller square blocks.

### Backward Compatibility

All existing roundtrip tests must continue to pass. The encoder still writes flat 64×64/32×32 blocks with `PARTITION_NONE`. The new partition parsing only activates when decoding avmenc bitstreams.

### CDF Table Source Files

- Partition CDFs: `../av2/av2/common/entropy_inits_partition.h`
- EOB CDFs: `../av2/av2/common/entropy_inits_coeffs.h`
- Scan tables: `../av2/av2/common/scan.c`
- DCT kernels: `../av2/av2/common/txb_common.c`
- Shift tables: `../av2/av2/common/av2_txfm.h`
