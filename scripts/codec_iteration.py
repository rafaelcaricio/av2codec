#!/usr/bin/env -S uv run --no-project --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "pillow>=11,<12",
# ]
# ///

from __future__ import annotations

import argparse
import hashlib
import json
import math
import shlex
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


@dataclass
class Y4mFrame:
    width: int
    height: int
    y: bytes
    u: bytes
    v: bytes


@dataclass
class Toolchain:
    project_dir: Path
    avmenc: Path
    avmdec: Path
    av2codec: Path
    av2codec_decode: Path


@dataclass
class PreparedFrame:
    input_y4m: Path
    original_png: Path
    width: int
    height: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Cached encoder iteration loop for av2codec. Reuses the avmenc reference side, rebuilds av2codec incrementally, computes PSNR metrics, and optionally watches the repo for changes."
    )
    parser.add_argument("input", type=Path, help="Input image or Y4M path")
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=Path("/tmp/av2codec_iteration.png"),
        help="Output PNG path for the matrix (default: /tmp/av2codec_iteration.png)",
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
        help="Quality value applied to avmenc --qp and av2codec --qindex (default: 30)",
    )
    parser.add_argument(
        "--cpu-used",
        type=int,
        default=8,
        help="avmenc speed setting (default: 8)",
    )
    parser.add_argument(
        "--profile",
        choices=["debug", "release"],
        default="debug",
        help="Cargo build profile for av2codec binaries (default: debug)",
    )
    parser.add_argument(
        "--scale-up",
        type=int,
        default=2,
        help="Nearest-neighbor upscale factor for the matrix (default: 2)",
    )
    parser.add_argument(
        "--workdir",
        type=Path,
        default=Path("/tmp/av2codec_iteration"),
        help="Persistent work directory for cached references and reports (default: /tmp/av2codec_iteration)",
    )
    parser.add_argument(
        "--report-json",
        type=Path,
        help="Optional JSON report path (default: <workdir>/last_run.json)",
    )
    parser.add_argument(
        "--refresh-reference",
        action="store_true",
        help="Rebuild the prepared frame and avmenc reference cache",
    )
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="Skip cargo build and use existing binaries",
    )
    parser.add_argument(
        "--no-matrix",
        action="store_true",
        help="Skip PNG conversion and matrix rendering; metrics only",
    )
    parser.add_argument(
        "--watch",
        action="store_true",
        help="Rerun automatically when Rust source files change",
    )
    parser.add_argument(
        "--watch-interval",
        type=float,
        default=0.75,
        help="Polling interval in seconds for --watch (default: 0.75)",
    )
    args = parser.parse_args()
    if args.quality < 0:
        parser.error("--quality must be >= 0")
    if args.cpu_used < 0:
        parser.error("--cpu-used must be >= 0")
    if args.scale_up < 1:
        parser.error("--scale-up must be >= 1")
    if args.watch_interval <= 0:
        parser.error("--watch-interval must be > 0")
    if args.report_json is None:
        args.report_json = args.workdir / "last_run.json"
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
    toolchain = resolve_toolchain(args.profile)
    ensure_input_exists(args.input)
    args.workdir.mkdir(parents=True, exist_ok=True)
    if args.watch:
        print(f"Watching {toolchain.project_dir} every {args.watch_interval:.2f}s")
        watch_loop(args, toolchain)
        return 0
    run_once(args, toolchain)
    return 0


def resolve_toolchain(profile: str) -> Toolchain:
    script_path = Path(__file__).resolve()
    project_dir = script_path.parent.parent
    avm_build = project_dir.parent / "avm_build"
    for tool in ("ffmpeg", "ffprobe", "cargo"):
        if shutil.which(tool) is None:
            raise SystemExit(f"Missing required tool in PATH: {tool}")
    toolchain = Toolchain(
        project_dir=project_dir,
        avmenc=avm_build / "avmenc",
        avmdec=avm_build / "avmdec",
        av2codec=project_dir / "target" / profile / "av2codec",
        av2codec_decode=project_dir / "target" / profile / "av2codec_decode",
    )
    if not toolchain.avmenc.exists():
        raise SystemExit(f"Missing avmenc at {toolchain.avmenc}")
    if not toolchain.avmdec.exists():
        raise SystemExit(f"Missing avmdec at {toolchain.avmdec}")
    return toolchain


