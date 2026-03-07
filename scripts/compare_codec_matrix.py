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
import tempfile
import time
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
    profile: str


@dataclass
class PreparedFrame:
    input_y4m: Path
    original_png: Path
    width: int
    height: int


@dataclass
class WorkArea:
    root: Path
    keep: bool
    temp_dir: tempfile.TemporaryDirectory[str] | None


@dataclass
class ReferenceArtifacts:
    input_y4m: Path
    original_png: Path
    avmenc_obu: Path
    avmenc_avmdec_y4m: Path
    avmenc_avmdec_png: Path
    width: int
    height: int
    cache_hit: bool


@dataclass
class FrameData:
    width: int
    height: int
    y: bytes
    u: bytes
    v: bytes


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Encode one prepared frame with avmenc and wav2c, decode both bitstreams with avmdec and wav2c_decode, render a labeled matrix, and emit numeric metrics."
    )
    parser.add_argument("input", type=Path, help="Input image, video, or Y4M path")
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
        "--profile",
        choices=("debug", "release"),
        default="debug",
        help="Cargo profile for wav2c binaries (default: debug)",
    )
    parser.add_argument(
        "--workdir",
        type=Path,
        help="Fixed work directory for intermediates",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        help="Persistent cache directory for reference and latest artifacts",
    )
    parser.add_argument(
        "--metrics-json",
        type=Path,
        help="Metrics JSON path (default: <output>.json)",
    )
    parser.add_argument(
        "--metrics-text",
        type=Path,
        help="Metrics text summary path (default: <output>.txt)",
    )
    parser.add_argument(
        "--refresh-reference",
        action="store_true",
        help="Refresh the prepared frame and avmenc reference artifacts even if cached",
    )
    parser.add_argument(
        "--watch",
        action="store_true",
        help="Watch source paths and rerun the loop after edits",
    )
    parser.add_argument(
        "--watch-path",
        action="append",
        type=Path,
        help="Extra file or directory to watch; can be repeated",
    )
    parser.add_argument(
        "--poll-seconds",
        type=float,
        default=1.0,
        help="Watch polling interval in seconds (default: 1.0)",
    )
    parser.add_argument(
        "--keep-workdir",
        action="store_true",
        help="Keep intermediate files after completion",
    )
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="Skip cargo build and use existing binaries from the selected profile",
    )
    args = parser.parse_args()
    if args.scale_up < 1:
        parser.error("--scale-up must be >= 1")
    if args.quality < 0:
        parser.error("--quality must be >= 0")
    if args.cpu_used < 0:
        parser.error("--cpu-used must be >= 0")
    if args.poll_seconds <= 0:
        parser.error("--poll-seconds must be > 0")
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
    input_path = args.input.resolve()
    ensure_input_exists(input_path)
    toolchain = resolve_toolchain(args.profile)
    work_area = prepare_work_area(args, input_path)
    output_path = args.output.resolve()
    metrics_json_path = resolve_metrics_path(output_path, args.metrics_json, ".json")
    metrics_text_path = resolve_metrics_path(output_path, args.metrics_text, ".txt")

    try:
        if args.watch:
            return run_watch_loop(
                args=args,
                toolchain=toolchain,
                input_path=input_path,
                work_area=work_area,
                output_path=output_path,
                metrics_json_path=metrics_json_path,
                metrics_text_path=metrics_text_path,
            )
        run_iteration(
            args=args,
            toolchain=toolchain,
            input_path=input_path,
            work_area=work_area,
            output_path=output_path,
            metrics_json_path=metrics_json_path,
            metrics_text_path=metrics_text_path,
            run_index=1,
        )
        return 0
    finally:
        if work_area.temp_dir is not None and not work_area.keep:
            work_area.temp_dir.cleanup()


