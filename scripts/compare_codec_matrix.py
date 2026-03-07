#!/usr/bin/env -S uv run --no-project --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "pillow>=11,<12",
# ]
# ///

from __future__ import annotations

import argparse
import shlex
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


@dataclass
class Toolchain:
    project_dir: Path
    avmenc: Path
    avmdec: Path
    wav2c: Path
    wav2c_decode: Path


@dataclass
class PreparedFrame:
    input_y4m: Path
    original_png: Path
    width: int
    height: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Encode one image with avmenc and wav2c, decode both bitstreams with avmdec and wav2c_decode, and render a labeled 2x3 comparison matrix."
    )
    parser.add_argument("input", type=Path, help="Input image path")
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=Path("/tmp/wav2c_codec_matrix.png"),
        help="Output PNG path (default: /tmp/wav2c_codec_matrix.png)",
    )
    parser.add_argument(
        "-s",
        "--size",
        type=parse_size,
        help="Resize and center-crop to WxH before encode",
    )
    parser.add_argument(
        "-q",
        "--quality",
        type=int,
        default=30,
        help="Quality value applied to avmenc --qp and wav2c --qindex (default: 30)",
    )
    parser.add_argument(
        "--cpu-used",
        type=int,
        default=8,
        help="avmenc speed setting (default: 8)",
    )
    parser.add_argument(
        "--scale-up",
        type=int,
        default=2,
        help="Nearest-neighbor upscale factor for the comparison grid (default: 2)",
    )
    parser.add_argument(
        "--font",
        type=Path,
        help="Optional .ttf or .ttc font path for labels",
    )
    parser.add_argument(
        "--workdir",
        type=Path,
        help="Optional work directory for intermediates",
    )
    parser.add_argument(
        "--keep-workdir",
        action="store_true",
        help="Keep intermediate files after completion",
    )
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="Skip cargo build and use existing release binaries",
    )
    args = parser.parse_args()
    if args.scale_up < 1:
        parser.error("--scale-up must be >= 1")
    if args.quality < 0:
        parser.error("--quality must be >= 0")
    if args.cpu_used < 0:
        parser.error("--cpu-used must be >= 0")
    return args


def parse_size(value: str) -> tuple[int, int]:
    text = value.lower()
    if "x" not in text:
        raise argparse.ArgumentTypeError("size must look like WIDTHxHEIGHT")
    left, right = text.split("x", 1)
    try:
        width = int(left)
        height = int(right)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("size must look like WIDTHxHEIGHT") from exc
    if width <= 0 or height <= 0:
        raise argparse.ArgumentTypeError("size values must be > 0")
    return width, height


