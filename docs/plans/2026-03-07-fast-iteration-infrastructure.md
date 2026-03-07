# Fast Iteration Infrastructure Plan

## Goal

Create a repo-native iteration loop for `wav2c` that makes encoder and decoder changes cheap to verify against the authoritative `avmdec` path, with the same image rendered through all four relevant combinations:

- `avmenc -> avmdec`
- `avmenc -> wav2c`
- `wav2c -> avmdec`
- `wav2c -> wav2c`

The loop must optimize for repeated local runs after small code edits, not only for final validation before commit.

## Current Friction

The project already has the core pieces:

- [`scripts/compare_codec_matrix.py`](/Users/rafaelcaricio/development/wav2c/scripts/compare_codec_matrix.py) renders the right visual matrix.
- [`tests/avmdec_integration.rs`](/Users/rafaelcaricio/development/wav2c/tests/avmdec_integration.rs) provides end-to-end encode/decode coverage against `avmdec`.
- [`src/bin/wav2c.rs`](/Users/rafaelcaricio/development/wav2c/src/bin/wav2c.rs) and [`src/bin/wav2c_decode.rs`](/Users/rafaelcaricio/development/wav2c/src/bin/wav2c_decode.rs) expose the exact encoder/decoder binaries the loop should exercise.

What is missing is a fast path between editing code and seeing whether the change improved or regressed the current failure mode. The expensive parts today are:

- rebuilding release binaries every run
- re-encoding the reference bitstream with `avmenc` every run
- relying on visual inspection without stable numeric deltas
- manually managing intermediate artifacts and logs

## Proposed Workflow

Split validation into two layers.

### Layer 1: Fast local probe loop

This is the default loop during development.

1. Prepare one representative probe frame once from a source image.
2. Encode that prepared frame once with `avmenc`.
3. Decode the `avmenc` bitstream once with `avmdec`.
4. Cache those reference artifacts.
5. On each code change:
   - incrementally rebuild `wav2c` and `wav2c_decode`
   - encode the cached probe frame with `wav2c`
   - decode cached `avmenc.obu` with the current `wav2c_decode`
   - decode fresh `wav2c.obu` with both `avmdec` and `wav2c_decode`
   - render the comparison matrix
   - compute and print numeric deltas

This keeps the authoritative reference fixed while allowing the wav2c side to move quickly.

### Layer 2: Full regression gate

This remains the pre-commit barrier.

Run:

```bash
cargo test
cargo clippy --tests
cargo test --test avmdec_integration -- --ignored
```

Layer 1 is for iteration speed. Layer 2 is for correctness and regression protection.

## What The Fast Loop Should Measure

The loop should emit both images and numbers. The numbers matter because they tell us whether the problem is in the encoder, the decoder, or both.

Minimum metrics per run:

- original vs `wav2c -> avmdec`
- original vs `wav2c -> wav2c`
- `avmenc -> avmdec` vs `avmenc -> wav2c`
- `wav2c -> avmdec` vs `wav2c -> wav2c`
- `avmenc` bitstream size vs `wav2c` bitstream size

For each pair, report:

- Y / U / V PSNR
- whole-frame PSNR
- mean absolute error

This gives direct answers to four common questions:

- Did the encoder improve against the reference decoder?
- Did the internal decoder improve against the reference decoder?
- Did a change only help wav2c decoding of wav2c streams while leaving `avmdec` output unchanged?
- Did size move in a good or bad direction?

## Artifact Layout

Use a stable cache directory per probe case, for example under `/tmp/wav2c_iter/`.

Suggested layout:

```text
/tmp/wav2c_iter/<case-id>/
  reference/
    input.y4m
    original.png
    avmenc.obu
    avmenc__avmdec.y4m
    metadata.json
  latest/
    wav2c.obu
    avmenc__wav2c.y4m
    wav2c__avmdec.y4m
    wav2c__wav2c.y4m
    matrix.png
    metrics.txt
    metrics.json
    *.log
```

