# av2codec — AV2 Codec Development Protocol

## Core Principle: AV2 Specification Compliance

av2codec MUST BE SPECIFICATION COMPLIANT. Both the encoder and decoder must produce and consume valid AV2 bitstreams. The decoder must accept ANY AV2-compliant bitstream — it may skip or ignore unsupported codec tools, but the tools that ARE implemented must work correctly. av2codec will gradually evolve to implement more AV2 tools, but what exists must be correct.

## Goal

Encode intra-only frames to AV2, decodable by the reference avmdec decoder. Decode any AV2-compliant intra-only bitstream.

## Autonomous Closed-Loop Protocol

Execute phases sequentially. Each phase follows this cycle:

1. **Plan** — Define scope, identify what changes in headers/encoding/container
2. **Research** — Use a subagent to study AV2 codebase and avmdec decoder expectations
3. **Document** — Use a subagent to write implementation plan to `docs/plans/`
4. **Implement** — Subagent-driven development with code review per task
5. **Validate** — Use real images from `../image-set/png` to generate test artifacts, decode with avmdec, write regression tests
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
- Must execute an end-to-end transcoding with av2codec and decode with reference avmdec; When necessary create test frames with FFmpeg

## Test Strategy

- Unit tests for each module (bitwriter, obu, entropy, cdf, tile, frame, sequence)
- Integration tests that encode → decode with avmdec → verify pixel values
- Use real images from `../image-set/png`
- Every new phase must not break tests from previous phases

## Key Technical Reference

- AVM reference source: `../av2/` (encoder and decoder reference implementation)
- avmdec binary: `/Users/rafaelcaricio/development/avm_build/avmdec`
- avmdec usage: `/Users/rafaelcaricio/development/avm_build/avmdec -o /tmp/output.y4m <input.obu>`
- avmdec raw output: `/Users/rafaelcaricio/development/avm_build/avmdec --rawvideo -o /tmp/output.raw <input.obu>`
- AV2 bitstream format: OBU-based (Open Bitstream Units)
