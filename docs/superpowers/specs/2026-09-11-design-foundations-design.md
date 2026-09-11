# Design foundations

Status: approved design, not yet implemented.

## Problem

`frontend/src/app.css` is 87 lines that have carried the whole app since the first commit. It is
pragmatic and coherent, and it has four gaps that every screen inherits:

- **No spacing scale.** `4`, `6`, `8`, `10`, `12`, `14`, `16`, `20` and `24` px all appear, chosen
  per site rather than from a system, so nothing lines up between components.
- **No type scale.** Nine improvised sizes — `1.4`, `1.15`, `1.1`, `1.05`, `1`, `.9`, `.85`, `.8`,
  `.75rem` — with no ratio between them.
- **No focus styling at all.** `:focus-visible` appears nowhere in the codebase. Anyone navigating
  by keyboard, or with a screen reader that follows focus, has no idea where they are.
- **One radius for everything.** `--radius: 12px` on cards, buttons and dialogs alike, so a
  container and a control read as the same kind of object.

Separately, three emoji stand in for icons — `🔍` search, `⚙️` settings, `📎` edit. They render
differently on every platform, ignore the theme's colour, and are the clearest sign in the app
that nobody chose them.

## Scope

The foundations only. Tokens, focus and motion states, icons, and consistent input and button
styling — plus two layout defects that are scale problems rather than design decisions.

Out of scope, and next: the dashboard, cards earning their space, empty and loading states. This
slice is deliberately invisible on a screenshot; it exists so those changes inherit a system
instead of extending an improvisation.

The palette does not change. Both themes are already coherent, and the gap here is craft.

## Direction

This is a logbook for owned things — service records, meter readings, costs accumulated over
years. Its vernacular is the stamped service booklet: dated entries, and figures that line up.

**Numbers are the content.** `84,210 km`, `€189.00`, `€698.50` are currently set in proportional
`system-ui`, so a stacked column of costs does not align and an odometer reading reads as prose.
Data gets `font-variant-numeric: tabular-nums` and its own role in the type scale.

This is tabular figures *within the existing family* — not a monospace face for data, which is a
different and much more common move. The point is typesetting, not decoration.

## Tokens

**Type** — six steps at roughly a 1.2 ratio, sized for UI density rather than editorial reading:

| Token | Size | For |
|---|---|---|
| `--text-xs` | 12px | hints, captions |
| `--text-sm` | 14px | labels, secondary text |
| `--text-base` | 16px | body |
| `--text-lg` | 18px | section headings, top-bar title |
| `--text-xl` | 22px | page title |
| `--text-data` | 20px | stat values, tabular |

No webfont. This is an offline-first PWA with zero runtime dependencies, and a display face would
cost a network round trip before first paint. A constraint worth naming rather than dressing up.

**Space** — `--space-1` 4px through `--space-6` 32px, on a 4px grid. The stray 6s, 10s and 14s
resolve to the nearest step.

**Radius** — three steps replacing one: `--radius-sm` 8px for inputs and thumbnails, `--radius-md`
12px for cards and dialogs, `--radius-full` for chips and the floating button. The app already
does this by accident; naming it is what stops every element reading as the same rounded
rectangle.

**Focus** — a `--focus` colour with contrast against both grounds *and* against the filled accent,
since the ring has to be visible on a teal button too. Applied as `:focus-visible` with an offset
outline so it never overlaps the control it marks.

**Motion** — one `--transition` token at 150ms, used only for state the user caused. Wrapped in
`prefers-reduced-motion: reduce`, which the app currently ignores.

## Icons

Inline SVG, 20px, `currentColor`, one stroke weight, replacing the three emoji. Inline rather than
a sprite or a library: there are three of them, they must take the theme's colour, and the project
has no runtime dependencies to spend.

## The two layout defects

- **`.stats` orphans its last item.** `repeat(auto-fit, minmax(110px, 1fr))` fits three across on a
  phone and drops the fourth onto its own row, left-aligned under a three-column grid. It reads as
  a bug because it is one.
- **`.chips` scrolls with no affordance.** The filter row runs off the viewport edge mid-word, so
  "Inspection" looks truncated rather than scrollable.

## Testing

Visual foundations are not unit-testable, and asserting on computed CSS values proves only that
the file says what the file says. What is worth testing is the behaviour that is currently absent
and that a regression would silently remove:

- **Focus is visible.** An end-to-end test tabs to a control and asserts a focus indicator is
  actually rendered — not that a class exists.
- **The emoji are gone.** A test asserts no emoji remain in the components that had them, so they
  cannot creep back in.

Everything else is verified by eye against the screens captured before the change: dashboard,
object detail, activity form, settings, search, in both themes.

## What this deliberately does not do

No component is restructured, no screen is relaid out beyond the two defects, and no copy changes.
A reviewer comparing before-and-after screenshots should see the same app, slightly tidier — with
the difference being that the next slice has a system to build on.