def build_av2codec(toolchain: Toolchain, profile: str, log_path: Path) -> float:
    command = ["cargo", "build", "--quiet", "--bin", "av2codec", "--bin", "av2codec_decode"]
    if profile == "release":
        command.insert(2, "--release")
    started = time.perf_counter()
    run_command(command, cwd=toolchain.project_dir, log_path=log_path)
    return time.perf_counter() - started


def reference_signature(args: argparse.Namespace) -> str:
    hasher = hashlib.sha256()
    input_path = args.input.resolve()
    stat = input_path.stat()
    hasher.update(str(input_path).encode())
    hasher.update(str(stat.st_mtime_ns).encode())
    hasher.update(str(args.size).encode())
    hasher.update(str(args.quality).encode())
    hasher.update(str(args.cpu_used).encode())
    hasher.update(b"codec_iteration_v1")
    return hasher.hexdigest()


def ensure_reference(
    args: argparse.Namespace,
    toolchain: Toolchain,
) -> tuple[PreparedFrame, bool]:
    ref_dir = args.workdir / "reference"
    meta_path = ref_dir / "reference.json"
    required = [
        ref_dir / "input.y4m",
        ref_dir / "original.png",
        ref_dir / "avmenc.obu",
        ref_dir / "avmenc__avmdec.y4m",
        ref_dir / "avmenc__avmdec.png",
    ]
    signature = reference_signature(args)
    if not args.refresh_reference and meta_path.exists() and all(path.exists() for path in required):
        metadata = json.loads(meta_path.read_text())
        if metadata.get("signature") == signature:
            width, height = probe_dimensions(ref_dir / "input.y4m")
            prepared = PreparedFrame(
                input_y4m=ref_dir / "input.y4m",
                original_png=ref_dir / "original.png",
                width=width,
                height=height,
            )
            return prepared, True

    ref_dir.mkdir(parents=True, exist_ok=True)
    prepared = prepare_frame(args.input.resolve(), ref_dir, args.size)
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
            str(ref_dir / "avmenc.obu"),
            str(prepared.input_y4m),
        ],
        log_path=ref_dir / "avmenc_encode.log",
    )
    run_command(
        [
            str(toolchain.avmdec),
            str(ref_dir / "avmenc.obu"),
            "-o",
            str(ref_dir / "avmenc__avmdec.y4m"),
        ],
        log_path=ref_dir / "avmenc__avmdec.log",
    )
    convert_y4m_to_png(ref_dir / "avmenc__avmdec.y4m", ref_dir / "avmenc__avmdec.png")
    meta_path.write_text(
        json.dumps(
            {
                "signature": signature,
                "input": str(args.input.resolve()),
                "size": list(args.size) if args.size else None,
                "quality": args.quality,
                "cpu_used": args.cpu_used,
                "width": prepared.width,
                "height": prepared.height,
            },
            indent=2,
        )
    )
    return prepared, False