def run_watch_loop(
    *,
    args: argparse.Namespace,
    toolchain: Toolchain,
    input_path: Path,
    work_area: WorkArea,
    output_path: Path,
    metrics_json_path: Path,
    metrics_text_path: Path,
) -> int:
    watch_paths = resolve_watch_paths(args, toolchain.project_dir, input_path)
    snapshot = snapshot_paths(watch_paths)
    run_index = 1
    print(f"[watch] watching {len(watch_paths)} path(s)")
    print(f"[watch] workdir: {work_area.root}")

    while True:
        try:
            run_iteration(
                args=args,
                toolchain=toolchain,
                input_path=input_path,
                work_area=work_area,
                output_path=output_path,
                metrics_json_path=metrics_json_path,
                metrics_text_path=metrics_text_path,
                run_index=run_index,
            )
        except KeyboardInterrupt:
            print("\n[watch] stopped")
            return 130
        except SystemExit as exc:
            code = exc.code if isinstance(exc.code, int) else 1
            print(f"[watch] run {run_index} failed with exit code {code}")
        except Exception as exc:
            print(f"[watch] run {run_index} failed: {exc}", file=sys.stderr)
        run_index += 1

        print(f"[watch] waiting for changes (poll={args.poll_seconds:.1f}s)")
        try:
            snapshot, changed = wait_for_changes(watch_paths, snapshot, args.poll_seconds)
        except KeyboardInterrupt:
            print("\n[watch] stopped")
            return 130
        print(f"[watch] change detected: {', '.join(changed[:6])}")


def run_iteration(
    *,
    args: argparse.Namespace,
    toolchain: Toolchain,
    input_path: Path,
    work_area: WorkArea,
    output_path: Path,
    metrics_json_path: Path,
    metrics_text_path: Path,
    run_index: int,
) -> None:
    print("")
    print(f"=== run {run_index} ===")
    reference = ensure_reference(args, toolchain, input_path, work_area.root)

    if args.no_build:
        ensure_binaries_exist(toolchain)
        print(f"[build] skipped ({toolchain.profile})")
    else:
        build_wav2c(toolchain.project_dir, toolchain.profile)

    latest_dir = work_area.root / "latest"
    latest_dir.mkdir(parents=True, exist_ok=True)

    wav2c_obu = latest_dir / "wav2c.obu"
    avmenc_wav2c_y4m = latest_dir / "avmenc__wav2c.y4m"
    wav2c_avmdec_y4m = latest_dir / "wav2c__avmdec.y4m"
    wav2c_wav2c_y4m = latest_dir / "wav2c__wav2c.y4m"
    avmenc_wav2c_png = latest_dir / "avmenc__wav2c.png"
    wav2c_avmdec_png = latest_dir / "wav2c__avmdec.png"
    wav2c_wav2c_png = latest_dir / "wav2c__wav2c.png"

    print("[encode] wav2c")
    run_command(
        [
            str(toolchain.wav2c),
            str(reference.input_y4m),
            str(wav2c_obu),
            "--qindex",
            str(args.quality),
        ],
        log_path=latest_dir / "wav2c_encode.log",
    )

    decode_jobs = [
        (
            "avmenc -> wav2c",
            [str(toolchain.wav2c_decode), str(reference.avmenc_obu), "-o", str(avmenc_wav2c_y4m)],
            latest_dir / "avmenc__wav2c.log",
        ),
        (
            "wav2c -> avmdec",
            [str(toolchain.avmdec), str(wav2c_obu), "-o", str(wav2c_avmdec_y4m)],
            latest_dir / "wav2c__avmdec.log",
        ),
        (
            "wav2c -> wav2c",
            [str(toolchain.wav2c_decode), str(wav2c_obu), "-o", str(wav2c_wav2c_y4m)],
            latest_dir / "wav2c__wav2c.log",
        ),
    ]
    print("[decode] running 3 decode checks")
    for label, command, log_path in decode_jobs:
        print(f"         {label}")
        run_command(command, log_path=log_path)

    print("[render] converting outputs to png")
    convert_y4m_to_png(avmenc_wav2c_y4m, avmenc_wav2c_png)
    convert_y4m_to_png(wav2c_avmdec_y4m, wav2c_avmdec_png)
    convert_y4m_to_png(wav2c_wav2c_y4m, wav2c_wav2c_png)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    metrics_json_path.parent.mkdir(parents=True, exist_ok=True)
    metrics_text_path.parent.mkdir(parents=True, exist_ok=True)

    avmenc_size = reference.avmenc_obu.stat().st_size
    wav2c_size = wav2c_obu.stat().st_size
    report = build_metrics_report(
        args=args,
        input_path=input_path,
        output_path=output_path,
        metrics_json_path=metrics_json_path,
        metrics_text_path=metrics_text_path,
        workdir=work_area.root,
        reference=reference,
        avmenc_wav2c_y4m=avmenc_wav2c_y4m,
        wav2c_avmdec_y4m=wav2c_avmdec_y4m,
        wav2c_wav2c_y4m=wav2c_wav2c_y4m,
        avmenc_size=avmenc_size,
        wav2c_size=wav2c_size,
        run_index=run_index,
    )
    metrics_json_path.write_text(json.dumps(report, indent=2) + "\n")
    summary_text = render_metrics_summary(report)
    metrics_text_path.write_text(summary_text)

    print("[render] writing matrix")
    render_matrix(
        output_path=output_path,
        images={
            "original": reference.original_png,
            "avmenc_avmdec": reference.avmenc_avmdec_png,
            "avmenc_wav2c": avmenc_wav2c_png,
            "wav2c_avmdec": wav2c_avmdec_png,
            "wav2c_wav2c": wav2c_wav2c_png,
        },
        size=(reference.width, reference.height),
        scale_up=args.scale_up,
        quality=args.quality,
        avmenc_size=avmenc_size,
        wav2c_size=wav2c_size,
        font_path=args.font,
        profile=toolchain.profile,
    )

    print(summary_text, end="" if summary_text.endswith("\n") else "\n")
    print(f"matrix: {output_path}")
    print(f"summary: {metrics_text_path}")
    print(f"metrics: {metrics_json_path}")
    print(f"workdir: {work_area.root}")


