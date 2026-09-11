#!/usr/bin/env bash
# Render every PNG the manifest needs from public/icon.svg.
#
# The PNGs are committed rather than built, so CI needs no rasteriser -- run this by hand
# whenever icon.svg changes, and commit what it produces. It also stamps icon.svg's hash into
# scripts/icon.sha256, which tests/pwa-icons.test.ts compares against the current icon.svg --
# that stamp mismatch, not the PNGs themselves, is what catches a forgotten re-render.
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
# An empty slice means the markers are gone -- without this guard, sed silently matches
# nothing, $bee is empty, and this would render a plain green plate and exit 0.
[ -n "$bee" ] || { echo "no <!--bee--> ... <!--/bee--> markers found in $svg -- can't slice the maskable icon" >&2; exit 1; }
tmp=$(mktemp --suffix=.svg)
trap 'rm -f "$tmp"' EXIT
{
  # Quoted heredoc delimiter: the static wrapper is emitted verbatim, and $bee is written
  # separately with printf so any $ or backtick sliced out of icon.svg can't be shell-expanded.
  cat <<'SVG_HEAD'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <rect width="64" height="64" fill="#1f6f5f"/>
  <g transform="translate(6.4 6.4) scale(.8)">
SVG_HEAD
  printf '%s\n' "$bee"
  cat <<'SVG_TAIL'
  </g>
</svg>
SVG_TAIL
} > "$tmp"
rsvg-convert -w 512 -h 512 "$tmp" -o public/pwa-512-maskable.png

sha256sum "$svg" | cut -d' ' -f1 > scripts/icon.sha256

echo "rendered: pwa-192.png pwa-512.png pwa-512-maskable.png (icon.sha256 updated)"
