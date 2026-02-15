# wav2c — AV2 Encoder Development Protocol

## Goal

Encode intra-only frames to AV2, decodable by the reference avmdec decoder.

## Autonomous Closed-Loop Protocol

Execute phases sequentially. Each phase follows this cycle:

1. **Plan** — Define scope, identify what changes in headers/encoding/container
2. **Research** — Use a subagent to study AV2 codebase and avmdec decoder expectations
3. **Document** — Use a subagent to write implementation plan to `docs/plans/`
4. **Implement** — Subagent-driven development with code review per task
5. **Validate** — Generate test artifacts with ffmpeg, decode with avmdec, write regression tests
6. **Loop** — Commit, update roadmap, update documentation, proceed to next phase

## Development Rules

- Never stop to ask questions — proceed with best approach
- No code comments — self-documenting code through clear naming
- Tasks only done when code compiles with all features working
- Use subagent development for parallel work
- All tests must pass before committing (cargo test + cargo clippy --tests)
- Every phase adds automated regression tests that protect previous work
- avmdec at `../avm_build/avmdec` is the authoritative decoder
- Can add debug logging to avmdec for investigation, but encoder must work with stock avmdec
- No code changes that affect the AV2 decoding process are allowed in avmdec
- Must execute an end-to-end transcoding with wav2c and decode with reference avmdec; When necessary create test frames with FFmpeg

## Test Strategy

- Unit tests for each module (bitwriter, obu, entropy, cdf, tile, frame, sequence)
- Integration tests that encode → decode with avmdec → verify pixel values
- Generate reference inputs with ffmpeg (solid colors, gradients, real images)
- Every new phase must not break tests from previous phases

## Phase Roadmap

See `docs/plans/` for the full phase breakdown.

- Phase 0: Infrastructure [COMPLETE]
- Phase 1: Multi-frame all-intra encoding with DC coefficients [COMPLETE]
- Phase 2: AC coefficient support [NEXT]

## Key Technical Reference

- AVM reference source: `../av2/` (encoder and decoder reference implementation)
- avmdec binary: `../avm_build/avmdec`
- AV2 bitstream format: OBU-based (Open Bitstream Units)
- Video mode: `single_picture_header_flag = 0`, CLK OBUs with order_hint
- Profile 0: 8-bit 4:2:0
- Entropy coding: CDF-based arithmetic range coder (Daala-derived)
- CDF format: cumulative distribution functions with adaptation counter
- Sequence header disables: CDEF, GDF, restoration, intraBC, palette, all inter tools
- Intra modes: 13 base modes (DC, V, H, D45-D203, Smooth×3, Paeth)
- Transforms: DCT + ADST (integer arithmetic, no floating point in codec path)