def prepare_work_area(args: argparse.Namespace, input_path: Path) -> WorkArea:
    if args.workdir is not None:
        root = args.workdir.resolve()
        root.mkdir(parents=True, exist_ok=True)
        return WorkArea(root=root, keep=True, temp_dir=None)
    if args.cache_dir is not None:
        root = args.cache_dir.resolve()
        root.mkdir(parents=True, exist_ok=True)
        return WorkArea(root=root, keep=True, temp_dir=None)
    if args.watch:
        key = cache_key(
            input_path=input_path,
            size=args.size,
            quality=args.quality,
            cpu_used=args.cpu_used,
        )
        slug = slugify(input_path.stem) or "input"
        root = Path("/tmp/wav2c_codec_matrix_cache") / f"{slug}-{key[:12]}"
        root.mkdir(parents=True, exist_ok=True)
        return WorkArea(root=root, keep=True, temp_dir=None)
    temp_dir = tempfile.TemporaryDirectory(prefix="wav2c_codec_matrix.", dir="/tmp")
    return WorkArea(root=Path(temp_dir.name), keep=args.keep_workdir, temp_dir=temp_dir)


def cache_key(
    *,
    input_path: Path,
    size: tuple[int, int] | None,
    quality: int,
    cpu_used: int,
) -> str:
    stat = input_path.stat()
    payload = {
        "input": str(input_path),
        "size": list(size) if size is not None else None,
        "quality": quality,
        "cpu_used": cpu_used,
        "input_mtime_ns": stat.st_mtime_ns,
        "input_size": stat.st_size,
    }
    return hashlib.blake2b(
        json.dumps(payload, sort_keys=True).encode("utf-8"),
        digest_size=10,
    ).hexdigest()


def slugify(text: str) -> str:
    cleaned = "".join(char.lower() if char.isalnum() else "-" for char in text)
    while "--" in cleaned:
        cleaned = cleaned.replace("--", "-")
    return cleaned.strip("-")


def resolve_metrics_path(output_path: Path, override: Path | None, suffix: str) -> Path:
    if override is not None:
        return override.resolve()
    return output_path.with_suffix(suffix)


def resolve_watch_paths(
    args: argparse.Namespace,
    project_dir: Path,
    input_path: Path,
) -> list[Path]:
    if args.watch_path:
        paths = [resolve_watch_path(path, project_dir) for path in args.watch_path]
    else:
        defaults = [
            project_dir / "src",
            project_dir / "tests",
            project_dir / "scripts",
            project_dir / "Cargo.toml",
            project_dir / "Cargo.lock",
            project_dir / "README.md",
            input_path,
        ]
        paths = [path.resolve() for path in defaults]
    unique: list[Path] = []
    seen: set[Path] = set()
    for path in paths:
        if path in seen:
            continue
        seen.add(path)
        unique.append(path)
    return unique