The reference directory is reused until one of these changes:

- source image
- resize/crop choice
- quality level
- `avmenc` encode settings

Everything in `latest/` is overwritten every run.

## Concrete Script Changes

Extend [`scripts/compare_codec_matrix.py`](/Users/rafaelcaricio/development/wav2c/scripts/compare_codec_matrix.py) instead of creating a parallel tool with overlapping responsibilities.

Add the following capabilities:

1. Persistent cache mode
- Add a flag for a stable cache/work directory.
- Reuse prepared input, `avmenc.obu`, and `avmenc -> avmdec` when inputs and settings match.

2. Fast build mode
- Allow debug-profile binaries for the inner loop.
- Keep release mode available for final confirmation.

3. Numeric report output
- Write both `metrics.txt` and `metrics.json`.
- Print a compact summary to stdout after the matrix is generated.

4. Watch mode
- Poll `src/`, `tests/`, [`Cargo.toml`](/Users/rafaelcaricio/development/wav2c/Cargo.toml), and the script itself.
- On change, rerun only the wav2c side while preserving the cached reference side.
- Stay alive after a failed build or failed decode so the next edit can immediately retrigger the run.

5. Failure-oriented logs
- Preserve one log file per subprocess.
- Print the exact failing command and the log path when a run fails.

## Expected Command Shape

One-shot run:

```bash
./scripts/compare_codec_matrix.py \
  -s 192x192 \
  -q 30 \
  --cache-dir /tmp/wav2c_iter/jellyfish \
  -o /tmp/wav2c_iter/jellyfish/latest/matrix.png \
  photo.jpg
```

Watch loop:

```bash
./scripts/compare_codec_matrix.py \
  -s 192x192 \
  -q 30 \
  --cache-dir /tmp/wav2c_iter/jellyfish \
  --watch \
  photo.jpg
```

The watch command becomes the default “edit -> save -> inspect matrix and metrics” loop.

## Probe Set

Start with a small fixed probe set instead of a large corpus.

Recommended probes:

- one photographic image with smooth gradients and textured detail
- one synthetic frame from `ffmpeg` `testsrc2`
- one high-contrast bars or sharp-edge pattern

The photo catches subjective blocking and directional prediction problems. The synthetic cases catch deterministic partitioning, coefficient, and plane-handling regressions.

The initial implementation should support one probe at a time. A future extension can add a multi-probe batch runner once the single-probe loop is stable.

## Validation Plan

After implementing the fast loop:

1. Generate a cached probe from a real image.
2. Run the loop once and confirm the matrix matches the current manual workflow.
3. Make a small intentional encoder perturbation and confirm:
   - `wav2c -> avmdec` metrics move
   - `avmenc -> wav2c` stays unchanged unless decoder code was touched
4. Make a small intentional decoder perturbation and confirm:
   - `avmenc -> wav2c` metrics move
   - `wav2c -> avmdec` stays unchanged unless encoder code was touched
5. Run the existing test gates:

```bash
cargo test
cargo clippy --tests
cargo test --test avmdec_integration -- --ignored
```

## Success Criteria

This infrastructure is successful when:

- the common inner loop avoids re-running `avmenc` unless the probe definition changed
- a save-and-rerun cycle produces fresh matrix output in seconds, not minutes
- every run leaves behind enough logs and artifacts to diagnose failures without rerunning blindly
- matrix inspection is paired with stable numeric metrics
- final correctness still flows through the existing `avmdec` integration tests

## Next Step After Infrastructure

Once this loop exists, use it to drive targeted encoder work on the current visible mismatch:

- if `wav2c -> avmdec` improves while `wav2c -> wav2c` tracks it, the encoder is improving
- if `avmenc -> wav2c` remains grossly wrong, keep decoder work separate from encoder tuning
- if both improve together, re-run the full ignored `avmdec` integration suite and preserve the new probe artifacts as regression evidence
