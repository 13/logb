# UI overhaul — design

Date: 2026-09-29. Base: `main` at 0.18.0 (`1d1fe33`).

The user asked to make the web app "more beautiful, more modern, more intuitive, more
userfriendly". They chose a full visual overhaul built on a component library, released
incrementally, in a calm utility style, for phone and desktop equally. An audit of the current UI
(68 screenshots, mobile 390×844 and desktop 1440×900, light and dark) grounds the decisions
below; its findings are cited where a round fixes them.

## Decisions

- **Stack:** shadcn-svelte on Tailwind CSS v4 (`@tailwindcss/vite`) with `bits-ui` and
  `tailwind-variants`. Components are generated into `frontend/src/lib/components/ui/` by the
  `shadcn-svelte` CLI and are then our own code. Only components in use are added.
- **Palette:** zinc neutrals with bee amber as primary (mockup "A"). Red is reserved for overdue
  and destructive states.
- **Object detail on desktop:** two panes — a sticky summary on the left, tabbed history on the
  right (mockup "A").
- **Rollout:** rounds 0–5 below, each with its own branch, implementation plan and release.

## Rules for every round

- **Old and new CSS coexist.** Tailwind's base layer loads before `frontend/src/app.css`, so
  screens not yet migrated render as before. A round deletes the `app.css` rules for the screens
  it migrates. Round 5 deletes `app.css`.
- **Offline and CSP.** No remote assets. The font is bundled with `@fontsource-variable/inter`.
  The CSP in `src/lib.rs` stays as it is: `style-src 'self' 'unsafe-inline'` already allows the
  inline positioning styles bits-ui sets on popovers.
- **Bundle budget.** Round 1 may add at most 25 KB gzipped JavaScript and 15 KB gzipped CSS over
  0.18.0's `frontend/dist`, excluding the font files. Later rounds must not grow JavaScript by more
  than 10 KB gzipped each. Measured with the method in `docs/perf/baseline-2026-09-29-frontend.md`.
- **Accessibility.** Text meets WCAG AA contrast in both themes. Every interactive element has a
  visible focus ring (amber), a 44×44 px minimum touch target on mobile, and a name that
  `getByRole` can find. Motion is 150 ms and off under `prefers-reduced-motion`.
- **Themes.** The existing light/dark/system setting in `frontend/src/lib/theme.ts` keeps working;
  it toggles a `.dark` class on `<html>` that the shadcn tokens key off. Dark mode separates
  layers with surface shades, not borders alone.
- **i18n.** Every new string goes into both `frontend/src/i18n/en.ts` and `de.ts`.
- **Tests.**
  - `npm run check`, `npm test`, and the Playwright suite on both configured viewports pass at
    the end of every round.
  - End-to-end tests that locate by CSS class (about 50 calls: `.card`, `.chip.due`, `.bar-row`,
    `.list-card`, `.thumb-strip img` and others) are rewritten to role, label, text or
    `data-testid` when the round that touches their screen lands. No test is deleted to make a
    round pass.
- **Screenshots.** The audit's capture script becomes `frontend/scripts/shots.mjs` (seeds a
  scratch instance, captures every screen at both sizes in both themes). Each round attaches
  before/after captures of the screens it changed to its merge.
- **Release.** Each round ends with a version bump and tag, as in previous releases.

## Round 0 — statistics fuel bug (0.18.1)

`GET /stats/fuel` (`src/api/stats.rs`, `fuel_usage`) is labelled "Household heating fuel usage"
but counts every object whose resource unit is litres or gallons except water, so a car's diesel
refuels appear as heating fuel. The query is restricted to objects whose `resource_kind` is
`heating_fuel`. Tests on SQLite and PostgreSQL cover: a car with `vehicle_fuel` refuels is
excluded; a home with `heating_fuel` is counted. The Statistics page hides the section when every
month is zero. No UI redesign in this round.

## Round 1 — foundation and app frame (0.19.0)

**Foundation**

- Tailwind v4, shadcn-svelte init, Inter variable font, `tabular-nums` for figures.
- Tokens (`--background`, `--foreground`, `--card`, `--muted`, `--muted-foreground`, `--border`,
  `--primary`, `--primary-foreground`, `--destructive`, `--ring`, radius 12 px) for light and dark,
  starting values from the approved mockup:

  | Token | Light | Dark |
  |---|---|---|
  | background | `#fafafa` | `#09090b` |
  | card | `#ffffff` | `#18181b` |
  | foreground | `#18181b` | `#fafafa` |
  | muted-foreground | `#71717a` | `#a1a1aa` |
  | border | `#e4e4e7` | `#27272a` |
  | primary | `#f59e0b` | `#fbbf24` |
  | primary-foreground | `#1c1300` | `#1c1300` |
  | destructive | `#dc2626` | `#f87171` |

  Final values may shift only to pass the AA contrast check.