def resolve_watch_path(path: Path, project_dir: Path) -> Path:
    if path.is_absolute():
        return path.resolve()
    return (project_dir / path).resolve()


def wait_for_changes(
    watch_paths: list[Path],
    previous_snapshot: dict[str, tuple[int, int]],
    poll_seconds: float,
) -> tuple[dict[str, tuple[int, int]], list[str]]:
    while True:
        time.sleep(poll_seconds)
        current_snapshot = snapshot_paths(watch_paths)
        if current_snapshot != previous_snapshot:
            return current_snapshot, describe_changes(previous_snapshot, current_snapshot)


def snapshot_paths(paths: list[Path]) -> dict[str, tuple[int, int]]:
    snapshot: dict[str, tuple[int, int]] = {}
    for path in paths:
        if not path.exists():
            snapshot[f"{path}::missing"] = (0, 0)
            continue
        if path.is_file():
            if should_ignore_path(path):
                continue
            stat = path.stat()
            snapshot[str(path)] = (stat.st_mtime_ns, stat.st_size)
            continue
        for file_path in sorted(path.rglob("*")):
            if not file_path.is_file() or should_ignore_path(file_path):
                continue
            stat = file_path.stat()
            snapshot[str(file_path)] = (stat.st_mtime_ns, stat.st_size)
    return snapshot


def should_ignore_path(path: Path) -> bool:
    parts = path.parts
    if any(part in {".git", "target", "__pycache__"} for part in parts):
        return True
    return path.name == ".DS_Store"


def describe_changes(
    previous_snapshot: dict[str, tuple[int, int]],
    current_snapshot: dict[str, tuple[int, int]],
) -> list[str]:
    changes: list[str] = []
    all_keys = sorted(set(previous_snapshot) | set(current_snapshot))
    for key in all_keys:
        if previous_snapshot.get(key) == current_snapshot.get(key):
            continue
        changes.append(key)
    return changes


def resolve_toolchain(profile: str) -> Toolchain:
    script_path = Path(__file__).resolve()
    project_dir = script_path.parent.parent
    avm_build = project_dir.parent / "avm_build"
    binary_dir = project_dir / "target" / profile
    toolchain = Toolchain(
        project_dir=project_dir,
        avmenc=avm_build / "avmenc",
        avmdec=avm_build / "avmdec",
        wav2c=binary_dir / "wav2c",
        wav2c_decode=binary_dir / "wav2c_decode",
        profile=profile,
    )
    for tool in ("ffmpeg", "ffprobe", "cargo"):
        if shutil.which(tool) is None:
            raise SystemExit(f"Missing required tool in PATH: {tool}")
    if not toolchain.avmenc.exists():
        raise SystemExit(f"Missing avmenc at {toolchain.avmenc}")
    if not toolchain.avmdec.exists():
        raise SystemExit(f"Missing avmdec at {toolchain.avmdec}")
    return toolchain


def ensure_binaries_exist(toolchain: Toolchain) -> None:
    missing = [path for path in (toolchain.wav2c, toolchain.wav2c_decode) if not path.exists()]
    if missing:
        joined = ", ".join(str(path) for path in missing)
        raise SystemExit(
            f"Missing wav2c binaries for profile {toolchain.profile}: {joined}. Run without --no-build first."
        )


def ensure_input_exists(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"Input file not found: {path}")


def build_wav2c(project_dir: Path, profile: str) -> None:
    command = ["cargo", "build"]
    if profile == "release":
        command.append("--release")
    command.extend(["--bin", "wav2c", "--bin", "wav2c_decode", "--quiet"])
    print(f"[build] {shlex.join(command)}")
    run_command(
        command,
        cwd=project_dir,
        log_path=project_dir / "target" / f"compare_codec_matrix_build_{profile}.log",
    )


