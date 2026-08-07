#!/usr/bin/env bash
# Render the clip the `video` example plays.
#
# It is a recording of the `plasma` patch, made by OpenTouchDesigner. The
# alternative was ffmpeg's test card, which is the right thing to verify a
# decoder against and the wrong thing to put in front of somebody: nobody
# opens a visual tool to look at colour bars and a timecode.
#
# Making it from a shipped example also means the file is reproducible from a
# clean checkout, owes nobody a licence, and demonstrates the round trip —
# `otd render` out, Movie File In back.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/examples/media/plasma.mp4"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cargo build --release -p otd-cli
# 8 seconds at 30fps. Small enough to live in a git repo, long enough that the
# loop is not obvious.
"$ROOT/target/release/otd" render "$ROOT/examples/plasma.otd" \
    --node /out1 --frames 480 --fps 60 --out "$WORK/frames" >/dev/null

mkdir -p "$(dirname "$OUT")"
ffmpeg -y -v error -framerate 60 -i "$WORK/frames/%05d.png" \
    -vf "fps=30,scale=640:360:flags=lanczos" \
    -c:v libx264 -preset slow -crf 30 -pix_fmt yuv420p -movflags +faststart \
    "$OUT"
ls -lh "$OUT"
