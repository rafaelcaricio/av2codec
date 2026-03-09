#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
AVM_BUILD="${PROJECT_DIR}/../avm_build"
AVMDEC="${AVM_BUILD}/avmdec"
AV2CODEC="${PROJECT_DIR}/target/release/av2codec"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options] <input_image>

Encode an image with av2codec and decode with the AVM reference decoder (avmdec),
producing a PNG of the decoded result.

Pipeline: image → ffmpeg → Y4M → av2codec → OBU → avmdec → Y4M → ffmpeg → PNG

Options:
  -q <qindex>   Quantizer index for av2codec, 0-255 (default: 50)
  -s <WxH>      Resize and center-crop input to WxH before encoding
  -o <output>   Output PNG path (default: /tmp/av2codec_roundtrip.png)
  -p <profile>  Cargo profile: debug or release (default: release)
  -k            Keep intermediate files in workdir
  -B            Skip cargo build, use existing binary
  -h            Show this help

Examples:
  $(basename "$0") photo.png
  $(basename "$0") -q 30 photo.png
  $(basename "$0") -q 30 -s 256x256 -o result.png photo.jpg
  $(basename "$0") -q 80 -s 64x64 -k photo.png
EOF
    exit 0
}

QINDEX=50
RESIZE=""
OUTPUT="/tmp/av2codec_roundtrip.png"
PROFILE="release"
KEEP=false
NO_BUILD=false

while getopts "q:s:o:p:kBh" opt; do
    case $opt in
        q) QINDEX="$OPTARG" ;;
        s) RESIZE="$OPTARG" ;;
        o) OUTPUT="$OPTARG" ;;
        p) PROFILE="$OPTARG" ;;
        k) KEEP=true ;;
        B) NO_BUILD=true ;;
        h) usage ;;
        *) usage ;;
    esac
done
shift $((OPTIND - 1))

if [[ $# -lt 1 ]]; then
    echo "Error: input image required" >&2
    usage
fi

INPUT="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
if [[ ! -f "$INPUT" ]]; then
    echo "Error: file not found: $INPUT" >&2
    exit 1
fi

for tool in ffmpeg ffprobe; do
    if ! command -v "$tool" &>/dev/null; then
        echo "Error: $tool not found in PATH" >&2
        exit 1
    fi
done

if [[ ! -x "$AVMDEC" ]]; then
    echo "Error: avmdec not found at $AVMDEC" >&2
    echo "Build it: cd ${AVM_BUILD} && cmake --build . --target avmdec -j\$(sysctl -n hw.ncpu)" >&2
    exit 1
fi

AV2CODEC="${PROJECT_DIR}/target/${PROFILE}/av2codec"

if [[ "$NO_BUILD" == true ]]; then
    if [[ ! -x "$AV2CODEC" ]]; then
        echo "Error: av2codec not found at $AV2CODEC (run without -B to build)" >&2
        exit 1
    fi
else
    BUILD_CMD="cargo build --bin av2codec --quiet"
    if [[ "$PROFILE" == "release" ]]; then
        BUILD_CMD+=" --release"
    fi
    echo "[build] ${BUILD_CMD}"
    (cd "$PROJECT_DIR" && $BUILD_CMD)
fi

WORKDIR=$(mktemp -d /tmp/av2codec_roundtrip.XXXXXX)
cleanup() {
    if [[ "$KEEP" == false ]]; then
        rm -rf "$WORKDIR"
    else
        echo "workdir: $WORKDIR"
    fi
}
trap cleanup EXIT

echo "=== av2codec roundtrip ==="
echo "input:   $INPUT"
echo "qindex:  $QINDEX"
echo "profile: $PROFILE"

if [[ -n "$RESIZE" ]]; then
    TARGET_W=$(echo "$RESIZE" | cut -dx -f1)
    TARGET_H=$(echo "$RESIZE" | cut -dx -f2)
    VF="scale=${TARGET_W}:${TARGET_H}:force_original_aspect_ratio=increase:flags=lanczos,crop=${TARGET_W}:${TARGET_H},format=yuv420p"
    echo "resize:  ${TARGET_W}x${TARGET_H}"
else
    VF="scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p"
fi

echo ""
echo "[1/4] Preparing Y4M input..."
ffmpeg -y -v error -i "$INPUT" -vf "$VF" -frames:v 1 "${WORKDIR}/input.y4m"

DIMS=$(ffprobe -v error -show_entries stream=width,height -of csv=p=0:s=x "${WORKDIR}/input.y4m" 2>/dev/null | head -1)
echo "       frame: ${DIMS}"

echo "[2/4] Encoding with av2codec (qindex=$QINDEX)..."
"$AV2CODEC" "${WORKDIR}/input.y4m" "${WORKDIR}/encoded.obu" --qindex "$QINDEX"

OBU_SIZE=$(stat -f%z "${WORKDIR}/encoded.obu" 2>/dev/null || stat -c%s "${WORKDIR}/encoded.obu")
echo "       bitstream: ${OBU_SIZE} bytes"

echo "[3/4] Decoding with avmdec..."
"$AVMDEC" "${WORKDIR}/encoded.obu" -o "${WORKDIR}/decoded.y4m" 2>/dev/null

echo "[4/4] Converting to PNG..."
mkdir -p "$(dirname "$OUTPUT")"
ffmpeg -y -v error -i "${WORKDIR}/decoded.y4m" -frames:v 1 "$OUTPUT"

echo ""
echo "done: $OUTPUT"
echo "open: open $OUTPUT"