def ensure_reference(
    args: argparse.Namespace,
    toolchain: Toolchain,
    input_path: Path,
    workdir: Path,
) -> ReferenceArtifacts:
    reference_dir = workdir / "reference"
    reference_dir.mkdir(parents=True, exist_ok=True)

    input_y4m = reference_dir / "input.y4m"
    original_png = reference_dir / "original.png"
    avmenc_obu = reference_dir / "avmenc.obu"
    avmenc_avmdec_y4m = reference_dir / "avmenc__avmdec.y4m"
    avmenc_avmdec_png = reference_dir / "avmenc__avmdec.png"
    metadata_path = reference_dir / "metadata.json"

    metadata = reference_metadata(args, input_path)
    required = [
        input_y4m,
        original_png,
        avmenc_obu,
        avmenc_avmdec_y4m,
        avmenc_avmdec_png,
        metadata_path,
    ]
    cache_hit = reference_current(metadata_path, metadata, required) and not args.refresh_reference

    if cache_hit:
        print("[ref] cache hit")
    else:
        print("[ref] refreshing reference artifacts")
        prepared = prepare_frame(input_path, reference_dir, args.size)
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
            log_path=reference_dir / "avmenc_encode.log",
        )
        run_command(
            [
                str(toolchain.avmdec),
                str(avmenc_obu),
                "-o",
                str(avmenc_avmdec_y4m),
            ],
            log_path=reference_dir / "avmenc__avmdec.log",
        )
        convert_y4m_to_png(avmenc_avmdec_y4m, avmenc_avmdec_png)
        metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")

    width, height = probe_dimensions(input_y4m)
    return ReferenceArtifacts(
        input_y4m=input_y4m,
        original_png=original_png,
        avmenc_obu=avmenc_obu,
        avmenc_avmdec_y4m=avmenc_avmdec_y4m,
        avmenc_avmdec_png=avmenc_avmdec_png,
        width=width,
        height=height,
        cache_hit=cache_hit,
    )


def reference_metadata(args: argparse.Namespace, input_path: Path) -> dict[str, object]:
    stat = input_path.stat()
    return {
        "input": str(input_path),
        "input_size": stat.st_size,
        "input_mtime_ns": stat.st_mtime_ns,
        "size": list(args.size) if args.size is not None else None,
        "quality": args.quality,
        "cpu_used": args.cpu_used,
    }


def reference_current(
    metadata_path: Path,
    expected_metadata: dict[str, object],
    required_paths: list[Path],
) -> bool:
    if not metadata_path.exists():
        return False
    if not all(path.exists() for path in required_paths):
        return False
    try:
        actual = json.loads(metadata_path.read_text())
    except json.JSONDecodeError:
        return False
    return actual == expected_metadata


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


def build_metrics_report(
    *,
    args: argparse.Namespace,
    input_path: Path,
    output_path: Path,
    metrics_json_path: Path,
    metrics_text_path: Path,
    workdir: Path,
    reference: ReferenceArtifacts,
    avmenc_wav2c_y4m: Path,
    wav2c_avmdec_y4m: Path,
    wav2c_wav2c_y4m: Path,
    avmenc_size: int,
    wav2c_size: int,
    run_index: int,
) -> dict[str, object]:
    original = read_y4m_frame(reference.input_y4m)
    avmenc_avmdec = read_y4m_frame(reference.avmenc_avmdec_y4m)
    avmenc_wav2c = read_y4m_frame(avmenc_wav2c_y4m)
    wav2c_avmdec = read_y4m_frame(wav2c_avmdec_y4m)
    wav2c_wav2c = read_y4m_frame(wav2c_wav2c_y4m)

    pairs = [
        compare_frames("orig vs avmenc -> avmdec", original, avmenc_avmdec),
        compare_frames("orig vs avmenc -> wav2c", original, avmenc_wav2c),
        compare_frames("orig vs wav2c -> avmdec", original, wav2c_avmdec),
        compare_frames("orig vs wav2c -> wav2c", original, wav2c_wav2c),
        compare_frames("avmenc ref vs avmenc -> wav2c", avmenc_avmdec, avmenc_wav2c),
        compare_frames("wav2c -> avmdec vs wav2c -> wav2c", wav2c_avmdec, wav2c_wav2c),
    ]
    focus = rank_focus_pairs(pairs)
    delta_bytes = wav2c_size - avmenc_size
    delta_percent = (delta_bytes / avmenc_size * 100.0) if avmenc_size else None

    return {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "run_index": run_index,
        "profile": args.profile,
        "quality": args.quality,
        "cpu_used": args.cpu_used,
        "input": str(input_path),
        "frame": {
            "width": reference.width,
            "height": reference.height,
        },
        "paths": {
            "output_png": str(output_path),
            "metrics_json": str(metrics_json_path),
            "metrics_text": str(metrics_text_path),
            "workdir": str(workdir),
        },
        "reference": {
            "cache_hit": reference.cache_hit,
            "input_y4m": str(reference.input_y4m),
            "avmenc_obu": str(reference.avmenc_obu),
            "avmenc_avmdec_y4m": str(reference.avmenc_avmdec_y4m),
        },
        "bitstreams": {
            "avmenc_bytes": avmenc_size,
            "wav2c_bytes": wav2c_size,
            "delta_bytes": delta_bytes,
            "delta_percent": delta_percent,
        },
        "pairs": pairs,
        "focus": focus,
    }


