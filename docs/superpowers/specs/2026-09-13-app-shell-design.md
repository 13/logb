# App shell: navigation, footer, and a real desktop layout

Status: approved, not yet implemented.

## The problem

LogB has no application chrome. Every screen renders its own sticky `TopBar` carrying a back
arrow, a title, and whatever contextual actions that screen needs. Two of those bars — the
Dashboard's — double as the app's only navigation: a gear that opens Settings and a magnifier
that opens Search. From anywhere else, both are unreachable without walking back up the stack
first.

There is no footer. The version appears once, as a muted line at the bottom of Settings.

There is no desktop layout. `main` is `max-width: 720px; margin: 0 auto`, and the stylesheet
contains exactly one media query — a `max-width: 479px` phone tweak. On a monitor the app is a
phone-width column floating in the middle of the screen.

Three consequences, in the order they hurt:

1. **Navigation lives where a thumb cannot reach it.** The top of a phone screen is the worst
   place for the controls used most often.
2. **Every destination is modal.** Getting from an activity form to Settings means backing out
   of the form, then the object, then the dashboard.
3. **The app does not look like software anyone runs a household — or a business — on.** A
   centred 720px column with no chrome reads as an unfinished phone app regardless of how good
   the screens inside it are.

## What this builds

An app shell: one persistent navigation surface that adapts to the viewport, a footer that owns
the version, and a content area that uses the width it is given.

This spec covers the shell only. The Settings screen's own redesign is a separate spec
(`2026-09-13-settings-hub-design.md`) that depends on this one and must be built after it.

## Navigation

The app has exactly three top-level destinations: **Objects** (`/`), **Search** (`/search`), and
**Settings** (`/settings`). Everything else in the app is a drill-down from one of them. Three
destinations is small enough that all of them are always visible — no overflow menu, no drawer,
no hamburger.

The icons all already exist in `Icon.svelte`: `object`, `search`, `settings`.

### Desktop (viewport ≥ 900px): persistent left sidebar

A `<nav>` fixed to the left edge, 240px wide, full viewport height:

- the LogB wordmark at the top,
- the three destinations as a vertical list — icon plus label, generous hit area,
- the version pinned at the foot.

Content occupies the remaining width.

### Mobile (viewport < 900px): bottom tab bar

A `<nav>` fixed to the bottom edge:

- the three destinations, evenly divided, icon above label,
- height 56px plus `env(safe-area-inset-bottom)`,
- it sits above page content, which gains matching bottom padding so nothing hides beneath it.

`TopBar` stays exactly as it is, on every screen. It is a *title* bar — title, back arrow,
contextual actions — not navigation. The two navigation icons currently in the Dashboard's
`TopBar` (gear, magnifier) are removed, because the nav now carries both from every screen.

### Where the shell does and does not render

The shell is for signed-in users. `App.svelte` already special-cases `/setup` and `/login`
ahead of its route table, and those two screens render **without** sidebar, tab bar, or footer:
there is nowhere to navigate to before you are signed in, and a nav offering three destinations
that all bounce off the route guard is worse than no nav. The same applies while the session is
still loading (`$user === undefined`).

### Active state

The destination matching the current route is marked with `aria-current="page"` and styled with
the accent colour. A drill-down route marks its parent destination: `/objects/42` and
`/objects/new` both mark **Objects**; `/settings/appearance` marks **Settings**. The rule is
prefix matching against the destination's own path, with `/` matching only itself and anything
beginning `/objects`.

## Footer

The footer carries the version and nothing else. `__APP_VERSION__` is already a build-time
global and is already used for exactly this string.

- **Desktop:** the foot of the sidebar.
- **Mobile:** the end of the page content, above the tab bar — not fixed, because a permanently
  visible version string is chrome nobody needs on a small screen.

Settings loses its own version line to this footer.

## Content width

Wider is not better everywhere. Two rules, applied by content type rather than by screen:

- **Lists, cards, grids, tables** use the full available content width. These are the things a
  desktop screen genuinely helps.
- **Forms and prose** stay capped at `68ch`. A text input stretched across a 27" monitor is
  worse than one that is not, and a paragraph at 200 characters per line is unreadable.

On mobile both collapse to the same thing — full width minus the page gutter — so the phone
layout is unchanged apart from the bottom padding the tab bar needs.

## The floating action button

The `+ New object` FAB (and the `+ Log activity` FAB on an object's timeline) currently sits
bottom-right, which is precisely where the tab bar goes.

- **Mobile:** the FAB lifts to sit directly above the tab bar, keeping its bottom-right
  position and its `env(safe-area-inset-bottom)` allowance.
- **Desktop:** the FAB sits at the bottom-right of the *content pane*, clear of the sidebar.

This is the one genuine collision the shell creates, and getting it wrong makes the app's
primary action unreachable, so it is called out here rather than left to the implementation.

## Accessibility

- The navigation is a real `<nav>` landmark; the content area is `<main>`.
- The active destination carries `aria-current="page"`.
- Every destination is a real link or button, reachable and operable by keyboard, with the
  project's existing visible focus treatment (`--focus`).
- Hit areas stay at or above the project's existing `44px` minimum.
- The existing `prefers-reduced-motion` block continues to apply; the shell adds no motion that
  is not covered by it.

## Theming

The shell uses the existing tokens and adds none: `--bg`, `--surface`, `--border`, `--accent`,
`--accent-text`, `--muted`, the spacing scale, and `--radius-*`. Both themes are already fully
defined, so the shell inherits dark mode with no new colour decisions.

## Testing

- **Unit (vitest):** the active-destination rule is a pure function — given a path, which of the
  three destinations is current. It gets its own module and its own tests, including the
  drill-down cases (`/objects/42` → Objects, `/settings/appearance` → Settings, `/search` →
  Search, `/` → Objects) and the case where nothing matches.
- **End-to-end (Playwright):** the existing suite runs against one Pixel 7 project. This spec
  adds a **second Playwright project at a desktop viewport**, because a desktop layout that is
  never rendered in a test is a desktop layout that will regress silently. New specs cover: the
  tab bar is present on mobile and the sidebar is not; the sidebar is present on desktop and the
  tab bar is not; each destination navigates; the active destination is marked on a drill-down
  route; and the FAB does not overlap the tab bar.
- **No regression:** all 33 existing e2e specs must still pass. They assert by role and
  accessible name, so removing the Dashboard's two `TopBar` icons is the one change likely to
  touch them — any spec that reaches Settings or Search through those icons must be updated to
  use the nav instead.

## Out of scope

- Any change to the screens themselves beyond the FAB position and the Dashboard's two removed
  `TopBar` icons.
- The Settings redesign, which is its own spec and comes after this one.
- A desktop-specific treatment of any individual screen's internals. This spec gives every
  screen a wider container; deciding what each screen *does* with that width is that screen's
  own work.
- Offline, sync, and PWA install behaviour, none of which the shell touches.
