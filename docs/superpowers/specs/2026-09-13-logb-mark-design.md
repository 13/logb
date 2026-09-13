# The LogB mark

Status: implemented. Replaces the bee ([2026-09-11-bee-logo-design.md](2026-09-11-bee-logo-design.md)).

## Why

The bee was a pun on the B in LogB and said nothing about what the app is. The replacement
started as a supplied raster sketch: a ring around three left-aligned bars getting shorter. That
sketch was a 1.1 MB RGB PNG with a checkerboard painted into its pixels, a ring of uneven weight,
and it read as a generic filter/sort control — a problem for a logo that sits next to real
controls.

## The artwork

Redrawn by hand on the 64-unit grid, not traced:

- ring: `r=20`, stroke `4.5`
- three bars, round caps, stroke `4.5`, at `y = 25 / 32 / 39`, starting at `x = 22`,
  lengths roughly 1 : 0.85 : 0.62
- the top bar starts with a dot (`r=2.9`): the newest entry in the log. That one detail is what
  moves it from "filter icon" to "log entry", and it survives at 16px, where a timeline variant
  with a spine and three nodes did not.

Colours: the existing accent `#1f6f5f` as the plate, white ink. No second colour.

Four candidates (clean redraw, entry dot, timeline, amber dot) were compared at 16–128px, on light
and dark tabs, masked, and in-app; the entry dot was chosen.

## One drawing

`frontend/public/icon.svg` holds the only copy, with the ink between `<!--mark-->` markers.

| Consumer | How it gets the mark |
|---|---|
| favicon | `icon.svg` directly |
| PWA 192/512, apple-touch 180 | `scripts/render-icons.sh` |
| PWA maskable 512 | render script slices the markers onto a full-bleed plate at 80% |
| Login, Setup, desktop sidebar | `Logo.svelte` imports `icon.svg?raw`, slices the markers, swaps `#ffffff` for `currentColor`, so the in-app mark follows `--accent` in both themes with no plate |
| export archive | `icon.svg` embedded via `rust-embed`, unchanged |
| README | `icon.svg` |

## Where it appears in the app

Login and Setup, as before. New: the desktop sidebar, beside the wordmark. Not the phone top bar,
which is already full — the wordmark and its logo are hidden below 900px.
