# Decoder Refmv-Bank Implementation Plan

## Goal

Replace the decoder's `intrabc_mv_history` approximation with AV2-style refmv-bank behavior for intra-only decoding so IntraBC DRL candidate selection matches avmdec more closely.

## Scope

- Sequence-header plumbing for `enable_refmvbank`
- Per-SB-row refmv-bank lifetime in intra-only decode, with per-SB hit-counter reset
- Candidate ordering: spatial, then bank, then defaults
- AV2-style bank update gating and LRU behavior
- Validation with unit coverage plus the existing decode-to-PSNR checks

## Reference Semantics To Mirror

- Parse and preserve `enable_refmvbank` instead of discarding the bit in [`src/decode/sequence.rs`](/Users/rafaelcaricio/development/wav2c/src/decode/sequence.rs).
- For intra-only decode, clear the bank once at the start of each SB row and reset only the hit counters at each SB. Do not prime from the SB row above.
- Keep the bank capacity at 4 entries, matching AV2 `REF_MV_BANK_SIZE`.
- Expose bank candidates newest-first, but maintain them with LRU semantics: duplicate moves to MRU, new entry appends, oldest entry is evicted when full.
- Apply AV2-style hit gating instead of the current sign-based heuristic:
  - `remain_hits`
  - `rmb_unit_hits`
  - `rmb_sb_hits`
  - first-unit / leftover limits equivalent to AV2's `BANK_1ST_UNIT_UPDATE_COUNT`, `BANK_UNIT_MAX_ALLOWED_LEFTOVER_UPDATES`, and `MAX_RMB_SB_HITS`

## Likely Touchpoints

- [`src/decode/sequence.rs`](/Users/rafaelcaricio/development/wav2c/src/decode/sequence.rs)
  - Add `enable_refmvbank` to `SequenceParams`
  - Return it from `parse_inter_group`
  - Thread it into the tile decode config instead of reading `_refmvbank` and dropping it
- [`src/decode/tile.rs`](/Users/rafaelcaricio/development/wav2c/src/decode/tile.rs)
  - Replace `intrabc_mv_history` with a dedicated refmv-bank state object
  - Add superblock-start reset
  - Change IntraBC candidate assembly
  - Add AV2-style bank update and unit-hit accounting
- Secondary wiring will likely be needed where `TileDecodeConfig` is built and where targeted decoder tests live, but the main behavior change is centered in the two files above.

## Implementation Steps

1. Sequence-header plumbing
- Extend `SequenceParams` with `enable_refmvbank: bool`.
- Change `parse_inter_group` to return the parsed bit instead of binding it to `_refmvbank`.
- Thread the flag through the existing decode config path so tile decoding can branch on spec signaling rather than hard-coded behavior.

2. Tile-local bank model
- Introduce a small refmv-bank type in `tile.rs` with:
  - fixed storage for 4 `(row, col)` BVs
  - `start_idx` or equivalent ring-buffer bookkeeping
  - `count`, `remain_hits`, `rmb_unit_hits`, `rmb_sb_hits`
- Remove `intrabc_mv_history`.
- Keep the state local to the tile decode path for now; only the intra-only subset is needed.

3. Row-local lifetime and per-SB counter reset
- Clear the bank once at the start of every SB row in intra-only decode.
- Reset `remain_hits`, `rmb_unit_hits`, and `rmb_sb_hits` at the start of each SB while keeping the row-local bank contents.
- Do not import candidates from the SB row above for this path.

4. Candidate ordering
- Keep the current spatial neighbor search first.
- After spatial candidates, append bank entries in most-recent-first order when `enable_refmvbank` is set.
- Append the existing AV2 default BVs last, still capped to `max_bvp_drl_bits + 1`.
- Preserve deduplication across all three sources so bank candidates never duplicate an earlier spatial or default entry.

5. Bank update behavior
- Split the current `fill_intrabc` responsibility so mode-grid writes stay separate from bank updates.
- After decoding an intra-only luma/shared-tree block:
  - if the block is IntraBC and `enable_refmvbank` is on, run the AV2-style bank update path
  - if the block is not IntraBC, still run the unit-hit accounting path so later IntraBC blocks in the same superblock see the correct update budget
- Remove the current `bv_row <= 0 && bv_col <= 0` gate. Bank eligibility should be driven by decoded block type plus AV2 hit rules, not by sign filtering.
- On duplicate BV, move it to MRU. On new BV, append and evict the oldest when the bank is full.

6. Targeted regression coverage
- Add a focused test around candidate assembly in `tile.rs`:
  - spatial candidates stay ahead of bank candidates
  - bank entries are emitted newest-first
  - duplicate insertion performs LRU promotion
  - superblock reset clears bank state for intra-only decode
- Add a sequence-header parsing check so a stream with `enable_refmvbank = 1` reaches the tile config intact.

## Validation

Run:

```bash
cargo test -q
cargo clippy --tests
cargo test -q decode_avmenc_128x128_q30
cargo test -q decode_avmenc_nofilter_psnr
cargo test -q decode_avmenc_noibc_psnr
```

Expected result:

- No regressions in existing decoder tests
- The PSNR tests remain green
- IntraBC candidate traces align more closely with avmdec when refmv-bank signaling is enabled
