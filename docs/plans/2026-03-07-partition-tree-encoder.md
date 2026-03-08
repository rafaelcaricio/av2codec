# Encoder Block Partitioning — Implementation Plan

## Goal
Add PARTITION_SPLIT support to the encoder so 64×64 Y superblocks can be recursively split into 32×32, 16×16, and 8×8 blocks. Each sub-block gets its own prediction mode and transform at native resolution, eliminating pixel doubling.

## Current State
- Encoder: fixed 64×64 Y / 32×32 UV per superblock
- Bitstream: writes `do_split=0` (PARTITION_NONE) for every SB
- Forward DCT: 4×4, 8×8, 16×16, 32×32 all available
- Scan tables: all sizes available
- CDF tables: partition CDFs already in cdf_tables.rs

## Key Insight
The existing simple tile encoder/decoder (`encode_tile_data_multi_sb` / `decode_tile_data_multi_sb`) writes a paired format where both sides agree on the symbol order. For partitioning, we need to write the full AV2 partition syntax so avmdec can decode it. The CDFs must match what avmdec expects (context-dependent from `DO_SPLIT_PART_ICDF`).

## Approach: Recursive Encoder with AV2-compatible Partition Syntax

### For Y plane (luma):
1. Start with 64×64 SB
2. Try PARTITION_NONE: encode as one 64×64 block (32×32 DCT + pixel double)
3. Try PARTITION_SPLIT: encode as four 32×32 blocks (each with 32×32 DCT at native res)
4. Recurse: each 32×32 can split into four 16×16, each 16×16 into four 8×8
5. Choose partition with best RD cost (distortion + λ × rate)
6. Minimum block: 8×8

### For UV plane (chroma):
- Stay at 32×32 per SB for now (matches 4:2:0 subsampling)
- The UV block covers the full SB's chroma area regardless of Y partitioning

### Bitstream format per SB:
```
[partition tree for Y - recursive do_split / do_square_split symbols]
  [for each Y leaf block: y_mode_set, y_mode_idx, Y coefficients]
[uv_mode]
[U coefficients (32×32)]
[V coefficients (32×32)]
```

### Tasks:
1. Add forward DCT + quantize + dequant at variable sizes
2. Add partition context tracking for encoder
3. Implement recursive RD partition decision
4. Update tile encoder to write partition tree
5. Update simple tile decoder to read partition tree
6. Integration test with avmdec