def main() -> int:
    args = parse_args()
    toolchain = resolve_toolchain()
    ensure_input_exists(args.input)
    if not args.no_build:
        build_wav2c(toolchain.project_dir)

    temp_dir: tempfile.TemporaryDirectory[str] | None = None
    keep_workdir = args.keep_workdir or args.workdir is not None
    if args.workdir is not None:
        workdir = args.workdir.resolve()
        workdir.mkdir(parents=True, exist_ok=True)
    else:
        temp_dir = tempfile.TemporaryDirectory(prefix="wav2c_codec_matrix.", dir="/tmp")
        workdir = Path(temp_dir.name)

    try:
        prepared = prepare_frame(args.input.resolve(), workdir, args.size)
        avmenc_obu = workdir / "avmenc.obu"
        wav2c_obu = workdir / "wav2c.obu"
        avmenc_log = workdir / "avmenc_encode.log"
        wav2c_log = workdir / "wav2c_encode.log"
        print(f"[1/7] Prepared frame at {prepared.width}x{prepared.height}")
        print("[2/7] Encoding with avmenc")
        run_command(
            [
                str(toolchain.avmenc),
                "--obu",
                "--limit=1",
                f"--qp={args.quality}",
                "--end-usage=q",
                "--bit-depth=8",
                "--full-still-picture-hdr",
                "--disable-warning-prompt",
                f"--cpu-used={args.cpu_used}",
                "--quiet",
                "-w",
                str(prepared.width),
                "-h",
                str(prepared.height),
                "-o",
                str(avmenc_obu),
                str(prepared.input_y4m),
            ],
            log_path=avmenc_log,
        )
        print("[3/7] Encoding with wav2c")
        run_command(
            [
                str(toolchain.wav2c),
                str(prepared.input_y4m),
                str(wav2c_obu),
                "--qindex",
                str(args.quality),
            ],
            log_path=wav2c_log,
        )

        decode_jobs = [
            (
                "avmenc -> avmdec",
                [str(toolchain.avmdec), str(avmenc_obu), "-o", str(workdir / "avmenc__avmdec.y4m")],
                workdir / "avmenc__avmdec.log",
            ),
            (
                "avmenc -> wav2c",
                [str(toolchain.wav2c_decode), str(avmenc_obu), "-o", str(workdir / "avmenc__wav2c.y4m")],
                workdir / "avmenc__wav2c.log",
            ),
            (
                "wav2c -> avmdec",
                [str(toolchain.avmdec), str(wav2c_obu), "-o", str(workdir / "wav2c__avmdec.y4m")],
                workdir / "wav2c__avmdec.log",
            ),
            (
                "wav2c -> wav2c",
                [str(toolchain.wav2c_decode), str(wav2c_obu), "-o", str(workdir / "wav2c__wav2c.y4m")],
                workdir / "wav2c__wav2c.log",
            ),
        ]
        print("[4/7] Decoding all four combinations")
        for label, command, log_path in decode_jobs:
            print(f"      {label}")
            run_command(command, log_path=log_path)

        print("[5/7] Converting decode outputs to PNG")
        pngs = {
            "original": prepared.original_png,
            "avmenc_avmdec": workdir / "avmenc__avmdec.png",
            "avmenc_wav2c": workdir / "avmenc__wav2c.png",
            "wav2c_avmdec": workdir / "wav2c__avmdec.png",
            "wav2c_wav2c": workdir / "wav2c__wav2c.png",
        }
        convert_y4m_to_png(workdir / "avmenc__avmdec.y4m", pngs["avmenc_avmdec"])
        convert_y4m_to_png(workdir / "avmenc__wav2c.y4m", pngs["avmenc_wav2c"])
        convert_y4m_to_png(workdir / "wav2c__avmdec.y4m", pngs["wav2c_avmdec"])
        convert_y4m_to_png(workdir / "wav2c__wav2c.y4m", pngs["wav2c_wav2c"])

        print("[6/7] Rendering labeled comparison grid")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        output_path = args.output.resolve()
        render_matrix(
            output_path=output_path,
            images=pngs,
            size=(prepared.width, prepared.height),
            scale_up=args.scale_up,
            quality=args.quality,
            avmenc_size=avmenc_obu.stat().st_size,
            wav2c_size=wav2c_obu.stat().st_size,
            font_path=args.font,
        )

        print("[7/7] Done")
    finally:
        if temp_dir is not None and not keep_workdir:
            temp_dir.cleanup()

    print(f"Output: {output_path}")
    if keep_workdir:
        print(f"Workdir: {workdir}")
    else:
        print("Workdir: cleaned up")

    return 0


def resolve_toolchain() -> Toolchain:
    script_path = Path(__file__).resolve()
    project_dir = script_path.parent.parent
    avm_build = project_dir.parent / "avm_build"
    toolchain = Toolchain(
        project_dir=project_dir,
        avmenc=avm_build / "avmenc",
        avmdec=avm_build / "avmdec",
        wav2c=project_dir / "target" / "release" / "wav2c",
        wav2c_decode=project_dir / "target" / "release" / "wav2c_decode",
    )
    for tool in ("ffmpeg", "ffprobe", "cargo"):
        if shutil.which(tool) is None:
            raise SystemExit(f"Missing required tool in PATH: {tool}")
    if not toolchain.avmenc.exists():
        raise SystemExit(f"Missing avmenc at {toolchain.avmenc}")
    if not toolchain.avmdec.exists():
        raise SystemExit(f"Missing avmdec at {toolchain.avmdec}")
    return toolchain


def ensure_input_exists(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"Input file not found: {path}")


def build_wav2c(project_dir: Path) -> None:
    print("[build] cargo build --release --bin wav2c --bin wav2c_decode")
    run_command(
        [
            "cargo",
            "build",
            "--release",
            "--bin",
            "wav2c",
            "--bin",
            "wav2c_decode",
            "--quiet",
        ],
        cwd=project_dir,
        log_path=project_dir / "target" / "compare_codec_matrix_build.log",
    )


def prepare_frame(input_path: Path, workdir: Path, size: tuple[int, int] | None) -> PreparedFrame:
    input_y4m = workdir / "input.y4m"
    original_png = workdir / "original.png"
    filter_chain = frame_filter(size, y4m=True)
    run_command(
        [
            "ffmpeg",
            "-y",
            "-v",
            "error",
            "-i",
            str(input_path),
            "-vf",
            filter_chain,
            "-frames:v",
            "1",
            str(input_y4m),
        ]
    )
    run_command(
        [
            "ffmpeg",
            "-y",
            "-v",
            "error",
            "-i",
            str(input_y4m),
            "-frames:v",
            "1",
            str(original_png),
        ]
    )
    width, height = probe_dimensions(input_y4m)
    return PreparedFrame(input_y4m=input_y4m, original_png=original_png, width=width, height=height)