- The existing eight tag hues are ported as tokens with the same contrast guarantees.
- Components added: button, card, badge, input, textarea, label, select, checkbox, radio-group,
  switch, tabs, dropdown-menu, sheet, dialog, tooltip, separator, skeleton, sonner (toasts).
- Type scale: 12 / 13 / 14 / 16 / 18 / 22 / 28 px; page titles 28 px on mobile and desktop,
  section labels 11 px uppercase muted.
- A `Chart` component: hand-written SVG bar chart (no library), used from round 3 on.

**App frame**

- Desktop (≥1024 px): full-height sidebar (fixes the audit's sidebar ending at viewport height),
  logo at top, four nav items, user and version pinned to the bottom. Active item amber-tinted.
- Mobile: bottom nav with the same four items; active item amber.
- `PageHeader`: title, optional subtitle, actions on the right. Used by every screen.
- **"+ Log" action** replaces the object page's three floating buttons (audit #4). On mobile it
  is a floating button that opens a bottom sheet; on desktop a button in the page header that
  opens a dropdown. It lists only the entry kinds the object's type supports (activity,
  fill/charge, trip, reading). From the dashboard it first asks for the object.
- Every page reserves bottom space so a floating button never covers content.

## Round 2 — dashboard and objects list (0.20.0)

- Subtitle "N active · €X this year".
- Due reminders: light red card per reminder with title, object, how overdue, and a Snooze
  button. "Coming up": plain rows with relative due time. No bullets, no underlines, no object
  tags on reminders (audit #2, #3).
- Toolbar row: search, sort, Active/Archived segmented control.
- Object cards: type icon tile, name, "N due" badge that does not stretch (audit #6), key facts,
  tags inside the card (audit #5). The unlabelled per-card "+" is removed; "+ Log" covers it.
  The last-activity date is always relative or labelled (audit #13).
- Desktop: responsive grid of cards, 2 columns from 1024 px, 3 from 1440 px.
- Empty state: one sentence and a "New object" button.

## Round 3 — object detail (0.21.0)

- **Desktop (≥1024 px), two panes.** Left, 300 px, sticky: photo (cover, fixed aspect 16:10,
  fixes the audit's cropping), name, type · since · tags, 2×2 figure cards (total cost, counter,
  per-month usage, consumption where the type has it), due reminders, spend-per-month chart,
  Export and "New object inside". Right: tabs Timeline / Documents / Reminders with "+ Log"
  and Edit in the header. The Info tab does not exist on desktop.
- **Mobile.** Photo, horizontally scrolling figure strip, due reminder, then tabs Timeline /
  Documents / Reminders / Info. Info holds the summary parts that do not fit above.
- Tabs never wrap; badges stay inline (audit #11).
- Timeline: category icon tile, title, date · counter · quantity, amount right-aligned, tags
  inside the entry, year headings. Category filter chips scroll horizontally with an edge fade.
- Charts hide empty months; figures that do not apply to the type are not shown (the audit found
  "Distance per charge" on a diesel car).

## Round 4 — forms (0.22.0)

- All forms share one field pattern: label, control, optional hint, error.
- Required fields first; optional fields in a collapsible "More details" section that opens
  automatically when editing an entry that has any of them set.
- Native selects, radios and checkboxes are replaced by shadcn components (audit #10).
- Mobile: Save / Cancel bar sticky at the bottom above the nav; never hidden on first load.
- Reminder form: a segmented "Due by date / by counter / both" control replaces the ambiguous
  pair of fields; labels drop stacked parentheticals.
- Object form: type first as icon tiles, then name; templates shown after the type with a label.
- Activity form: one "Notes" label (no heading duplicating it); section headings smaller than the
  page title.

## Round 5 — search, statistics, settings, auth (0.23.0)

- Search: results use the dashboard's card style (audit #12).
- Statistics: summary cards first (spend this year, change vs last year, top object), then
  charts. Fuel section below money (round 0 fixed its content). Empty months hidden.
- Settings: grouped rows with distinct icons per section (audit #13). One save model per page:
  settings changes save automatically with a "Saved" toast; actions with side effects (send test
  notification, configure bot, create backup) are explicit buttons. Exactly one primary button
  per page (audit #9). Fixes the appearance page's checkbox alignment.
- Login and setup: centred card with logo, password visibility toggle.
- `app.css` deleted.

## Out of scope

- New features or API changes beyond round 0's query fix.
- Changing routes or URLs.
- Replacing the icon set (existing `Icon.svelte` icons are kept and restyled via tokens).
