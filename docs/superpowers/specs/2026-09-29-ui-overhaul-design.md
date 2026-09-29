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

- **Old and new CSS coexist.** `app.css` is imported into a cascade layer, `legacy`, ordered
  after Tailwind's `base` and before `components` and `utilities`: its rules beat Tailwind's reset
  and lose to utility classes. Its element selectors (`button`, `input`, `select`, `textarea`)
  skip shadcn components, which carry a `data-slot` attribute. A marked block in `app.css`
  restores what Tailwind's reset takes from unmigrated screens (heading weights, list bullets,
  inline images and icons, link underlines). A round deletes the `app.css` rules for the screens
  it migrates. Round 5 deletes `app.css`.
- **Token names.** shadcn's names collide with `app.css` (`--muted` and `--accent` mean different
  things in each). The shadcn tokens are therefore declared as `--ui-*` (`--ui-background`,
  `--ui-muted-foreground`, …) and mapped to Tailwind colours in `@theme inline`; component class
  names (`bg-muted`, `text-primary-foreground`) are unchanged.
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
  it sets `data-theme` on `<html>`, and Tailwind's `dark:` variant is redefined to key off
  `[data-theme="dark"]`. Dark mode separates
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
refuels appear as heating fuel. Both queries behind it (monthly usage and tank levels) count an
object only if its `resource_kind` is `heating_fuel`, or if it has no `resource_kind` (objects
created with the legacy `fuel_unit` alone) and no distance counter (`counter_unit` is `NULL` or
`'h'`). Tests on SQLite and PostgreSQL cover: a car with `vehicle_fuel` refuels is excluded; a car
with only `fuel_unit` and `counter_unit: km` is excluded; a home with `heating_fuel` is counted;
a legacy generator (`fuel_unit: gal`, `counter_unit: h`) is still counted. The Statistics page
already hides the section when there is nothing to show. No UI redesign in this round.

## Round 1 — foundation and app frame (0.19.0)

**Foundation**

- Tailwind v4, shadcn-svelte init, Inter variable font, `tabular-nums` for figures.
- `--ui-*` tokens for light and dark, from the approved mockup, adjusted where the AA check
  required it (contrast computed for each pair):

  | Token | Light | Dark |
  |---|---|---|
  | background | `#fafafa` | `#09090b` |
  | card, popover | `#ffffff` | `#18181b` |
  | foreground | `#18181b` | `#fafafa` |
  | muted, secondary, accent (hover fill) | `#f4f4f5` | `#27272a` |
  | muted-foreground | `#6b6b74` | `#a1a1aa` |
  | border | `#e4e4e7` | `#27272a` |
  | input (control outline, ≥3:1) | `#8a8a93` | `#71717a` |
  | primary | `#f59e0b` | `#fbbf24` |
  | primary-foreground | `#1c1300` | `#1c1300` |
  | brand-ink (amber as text, focus ring) | `#b45309` | `#fbbf24` |
  | destructive | `#b91c1c` | `#f87171` |
  | destructive-foreground | `#ffffff` | `#09090b` |

- **The whole app changes colour and font in round 1**, not only migrated screens: `app.css`'s
  own tokens are repointed to the same values (`--bg`, `--surface`, `--surface-2`, `--text`,
  `--muted`, `--border`, `--control-border`, `--danger`, `--focus`), `--accent` becomes the amber
  fill with dark text on it, and a new `--accent-ink` replaces `--accent` wherever amber is used
  as text or an outline (links, the active tab and nav item, outlines of chosen items). The eight
  tag hues are unchanged.
- Components are added in the round that first uses them. Round 1 adds button and dropdown-menu.
- Type scale (Tailwind `text-*`): 12 / 14 / 16 / 18 / 22 / 28 px. Page titles 22 px on mobile,
  28 px on desktop; section labels 11 px uppercase muted.

**App frame**

- The shell keeps its breakpoint (sidebar from 900 px, `width >= 900px` range syntax).
- Desktop: sidebar with logo at top, four nav items, user and version pinned to the bottom.
  Active item amber-tinted. (The audit's "sidebar ends at the viewport height" was an artefact of
  full-page capture of a fixed element, not a bug.)
- Mobile: bottom nav with the same four items; active item amber.
- `TopBar` (every screen's header) gains an optional subtitle and the new title sizes.
- **"+ Log" action** replaces the object page's three floating buttons (audit #4): one floating
  button. When the object offers only activities it is a plain "+ Log activity" button; when it
  also offers trips or fills/charges it is "+ Log" and opens a dropdown menu upward, listing
  "Log activity", "Log trip", "Log fill"/"Log charge"/"Log water" as the object supports them.
  The dashboard keeps its "+ New object" button (restyled); a dashboard "+ Log" with an object
  picker comes with round 2's dashboard. Round 3 moves the button into the desktop page header.
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
- A `Chart` component (hand-written SVG bar chart, no chart library) is added here. Charts
  hide empty months; figures that do not apply to the type are not shown (the audit found
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