def frame_filter(size: tuple[int, int] | None, *, y4m: bool) -> str:
    if size is None:
        base = "scale=trunc(iw/2)*2:trunc(ih/2)*2"
    else:
        width, height = size
        base = (
            f"scale={width}:{height}:force_original_aspect_ratio=increase:flags=lanczos,"
            f"crop={width}:{height}"
        )
    if y4m:
        return f"{base},format=yuv420p"
    return base


def probe_dimensions(path: Path) -> tuple[int, int]:
    result = run_command(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0:s=x",
            str(path),
        ]
    )
    line = next((line.strip() for line in result.stdout.splitlines() if line.strip()), "")
    if "x" not in line:
        raise SystemExit(f"Unable to read dimensions from {path}")
    width_text, height_text = line.split("x", 1)
    return int(width_text), int(height_text)


def convert_y4m_to_png(y4m_path: Path, png_path: Path) -> None:
    run_command(
        [
            "ffmpeg",
            "-y",
            "-v",
            "error",
            "-i",
            str(y4m_path),
            "-frames:v",
            "1",
            str(png_path),
        ]
    )


def run_command(
    command: list[str],
    *,
    cwd: Path | None = None,
    log_path: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        cwd=str(cwd) if cwd is not None else None,
        capture_output=True,
        text=True,
    )
    combined = f"{result.stdout}{result.stderr}"
    if log_path is not None:
        log_path.parent.mkdir(parents=True, exist_ok=True)
        log_path.write_text(combined)
    if result.returncode != 0:
        print(f"Command failed: {shlex.join(command)}", file=sys.stderr)
        if log_path is not None:
            print(f"Log: {log_path}", file=sys.stderr)
        if combined.strip():
            print(combined, file=sys.stderr)
        raise SystemExit(result.returncode)
    return result


def render_matrix(
    *,
    output_path: Path,
    images: dict[str, Path],
    size: tuple[int, int],
    scale_up: int,
    quality: int,
    avmenc_size: int,
    wav2c_size: int,
    font_path: Path | None,
) -> None:
    source_width, source_height = size
    scaled_width = source_width * scale_up
    scaled_height = source_height * scale_up
    title_height = max(70, 34 * scale_up)
    subtitle_height = max(30, 16 * scale_up)
    label_height = max(42, 20 * scale_up)
    gap = max(12, 6 * scale_up)
    margin = max(16, 8 * scale_up)
    cell_border = max(2, scale_up)
    background = (18, 20, 24)
    card = (32, 36, 44)
    label_bar = (44, 49, 61)
    border = (76, 86, 104)
    text = (245, 247, 250)
    title = (255, 255, 255)
    canvas_width = margin * 2 + scaled_width * 3 + gap * 2
    canvas_height = (
        margin * 2
        + title_height
        + subtitle_height
        + gap
        + (label_height + scaled_height) * 2
        + gap
    )
    canvas = Image.new("RGB", (canvas_width, canvas_height), background)
    draw = ImageDraw.Draw(canvas)

    title_font = pick_font(font_path, 28 * scale_up, "wav2c / avmenc / avmdec matrix")
    subtitle_font = pick_font(
        font_path,
        14 * scale_up,
        f"{source_width}x{source_height}   q={quality}   avmenc={format_bytes(avmenc_size)}   wav2c={format_bytes(wav2c_size)}",
    )
    draw_text_centered(
        draw,
        (margin, margin, canvas_width - margin, margin + title_height),
        "wav2c / avmenc / avmdec matrix",
        title_font,
        title,
    )
    draw_text_centered(
        draw,
        (margin, margin + title_height, canvas_width - margin, margin + title_height + subtitle_height),
        f"{source_width}x{source_height}   q={quality}   avmenc={format_bytes(avmenc_size)}   wav2c={format_bytes(wav2c_size)}",
        subtitle_font,
        text,
    )

    labels = [
        [("Original", images["original"]), ("avmenc -> avmdec", images["avmenc_avmdec"]), ("avmenc -> wav2c", images["avmenc_wav2c"])],
        [("Original", images["original"]), ("wav2c -> avmdec", images["wav2c_avmdec"]), ("wav2c -> wav2c", images["wav2c_wav2c"])],
    ]

    row_start_y = margin + title_height + subtitle_height + gap
    for row_index, row in enumerate(labels):
        y = row_start_y + row_index * (label_height + scaled_height + gap)
        for col_index, (label, path) in enumerate(row):
            x = margin + col_index * (scaled_width + gap)
            cell = render_cell(
                image_path=path,
                label=label,
                scaled_size=(scaled_width, scaled_height),
                label_height=label_height,
                colors=(card, label_bar, border, text),
                border_width=cell_border,
                font_path=font_path,
            )
            canvas.paste(cell, (x, y))

    canvas.save(output_path)


