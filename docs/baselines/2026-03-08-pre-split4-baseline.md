# Baseline: Pre-Split4 Encoder (Fixed 32×32 Blocks)

Date: 2026-03-08
Encoder: fixed 64→32×32 partition (HORZ+VERT split, non-directional Y modes only)
Settings: q=50, 256×256 crops, debug profile, avmenc cpu-used=8

## Results

| Image   | avmenc size | wav2c size | Δ size  | avmenc Y PSNR | wav2c Y PSNR | Gap    |
|---------|------------|------------|---------|---------------|-------------|--------|
| baby    | 15.9 KiB   | 23.3 KiB   | +46.3%  | 52.26 dB      | 22.70 dB    | 29.6 dB |
| city    | 31.5 KiB   | 37.4 KiB   | +18.7%  | 51.68 dB      | 28.17 dB    | 23.5 dB |
| flowers | 43.7 KiB   | 46.3 KiB   | +6.1%   | 51.30 dB      | 26.00 dB    | 25.3 dB |
| house   | 9.4 KiB    | 15.9 KiB   | +69.1%  | 53.16 dB      | 40.95 dB    | 12.2 dB |
| sunset  | 10.1 KiB   | 16.2 KiB   | +60.6%  | 52.01 dB      | 43.87 dB    | 8.1 dB  |

## Key Observations

1. **Decoder agreement is perfect**: wav2c→avmdec vs wav2c→wav2c Y PSNR = ∞ (lossless)
2. **Quality gap is 8-30 dB** depending on image complexity
3. **Smooth images (house, sunset) are closest** — large uniform blocks work OK there
4. **Complex textures (baby, flowers, city) suffer most** — 32×32 blocks can't capture fine detail
5. **Bitstream overhead is 6-69%** — wav2c uses more bits for comparable or worse quality
6. **avmenc→wav2c decode is broken** (~11-14 dB) — wav2c decoder doesn't handle avmenc's partition trees

## What This Means for Split4

The biggest quality wins will come from:
- Adaptive partitioning (split complex regions into 16×16 or 8×8)
- Directional modes at smaller block sizes
- Better RD decisions to avoid wasting bits on smooth regions

Target: close the PSNR gap by at least 5-10 dB on complex images while keeping bitstream size growth modest.