def run_once(args: argparse.Namespace, toolchain: Toolchain) -> None:
    started = time.perf_counter()
    reference_started = time.perf_counter()
    prepared, reused_reference = ensure_reference(args, toolchain)
    reference_elapsed = time.perf_counter() - reference_started

    build_elapsed = 0.0
    if not args.no_build:
        build_elapsed = build_av2codec(toolchain, args.profile, args.workdir / "build.log")

    current_dir = args.workdir / "current"
    current_dir.mkdir(parents=True, exist_ok=True)

    run_command(
        [
            str(toolchain.av2codec),
            str(prepared.input_y4m),
            str(current_dir / "av2codec.obu"),
            "--qindex",
            str(args.quality),
        ],
        log_path=current_dir / "av2codec_encode.log",
    )
    run_command(
        [
            str(toolchain.av2codec_decode),
            str(args.workdir / "reference" / "avmenc.obu"),
            "-o",
            str(current_dir / "avmenc__av2codec.y4m"),
        ],
        log_path=current_dir / "avmenc__av2codec.log",
    )
    run_command(
        [
            str(toolchain.avmdec),
            str(current_dir / "av2codec.obu"),
            "-o",
            str(current_dir / "av2codec__avmdec.y4m"),
        ],
        log_path=current_dir / "av2codec__avmdec.log",
    )
    run_command(
        [
            str(toolchain.av2codec_decode),
            str(current_dir / "av2codec.obu"),
            "-o",
            str(current_dir / "av2codec__av2codec.y4m"),
        ],
        log_path=current_dir / "av2codec__av2codec.log",
    )

    original = load_first_frame(args.workdir / "reference" / "input.y4m")
    avmenc_avmdec = load_first_frame(args.workdir / "reference" / "avmenc__avmdec.y4m")
    avmenc_av2codec = load_first_frame(current_dir / "avmenc__av2codec.y4m")
    av2codec_avmdec = load_first_frame(current_dir / "av2codec__avmdec.y4m")
    av2codec_av2codec = load_first_frame(current_dir / "av2codec__av2codec.y4m")

    metrics = {
        "original_vs_avmenc_avmdec": psnr_triplet(original, avmenc_avmdec),
        "avmenc_avmdec_vs_avmenc_av2codec": psnr_triplet(avmenc_avmdec, avmenc_av2codec),
        "original_vs_av2codec_avmdec": psnr_triplet(original, av2codec_avmdec),
        "original_vs_av2codec_av2codec": psnr_triplet(original, av2codec_av2codec),
        "av2codec_avmdec_vs_av2codec_av2codec": psnr_triplet(av2codec_avmdec, av2codec_av2codec),
    }

    if not args.no_matrix:
        convert_y4m_to_png(current_dir / "avmenc__av2codec.y4m", current_dir / "avmenc__av2codec.png")
        convert_y4m_to_png(current_dir / "av2codec__avmdec.y4m", current_dir / "av2codec__avmdec.png")
        convert_y4m_to_png(current_dir / "av2codec__av2codec.y4m", current_dir / "av2codec__av2codec.png")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        render_matrix(
            output_path=args.output.resolve(),
            images={
                "original": args.workdir / "reference" / "original.png",
                "avmenc_avmdec": args.workdir / "reference" / "avmenc__avmdec.png",
                "avmenc_av2codec": current_dir / "avmenc__av2codec.png",
                "av2codec_avmdec": current_dir / "av2codec__avmdec.png",
                "av2codec_av2codec": current_dir / "av2codec__av2codec.png",
            },
            size=(prepared.width, prepared.height),
            scale_up=args.scale_up,
            quality=args.quality,
            avmenc_size=(args.workdir / "reference" / "avmenc.obu").stat().st_size,
            av2codec_size=(current_dir / "av2codec.obu").stat().st_size,
            font_path=None,
        )

    previous = load_previous_report(args.report_json)
    summary = {
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "profile": args.profile,
        "quality": args.quality,
        "cpu_used": args.cpu_used,
        "input": str(args.input.resolve()),
        "size": [prepared.width, prepared.height],
        "reference_reused": reused_reference,
        "durations": {
            "reference_seconds": round(reference_elapsed, 3),
            "build_seconds": round(build_elapsed, 3),
            "total_seconds": round(time.perf_counter() - started, 3),
        },
        "artifacts": {
            "workdir": str(args.workdir.resolve()),
            "av2codec_obu": str((current_dir / "av2codec.obu").resolve()),
            "report_json": str(args.report_json.resolve()),
            "matrix_png": None if args.no_matrix else str(args.output.resolve()),
        },
        "bitstream_sizes": {
            "avmenc": (args.workdir / "reference" / "avmenc.obu").stat().st_size,
            "av2codec": (current_dir / "av2codec.obu").stat().st_size,
        },
        "metrics": metrics,
    }
    args.report_json.parent.mkdir(parents=True, exist_ok=True)
    args.report_json.write_text(json.dumps(summary, indent=2))
    print_summary(summary, previous)