def render_cell(
    *,
    image_path: Path,
    label: str,
    scaled_size: tuple[int, int],
    label_height: int,
    colors: tuple[tuple[int, int, int], tuple[int, int, int], tuple[int, int, int], tuple[int, int, int]],
    border_width: int,
    font_path: Path | None,
) -> Image.Image:
    card, label_bar, border, text = colors
    scaled_width, scaled_height = scaled_size
    image = Image.open(image_path).convert("RGB").resize((scaled_width, scaled_height), Image.Resampling.NEAREST)
    cell = Image.new("RGB", (scaled_width, label_height + scaled_height), card)
    draw = ImageDraw.Draw(cell)
    draw.rectangle((0, 0, scaled_width - 1, label_height + scaled_height - 1), outline=border, width=border_width)
    draw.rectangle((0, 0, scaled_width - 1, label_height - 1), fill=label_bar)
    label_font = best_fit_font(font_path, label, scaled_width - 20, max(14, label_height - 10))
    draw_text_centered(draw, (8, 0, scaled_width - 8, label_height), label, label_font, text)
    cell.paste(image, (0, label_height))
    return cell


def draw_text_centered(
    draw: ImageDraw.ImageDraw,
    box: tuple[int, int, int, int],
    text: str,
    font: ImageFont.ImageFont,
    fill: tuple[int, int, int],
) -> None:
    left, top, right, bottom = box
    bbox = draw.textbbox((0, 0), text, font=font, stroke_width=2)
    text_width = bbox[2] - bbox[0]
    text_height = bbox[3] - bbox[1]
    x = left + (right - left - text_width) / 2
    y = top + (bottom - top - text_height) / 2 - bbox[1]
    draw.text((x, y), text, font=font, fill=fill, stroke_width=2, stroke_fill=(0, 0, 0))


def best_fit_font(font_path: Path | None, text: str, max_width: int, preferred_size: int) -> ImageFont.ImageFont:
    measure = ImageDraw.Draw(Image.new("RGB", (8, 8), (0, 0, 0)))
    for size in range(preferred_size, 11, -1):
        font = pick_font(font_path, size, text)
        bbox = measure.textbbox((0, 0), text, font=font, stroke_width=2)
        if bbox[2] - bbox[0] <= max_width:
            return font
    return pick_font(font_path, 12, text)


def pick_font(font_path: Path | None, size: int, sample_text: str) -> ImageFont.ImageFont:
    candidates: list[Path] = []
    if font_path is not None:
        candidates.append(font_path)
    match = fontconfig_match()
    if match is not None:
        candidates.append(match)
    candidates.extend(
        [
            Path("/System/Library/Fonts/Supplemental/Verdana Bold.ttf"),
            Path("/System/Library/Fonts/Supplemental/Arial Bold.ttf"),
            Path("/System/Library/Fonts/Supplemental/Arial.ttf"),
            Path("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
            Path("/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf"),
        ]
    )
    seen: set[Path] = set()
    for candidate in candidates:
        if candidate in seen or not candidate.exists():
            continue
        seen.add(candidate)
        try:
            return ImageFont.truetype(str(candidate), size=size)
        except OSError:
            continue
    fallback = ImageFont.load_default()
    try:
        fallback.getbbox(sample_text)
    except Exception:
        pass
    return fallback


def fontconfig_match() -> Path | None:
    executable = shutil.which("fc-match")
    if executable is None:
        return None
    result = subprocess.run(
        [executable, "-f", "%{file}\n", "sans:style=Bold"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return None
    path = result.stdout.strip()
    if not path:
        return None
    candidate = Path(path)
    if not candidate.exists():
        return None
    return candidate


def format_bytes(value: int) -> str:
    if value < 1024:
        return f"{value} B"
    if value < 1024 * 1024:
        return f"{value / 1024:.1f} KiB"
    return f"{value / (1024 * 1024):.2f} MiB"


if __name__ == "__main__":
    raise SystemExit(main())
