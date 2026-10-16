#!/usr/bin/env bash
# 400 synthetic WebP covers (128 px) + 80 px thumbnails for the spike (needs ffmpeg).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p public/covers public/covers80
for i in $(seq 0 399); do printf "%03d\n" "$i"; done | xargs -P 8 -I{} ffmpeg -v error -y -f lavfi -i "testsrc2=size=128x128:rate=1:duration=1,hue=h={}*0.9" -frames:v 1 -c:v libwebp -quality 75 public/covers/{}.webp
for i in $(seq 0 399); do printf "%03d\n" "$i"; done | xargs -P 8 -I{} ffmpeg -v error -y -i public/covers/{}.webp -vf scale=80:80 -c:v libwebp -quality 75 public/covers80/{}.webp
