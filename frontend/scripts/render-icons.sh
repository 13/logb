#!/usr/bin/env bash
# Render every PNG the manifest needs from public/icon.svg.
#
# The PNGs are committed rather than built, so CI needs no rasteriser -- run this by hand
# whenever icon.svg changes, and commit what it produces. tests/pwa-icons.test.ts fails if a
# declared file is missing, but nothing can tell you a PNG is STALE, so re-run it every time.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v rsvg-convert >/dev/null || { echo "needs rsvg-convert (librsvg)"; exit 1; }

svg=public/icon.svg
rsvg-convert -w 192 -h 192 "$svg" -o public/pwa-192.png
rsvg-convert -w 512 -h 512 "$svg" -o public/pwa-512.png

# The maskable variant: same bee, full-bleed plate, inset to the ~80% safe zone a launcher
# mask leaves visible. Built by slicing the bee out of icon.svg between its markers, so there
# is exactly one drawing in the repository.
bee=$(sed -n '/<!--bee-->/,/<!--\/bee-->/p' "$svg")
tmp=$(mktemp --suffix=.svg)
trap 'rm -f "$tmp"' EXIT
cat > "$tmp" <<EOF
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <rect width="64" height="64" fill="#1f6f5f"/>
  <g transform="translate(6.4 6.4) scale(.8)">
$bee
  </g>
</svg>
EOF
rsvg-convert -w 512 -h 512 "$tmp" -o public/pwa-512-maskable.png

echo "rendered: pwa-192.png pwa-512.png pwa-512-maskable.png"
