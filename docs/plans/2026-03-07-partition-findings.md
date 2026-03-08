# AV2 Partition Tree Findings

## Key Discovery: No Quad-Split at 64×64

In AV2, `PARTITION_SPLIT` (quad-split into 4 equal sub-blocks) is **only allowed for BLOCK_128X128 and BLOCK_256X256**. At BLOCK_64X64 (our SB size), the allowed partitions are:

- `PARTITION_NONE` — one 64×64 block
- `PARTITION_HORZ` — two 64×32 blocks  
- `PARTITION_VERT` — two 32×64 blocks
- `PARTITION_HORZ_3` / `PARTITION_VERT_3` — 3-way T-partition (requires `enable_ext_partitions`)

This is defined in `av2/common/blockd.h:is_square_split_eligible()` which returns true only for 128×128+ blocks.

## Partition Syntax at 64×64

The decoder reads:
1. `do_split` (from `do_split_cdf[plane][ctx]`)
2. If split and PARTITION_SPLIT not allowed:
   - Read `rect_type` (HORZ=0, VERT=1) from `rect_type_cdf`
   - If `enable_ext_partitions`: read `do_ext` for 3-way partitions
3. Sub-blocks are rectangular (64×32 or 32×64), NOT square

## Getting to 32×32 Blocks

To get four 32×32 blocks from a 64×64 SB:
1. PARTITION_VERT at 64×64 → two 32×64 blocks
2. For each 32×64: PARTITION_HORZ → two 32×32 blocks

Or equivalently:
1. PARTITION_HORZ at 64×64 → two 64×32 blocks  
2. For each 64×32: PARTITION_VERT → two 32×32 blocks

## Infrastructure Built

- `EncodedBlock` struct with per-block UV data
- `encode_uv_subblock()` for 4×4, 8×8, 16×16, 32×32 UV blocks
- `encode_partition_and_coeffs()` — recursive partition tree + coefficients
- `encode_partition_rd()` — RD-based partition decisions
- `build_partition_tree()` — tree construction from block list
- Size-parameterized helpers: `txs_ctx_idx_for_bsize()`, `bwl_for_bsize()`, etc.
- `encode_eob_var_size()` — EOB encoding for 16/64/256/1024 coefficients

## Next Steps

1. Implement PARTITION_HORZ/VERT at 64×64 level
2. Write `rect_type` CDF symbol after `do_split`
3. Handle rectangular block coding (64×32, 32×64)
4. Enable recursive splitting to reach 32×32 blocks