def ensure_input_exists(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"Input file not found: {path}")


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
    av2codec_size: int,
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

    title_font = pick_font(font_path, 28 * scale_up, "av2codec / avmenc / avmdec matrix")
    subtitle_font = pick_font(
        font_path,
        14 * scale_up,
        f"{source_width}x{source_height}   q={quality}   avmenc={format_bytes(avmenc_size)}   av2codec={format_bytes(av2codec_size)}",
    )
    draw_text_centered(
        draw,
        (margin, margin, canvas_width - margin, margin + title_height),
        "av2codec / avmenc / avmdec matrix",
        title_font,
        title,
    )
    draw_text_centered(
        draw,
        (margin, margin + title_height, canvas_width - margin, margin + title_height + subtitle_height),
        f"{source_width}x{source_height}   q={quality}   avmenc={format_bytes(avmenc_size)}   av2codec={format_bytes(av2codec_size)}",
        subtitle_font,
        text,
    )

    labels = [
        [("Original", images["original"]), ("avmenc -> avmdec", images["avmenc_avmdec"]), ("avmenc -> av2codec", images["avmenc_av2codec"])],
        [("Original", images["original"]), ("av2codec -> avmdec", images["av2codec_avmdec"]), ("av2codec -> av2codec", images["av2codec_av2codec"])],
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


def load_previous_report(path: Path) -> dict | None:
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError:
        return None


def load_first_frame(path: Path) -> Y4mFrame:
    data = path.read_bytes()
    header_end = data.find(b"\n")
    if header_end < 0:
        raise SystemExit(f"Invalid Y4M header in {path}")
    header = data[:header_end].decode()
    width = 0
    height = 0
    for token in header.split():
        if token.startswith("W"):
            width = int(token[1:])
        elif token.startswith("H"):
            height = int(token[1:])
    if width <= 0 or height <= 0:
        raise SystemExit(f"Invalid Y4M dimensions in {path}")

    frame_start = header_end + 1
    frame_end = data.find(b"\n", frame_start)
    if frame_end < 0:
        raise SystemExit(f"Missing FRAME header in {path}")
    frame_header = data[frame_start:frame_end]
    if not frame_header.startswith(b"FRAME"):
        raise SystemExit(f"Invalid FRAME header in {path}")

    y_size = width * height
    uv_size = (width // 2) * (height // 2)
    payload_start = frame_end + 1
    payload_end = payload_start + y_size + uv_size * 2
    payload = data[payload_start:payload_end]
    if len(payload) != y_size + uv_size * 2:
        raise SystemExit(f"Unexpected frame payload size in {path}")

    y_end = y_size
    u_end = y_end + uv_size
    return Y4mFrame(
        width=width,
        height=height,
        y=payload[:y_end],
        u=payload[y_end:u_end],
        v=payload[u_end:],
    )


def psnr_triplet(left: Y4mFrame, right: Y4mFrame) -> dict[str, float]:
    if left.width != right.width or left.height != right.height:
        raise SystemExit("Frame dimensions do not match for PSNR computation")
    return {
        "y": compute_psnr(left.y, right.y),
        "u": compute_psnr(left.u, right.u),
        "v": compute_psnr(left.v, right.v),
    }


def compute_psnr(left: bytes, right: bytes) -> float:
    if len(left) != len(right):
        raise SystemExit("Plane lengths do not match for PSNR computation")
    mse = 0.0
    for a, b in zip(left, right):
        diff = float(a) - float(b)
        mse += diff * diff
    mse /= len(left)
    if mse == 0.0:
        return float("inf")
    return 10.0 * math.log10((255.0 * 255.0) / mse)


def format_psnr(value: float) -> str:
    if value == float("inf"):
        return "inf"
    return f"{value:.2f}"


def print_summary(summary: dict, previous: dict | None) -> None:
    sizes = summary["bitstream_sizes"]
    avmenc_size = sizes["avmenc"]
    av2codec_size = sizes["av2codec"]
    delta_ratio = 0.0 if avmenc_size == 0 else ((av2codec_size / avmenc_size) - 1.0) * 100.0
    print("")
    print("=== av2codec encoder iteration ===")
    print(f"Input:      {summary['input']}")
    print(f"Frame:      {summary['size'][0]}x{summary['size'][1]} q={summary['quality']} profile={summary['profile']}")
    print(f"Reference:  {'cached' if summary['reference_reused'] else 'rebuilt'}")
    print(
        f"Duration:   ref={summary['durations']['reference_seconds']:.2f}s build={summary['durations']['build_seconds']:.2f}s total={summary['durations']['total_seconds']:.2f}s"
    )
    print(
        f"Bitstream:  avmenc={format_bytes(avmenc_size)} av2codec={format_bytes(av2codec_size)} delta={delta_ratio:+.2f}%"
    )
    print("")
    print("PSNR")
    labels = [
        ("original_vs_avmenc_avmdec", "original vs avmenc->avmdec"),
        ("avmenc_avmdec_vs_avmenc_av2codec", "avmenc->avmdec vs avmenc->av2codec"),
        ("original_vs_av2codec_avmdec", "original vs av2codec->avmdec"),
        ("original_vs_av2codec_av2codec", "original vs av2codec->av2codec"),
        ("av2codec_avmdec_vs_av2codec_av2codec", "av2codec->avmdec vs av2codec->av2codec"),
    ]
    for key, label in labels:
        row = summary["metrics"][key]
        print(
            f"  {label:<34} Y={format_psnr(row['y'])} U={format_psnr(row['u'])} V={format_psnr(row['v'])}"
        )
    if previous is not None and "metrics" in previous:
        current = summary["metrics"]["original_vs_av2codec_avmdec"]["y"]
        old = previous["metrics"]["original_vs_av2codec_avmdec"]["y"]
        decoder_current = summary["metrics"]["avmenc_avmdec_vs_avmenc_av2codec"]["y"]
        decoder_old = previous["metrics"]["avmenc_avmdec_vs_avmenc_av2codec"]["y"]
        if current != float("inf") and old != float("inf"):
            print("")
            print(
                f"Delta:      original vs av2codec->avmdec Y {current - old:+.2f} dB"
            )
        if decoder_current != float("inf") and decoder_old != float("inf"):
            print(
                f"Delta:      avmenc->avmdec vs avmenc->av2codec Y {decoder_current - decoder_old:+.2f} dB"
            )
    print(f"Report:     {summary['artifacts']['report_json']}")
    if summary["artifacts"]["matrix_png"] is not None:
        print(f"Matrix:     {summary['artifacts']['matrix_png']}")
    print(f"Workdir:    {summary['artifacts']['workdir']}")


def watch_loop(args: argparse.Namespace, toolchain: Toolchain) -> None:
    snapshot = watch_snapshot(toolchain.project_dir)
    while True:
        try:
            run_once(args, toolchain)
        except KeyboardInterrupt:
            raise
        except (Exception, SystemExit) as exc:
            print(f"Iteration failed: {exc}")
        print("")
        print("Waiting for changes...")
        while True:
            time.sleep(args.watch_interval)
            current = watch_snapshot(toolchain.project_dir)
            if current != snapshot:
                snapshot = current
                print("")
                print(f"[{time.strftime('%H:%M:%S')}] change detected")
                break


def watch_snapshot(project_dir: Path) -> dict[str, int]:
    paths = [
        project_dir / "Cargo.toml",
        project_dir / "Cargo.lock",
        project_dir / "scripts" / "codec_iteration.py",
        project_dir / "scripts" / "compare_codec_matrix.py",
    ]
    rust_files = list((project_dir / "src").rglob("*.rs")) + list((project_dir / "tests").rglob("*.rs"))
    snapshot: dict[str, int] = {}
    for path in paths + rust_files:
        if path.exists():
            snapshot[str(path.resolve())] = path.stat().st_mtime_ns
    return snapshot


if __name__ == "__main__":
    raise SystemExit(main())