def read_y4m_frame(path: Path) -> FrameData:
    data = path.read_bytes()
    header_end = data.find(b"\n")
    if header_end < 0 or not data.startswith(b"YUV4MPEG2"):
        raise SystemExit(f"Invalid Y4M header in {path}")
    header = data[:header_end].decode("ascii", errors="replace")
    width, height = parse_y4m_dimensions(header, path)

    pos = header_end + 1
    if not data[pos:].startswith(b"FRAME"):
        raise SystemExit(f"Missing FRAME marker in {path}")
    frame_end = data.find(b"\n", pos)
    if frame_end < 0:
        raise SystemExit(f"Invalid FRAME header in {path}")
    pos = frame_end + 1

    y_size = width * height
    uv_size = (width // 2) * (height // 2)
    frame_size = y_size + uv_size * 2
    frame = data[pos:pos + frame_size]
    if len(frame) != frame_size:
        raise SystemExit(f"Truncated Y4M frame in {path}")

    y = frame[:y_size]
    u = frame[y_size:y_size + uv_size]
    v = frame[y_size + uv_size:]
    return FrameData(width=width, height=height, y=y, u=u, v=v)


def parse_y4m_dimensions(header: str, path: Path) -> tuple[int, int]:
    width: int | None = None
    height: int | None = None
    for token in header.split():
        if token.startswith("W"):
            width = int(token[1:])
        elif token.startswith("H"):
            height = int(token[1:])
    if width is None or height is None:
        raise SystemExit(f"Missing width/height in Y4M header: {path}")
    return width, height


def compare_frames(label: str, left: FrameData, right: FrameData) -> dict[str, object]:
    if left.width != right.width or left.height != right.height:
        raise SystemExit(f"Frame size mismatch for {label}: {left.width}x{left.height} vs {right.width}x{right.height}")

    y_acc = accumulate_diff(left.y, right.y)
    u_acc = accumulate_diff(left.u, right.u)
    v_acc = accumulate_diff(left.v, right.v)
    all_acc = (
        y_acc[0] + u_acc[0] + v_acc[0],
        y_acc[1] + u_acc[1] + v_acc[1],
        y_acc[2] + u_acc[2] + v_acc[2],
        max(y_acc[3], u_acc[3], v_acc[3]),
    )

    return {
        "label": label,
        "y": stats_from_accumulator(y_acc),
        "u": stats_from_accumulator(u_acc),
        "v": stats_from_accumulator(v_acc),
        "all": stats_from_accumulator(all_acc),
    }


def accumulate_diff(left: bytes, right: bytes) -> tuple[int, int, int, int]:
    if len(left) != len(right):
        raise SystemExit(f"Plane size mismatch: {len(left)} vs {len(right)}")
    total_abs = 0
    total_sq = 0
    max_abs = 0
    for lval, rval in zip(left, right):
        diff = lval - rval
        abs_diff = diff if diff >= 0 else -diff
        total_abs += abs_diff
        total_sq += diff * diff
        if abs_diff > max_abs:
            max_abs = abs_diff
    return len(left), total_abs, total_sq, max_abs


def stats_from_accumulator(accumulator: tuple[int, int, int, int]) -> dict[str, object]:
    count, total_abs, total_sq, max_abs = accumulator
    mse = total_sq / count if count else 0.0
    lossless = total_sq == 0
    psnr_db = None if lossless else 10.0 * math.log10((255.0 * 255.0) / mse)
    return {
        "samples": count,
        "mae": total_abs / count if count else 0.0,
        "mse": mse,
        "max_abs": max_abs,
        "lossless": lossless,
        "psnr_db": psnr_db,
    }


def rank_focus_pairs(pairs: list[dict[str, object]]) -> list[dict[str, object]]:
    ranked = sorted(
        pairs,
        key=lambda pair: focus_sort_key(pair["y"]),
    )
    return [
        {
            "label": pair["label"],
            "y_psnr_db": pair["y"]["psnr_db"],
            "y_lossless": pair["y"]["lossless"],
            "y_mae": pair["y"]["mae"],
            "y_max_abs": pair["y"]["max_abs"],
        }
        for pair in ranked[:3]
    ]


def focus_sort_key(stats: dict[str, object]) -> tuple[float, float, int]:
    if stats["lossless"]:
        return (math.inf, -stats["mae"], -stats["max_abs"])
    return (stats["psnr_db"], -stats["mae"], -stats["max_abs"])


def render_metrics_summary(report: dict[str, object]) -> str:
    frame = report["frame"]
    bitstreams = report["bitstreams"]
    lines = [
        f"input: {report['input']}",
        f"frame: {frame['width']}x{frame['height']} q={report['quality']} profile={report['profile']} cpu-used={report['cpu_used']}",
        f"reference cache: {'hit' if report['reference']['cache_hit'] else 'refreshed'}",
        f"bitstreams: avmenc={format_bytes(bitstreams['avmenc_bytes'])} wav2c={format_bytes(bitstreams['wav2c_bytes'])} delta={format_signed_bytes(bitstreams['delta_bytes'])}{format_percent(bitstreams['delta_percent'])}",
        "",
        "pairs:",
    ]
    for pair in report["pairs"]:
        lines.append(
            "  "
            + pair["label"]
            + " | "
            + plane_summary("Y", pair["y"])
            + " | "
            + plane_summary("U", pair["u"])
            + " | "
            + plane_summary("V", pair["v"])
        )
    lines.append("")
    lines.append("focus:")
    for index, focus in enumerate(report["focus"], start=1):
        lines.append(
            f"  {index}. {focus['label']} | Y psnr={format_psnr(focus['y_psnr_db'], focus['y_lossless'])} mae={focus['y_mae']:.3f} max_abs={focus['y_max_abs']}"
        )
    lines.append("")
    return "\n".join(lines)


def plane_summary(name: str, stats: dict[str, object]) -> str:
    return f"{name} psnr={format_psnr(stats['psnr_db'], stats['lossless'])} mae={stats['mae']:.3f} max={stats['max_abs']}"


def format_psnr(psnr_db: float | None, lossless: bool) -> str:
    if lossless:
        return "inf"
    if psnr_db is None:
        return "n/a"
    return f"{psnr_db:.2f}dB"


def format_signed_bytes(value: int) -> str:
    if value == 0:
        return "0 B"
    sign = "+" if value > 0 else "-"
    absolute = abs(value)
    return f"{sign}{format_bytes(absolute)}"


def format_percent(value: float | None) -> str:
    if value is None:
        return ""
    sign = "+" if value > 0 else ""
    return f" ({sign}{value:.1f}%)"


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
    profile: str,
) -> None:
    source_width, source_height = size
    scaled_width = source_width * scale_up
    scaled_height = source_height * scale_up
    title_height = max(70, 34 * scale_up)
    subtitle_height = max(32, 16 * scale_up)
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
    subtitle_text = (
        f"{source_width}x{source_height}   q={quality}   {profile}   "
        f"avmenc={format_bytes(avmenc_size)}   wav2c={format_bytes(wav2c_size)}"
    )
    subtitle_font = best_fit_font(
        font_path,
        subtitle_text,
        canvas_width - margin * 2 - 10,
        14 * scale_up,
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
        subtitle_text,
        subtitle_font,
        text,
    )

    labels = [
        [
            ("Original", images["original"]),
            ("avmenc -> avmdec", images["avmenc_avmdec"]),
            ("avmenc -> wav2c", images["avmenc_wav2c"]),
        ],
        [
            ("Original", images["original"]),
            ("wav2c -> avmdec", images["wav2c_avmdec"]),
            ("wav2c -> wav2c", images["wav2c_wav2c"]),
        ],
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
