#!/usr/bin/env bash
# Render the app icon from the `tunnel` patch, because the icon should be a
# frame the program actually produced rather than a drawing of one.
set -euo pipefail
OUT="${1:?usage: make-icon.sh <out.icns>}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

"$ROOT/target/release/otd" render "$ROOT/examples/tunnel.otd" \
    --node /out1 --frames 150 --out "$WORK/frames" >/dev/null

# Centre square crop of a 16:9 frame, then every size macOS asks for.
SET="$WORK/AppIcon.iconset"
mkdir -p "$SET"
ffmpeg -y -v error -i "$WORK/frames/00149.png" -vf "crop=720:720:280:0,scale=1024:1024" "$WORK/icon.png"
for size in 16 32 128 256 512; do
    ffmpeg -y -v error -i "$WORK/icon.png" -vf "scale=$size:$size" "$SET/icon_${size}x${size}.png"
    ffmpeg -y -v error -i "$WORK/icon.png" -vf "scale=$((size*2)):$((size*2))" "$SET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$SET" -o "$OUT"
