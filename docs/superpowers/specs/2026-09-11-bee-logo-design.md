# A bee for logb

Status: superseded by [2026-09-13-logb-mark-design.md](2026-09-13-logb-mark-design.md). The bee is retired.

## Problem

`frontend/public/icon.svg` is a teal rounded square containing a stylised **M**. It was drawn
when the app was called *memto*, and has meant nothing since the rename. It is the favicon, the
apple-touch icon, and the source of the two PWA PNGs, so the stale letter is on every browser tab
and every phone home screen the app is installed on.

There is also no logo anywhere in the app itself. Login and Setup — the two screens a person sees
before any data exists, and the emptiest in the product — carry nothing but a heading and two
fields.

## Scope

In scope: a bee, hand-authored as SVG; the icon and PWA assets rendered from it; the logo placed
on Login and Setup; and a copy in the export archive so a backup is recognisably logb's.

Out of scope: the top bar. On a phone that row already holds a back button, the title, and the
pending/dead outbox chips, and a logo would compete with all three for the narrowest space in the
app. Also out of scope: any change to the palette. The bee joins the existing colours rather than
introducing a scheme.

## The artwork

A geometric bee, seen from above and tilted 14°: a capsule body, two stripes, a stinger, a round
head with two antennae, and two translucent wings. Built from a rect, two ellipses, two circles,
a triangle and one clip path — no gradients, no filters, no embedded raster.

Colours, all already in the product:

| Element | Colour | Source |
|---|---|---|
| Plate | `#1f6f5f` | the existing accent, and the manifest's `theme_color` |
| Body | `#f2b134` | amber |
| Head, stripes, stinger, antennae | `#123c34` | dark teal |
| Wings | white at 40–55% opacity | — |

The bee is the bright element and the plate stays the app's teal, so the tab silhouette people
already recognise does not change — only what is inside it. The stripes sit *on* the amber body
rather than relying on the background behind them, so the artwork needs no light/dark variant.

## Files

```
frontend/public/icon.svg              tight: rounded square, bee at full size
frontend/public/pwa-192.png           rendered from icon.svg
frontend/public/pwa-512.png           rendered from icon.svg
frontend/public/pwa-512-maskable.png  full-bleed plate, bee inset to the safe zone
```

**One drawing, several renders.** Android masks an adaptive icon to whatever shape the launcher
uses, clipping anything outside the middle ~80%, so the maskable asset needs padding the favicon
should not have. Rather than keep two artworks in step — they drift, and nothing would catch it —
the maskable PNG is produced from the same SVG with the bee scaled into the safe zone. A
committed script renders all three PNGs; the PNGs stay committed, exactly as they are today, so
CI needs no image tooling.

The manifest currently declares `pwa-512.png` as `purpose: 'any maskable'` while it is not drawn
for a mask. It drops to `'any'`, and the new maskable file is declared separately.

## Where it appears

- **Favicon and apple-touch icon.** Already wired in `frontend/index.html`; only the bytes change.
- **Login and Setup.** The plated icon above the heading, at a size that reads as a logo rather
  than a decoration. Plated rather than bare, so there is one asset and no second variant.
- **The export archive.** `icon.svg` at the root of the zip. The backend already embeds
  `frontend/dist/` through `rust-embed` to serve the SPA, so the export reads that same embedded
  file rather than keeping a second copy — `Assets` becomes `pub(crate)` and nothing else moves.

## Testing

- The export test asserts the archive contains `icon.svg`.
- A test asserts every icon the manifest names exists in `frontend/public/`, and that exactly one
  of them is declared maskable. This is the one that catches the real failure mode: somebody
  edits the SVG, forgets to re-render the PNGs, and ships a bee on the tab with an M on the home
  screen.

The artwork itself is not asserted pixel by pixel. A test that encodes the drawing would fail on
every deliberate edit while proving nothing about whether it looks like a bee.
