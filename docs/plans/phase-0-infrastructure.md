# Phase 0: Infrastructure

## Goal

Set up all tooling and implement the lowest-level building blocks that every
subsequent phase depends on.

## Scope

### 0.1 Build avmdec (decoder-only) from reference AVM source
- CMake build at /Users/rafaelcaricio/development/av2
- Decoder-only: CONFIG_AV2_ENCODER=0
- Build target: avmdec binary
- Verify it runs

### 0.2 Create test frame tooling
- Generate a solid green 64x64 YUV 4:2:0 raw frame using FFmpeg
- Store at tests/fixtures/
- Script or Makefile target for regeneration

### 0.3 Implement bit-level writer (BitWriter)
- Write individual bits, fixed-width integers, LEB128
- Write to a growable byte buffer
- Unit tests for correctness

### 0.4 Implement OBU writer
- OBU header (type, extension flag, has_size flag)
- OBU size in LEB128
- Trailing bits (byte alignment)
- Unit tests

### 0.5 Implement minimal sequence header
- Still picture mode, Profile 0, 8-bit 4:2:0
- All optional tools disabled
- Hardcoded for 64x64 frame initially
- Unit test: write sequence header, verify byte pattern

### 0.6 Implement minimal frame header + empty tile group
- KEY_FRAME, show_frame=1, fixed QP
- Single tile, empty compressed data
- Pack into OBU_REGULAR_TILE_GROUP

### 0.7 Integration test: avmdec parses the bitstream
- Combine sequence header OBU + tile group OBU
- Feed to avmdec
- Goal: avmdec should at least parse headers without crashing
  (it may fail on tile data — that's Phase 1's problem)

## Success Criteria

- avmdec binary built and functional
- Bit writer produces correct output (unit tested)
- OBU framing produces valid OBU structure
- Sequence header is parseable by avmdec
