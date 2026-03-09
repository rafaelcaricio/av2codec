#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
AVM_BUILD="${PROJECT_DIR}/../avm_build"
AVMENC="${AVM_BUILD}/avmenc"
AVMDEC="${AVM_BUILD}/avmdec"
WAV2C_DECODE="${PROJECT_DIR}/target/release/av2codec_decode"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options] <input_image>
       $(basename "$0") [options] -e <encoded.obu> <original_image>

Encode an image with avmenc, decode with both avmdec and av2codec, and
produce a side-by-side comparison PNG.

Options:
  -q <qp>       Quantizer parameter for avmenc (default: 50)
  -s <WxH>      Resize input before encoding (e.g. 768x768, 256x256)
  -o <output>   Output comparison PNG path (default: /tmp/av2codec_comparison.png)
  -e <obu>      Use pre-encoded OBU file (skip avmenc, much faster)
  -k            Keep intermediate files in workdir
  -h            Show this help

Examples:
  $(basename "$0") photo.png
  $(basename "$0") -q 30 -s 768x768 photo.jpg
  $(basename "$0") -e encoded.obu photo.png
  $(basename "$0") -q 80 -o result.png -s 256x256 photo.png
EOF
    exit 0
}

QP=50
RESIZE=""
OUTPUT="/tmp/av2codec_comparison.png"
KEEP=false
PRE_ENCODED=""

while getopts "q:s:o:e:kh" opt; do
    case $opt in
        q) QP="$OPTARG" ;;
        s) RESIZE="$OPTARG" ;;
        o) OUTPUT="$OPTARG" ;;
        e) PRE_ENCODED="$OPTARG" ;;
        k) KEEP=true ;;
        h) usage ;;
        *) usage ;;
    esac
done
shift $((OPTIND - 1))

if [[ $# -lt 1 ]]; then
    echo "Error: input image required"
    usage
fi

INPUT="$1"
if [[ ! -f "$INPUT" ]]; then
    echo "Error: file not found: $INPUT"
    exit 1
fi

if [[ -n "$PRE_ENCODED" && ! -f "$PRE_ENCODED" ]]; then
    echo "Error: OBU file not found: $PRE_ENCODED"
    exit 1
fi

for tool in ffmpeg ffprobe; do
    if ! command -v "$tool" &>/dev/null; then
        echo "Error: $tool not found in PATH"
        exit 1
    fi
done

if [[ -z "$PRE_ENCODED" ]]; then
    if [[ ! -x "$AVMENC" ]]; then
        echo "Error: avmenc not found at $AVMENC"
        echo "Build it: cd ${AVM_BUILD} && cmake --build . --target avmenc -j\$(sysctl -n hw.ncpu)"
        exit 1
    fi
fi

if [[ ! -x "$AVMDEC" ]]; then
    echo "Error: avmdec not found at $AVMDEC"
    echo "Build it: cd ${AVM_BUILD} && cmake --build . --target avmdec -j\$(sysctl -n hw.ncpu)"
    exit 1
fi

if [[ ! -x "$WAV2C_DECODE" ]]; then
    echo "Building av2codec in release mode..."
    cargo build --release --manifest-path="${PROJECT_DIR}/Cargo.toml" --quiet
fi

WORKDIR=$(mktemp -d /tmp/av2codec_compare.XXXXXX)
cleanup() {
    if [[ "$KEEP" == false ]]; then
        rm -rf "$WORKDIR"
    else
        echo "Intermediate files in: $WORKDIR"
    fi
}
trap cleanup EXIT

echo "=== av2codec decoder comparison ==="
echo "Input:  $INPUT"

DIMS=$(ffprobe -v error -show_entries stream=width,height -of csv=p=0 "$INPUT" 2>/dev/null | head -1)
ORIG_W=$(echo "$DIMS" | cut -d, -f1)
ORIG_H=$(echo "$DIMS" | cut -d, -f2)
echo "Source: ${ORIG_W}x${ORIG_H}"

W="$ORIG_W"
H="$ORIG_H"
SCALE_FILTER=""
if [[ -n "$RESIZE" ]]; then
    W=$(echo "$RESIZE" | cut -dx -f1)
    H=$(echo "$RESIZE" | cut -dx -f2)
    SCALE_FILTER="-vf scale=${W}:${H}"
    echo "Resize: ${W}x${H}"
fi

if [[ -n "$PRE_ENCODED" ]]; then
    echo "OBU:    $PRE_ENCODED (pre-encoded)"
    cp "$PRE_ENCODED" "${WORKDIR}/encoded.obu"
else
    echo "QP:     $QP"
    echo ""
    echo "[1/5] Preparing Y4M input..."
    # shellcheck disable=SC2086
    ffmpeg -y -v error -i "$INPUT" $SCALE_FILTER -pix_fmt yuv420p -frames:v 1 "${WORKDIR}/input.y4m"

    echo "[2/5] Encoding with avmenc (qp=$QP)..."
    echo "       (this can take several minutes for large frames)"
    "$AVMENC" \
        --obu \
        --limit=1 \
        --qp="$QP" \
        --end-usage=q \
        --bit-depth=8 \
        --full-still-picture-hdr \
        -w "$W" -h "$H" \
        --quiet \
        -o "${WORKDIR}/encoded.obu" \
        "${WORKDIR}/input.y4m" 2>/dev/null
fi

BITSTREAM_SIZE=$(stat -f%z "${WORKDIR}/encoded.obu" 2>/dev/null || stat -c%s "${WORKDIR}/encoded.obu")
echo ""
echo "Bitstream: ${BITSTREAM_SIZE} bytes"

STEP=3
TOTAL=5
if [[ -n "$PRE_ENCODED" ]]; then
    STEP=1
    TOTAL=3
fi

echo "[${STEP}/${TOTAL}] Decoding with avmdec (reference)..."
"$AVMDEC" "${WORKDIR}/encoded.obu" -o "${WORKDIR}/avmdec.y4m" 2>/dev/null
((STEP++))

echo "[${STEP}/${TOTAL}] Decoding with av2codec..."
"$WAV2C_DECODE" "${WORKDIR}/encoded.obu" -o "${WORKDIR}/av2codec.y4m" 2>&1
((STEP++))

AVMDEC_DIMS=$(ffprobe -v error -show_entries stream=width,height -of csv=p=0 "${WORKDIR}/avmdec.y4m" 2>/dev/null | head -1)
FRAME_W=$(echo "$AVMDEC_DIMS" | cut -d, -f1)
FRAME_H=$(echo "$AVMDEC_DIMS" | cut -d, -f2)

echo "[${STEP}/${TOTAL}] Generating comparison image..."
ffmpeg -y -v error \
    -i "$INPUT" \
    -i "${WORKDIR}/avmdec.y4m" \
    -i "${WORKDIR}/av2codec.y4m" \
    -filter_complex "
        [0]scale=${FRAME_W}:${FRAME_H},pad=iw:ih+30:0:30:color=0x222222,
            drawtext=text='Original':x=(w-tw)/2:y=6:fontsize=20:fontcolor=white[orig];
        [1]pad=iw:ih+30:0:30:color=0x222222,
            drawtext=text='avmdec (ref)':x=(w-tw)/2:y=6:fontsize=20:fontcolor=white[ref];
        [2]pad=iw:ih+30:0:30:color=0x222222,
            drawtext=text='av2codec':x=(w-tw)/2:y=6:fontsize=20:fontcolor=white[av2codec];
        [orig][ref][av2codec]hstack=inputs=3
    " \
    "$OUTPUT"

echo ""
echo "Comparison saved to: $OUTPUT"
echo "Open with:  open $OUTPUT"
