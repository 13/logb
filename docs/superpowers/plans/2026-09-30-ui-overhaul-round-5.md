# UI overhaul, round 5 (search, statistics, settings, auth, app.css deleted): implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the overhaul. Search hits become the dashboard's cards. Statistics open with
three summary cards for a year (spent, change against the year before, top object), then the
money charts with empty months hidden, then energy, fuel and water. Settings become grouped rows
with an icon of their own each; settings save themselves with a "Saved" toast, actions with side
effects keep explicit buttons, and no page has more than one primary button. Sign-in and setup
become a centred card with a password visibility toggle. The shared tokens, the tag chips, the
shell padding and the floating-button position move into `app.tw.css`; every screen still on
`app.css` is migrated; `app.css`, its revert block and the `legacy` cascade layer are deleted.
The app icon turns amber. Release as 0.23.0.

**Architecture:** Everything new is imported by lazy routes only (Search, Stats, the nine settings
pages, and Login/Setup, which become lazy routes in this round). Shared pieces: `lib/autosave.ts`
(debounced, serialised, latest-wins saver), `lib/toast.ts` + `lib/Toaster.svelte` (one polite live
region), `lib/PasswordInput.svelte` (plain class strings, inline icons), a `fab-pos` Tailwind
`@utility`, and the shell/tag rules as plain CSS in `app.tw.css` (no JavaScript bytes on the
first load). Statistics computes its summary client-side from the existing `/stats?year=`
responses (`yearSummary` in `lib/stats.ts`); no API change.

**Tech Stack:** Svelte 5, Tailwind v4 (`frontend/src/app.tw.css`), bits-ui 2 (Checkbox via
`CheckField` only), @lucide/svelte, Vitest, Playwright, rsvg-convert (icon render script).

**Spec:** `docs/superpowers/specs/2026-09-29-ui-overhaul-design.md` (Rules for every round,
Round 4's notes for round 5, Round 5).

**Baseline (0.22.1, measured 2026-09-30 with `gzip -9`):** entry chunk `index-*.js` 11,790 B
(the spec's 11,788 B plus the release bump); eager JS set (entry plus every `modulepreload`)
62,675 B; all JS 216,366 B; entry CSS 12,262 B. Removing `Login` and `Setup` from the entry chunk
alone (measured in a scratch copy) takes it to 10,892 B.

## Decisions (refinements of the spec, for the controller to relay)

1. **Login and Setup become lazy routes, drawn with plain class strings.** The spec allows either;
   both together is the only way to add a card, a logo and a password toggle without growing the
   entry chunk (they cost 898 B gzip there today). Every signed-in start stops paying for two
   screens it never shows; a first visit pays one small extra request, precached afterwards. They
   use `field/classes.ts` strings on native elements and `PasswordInput.svelte` (no `cn`, no
   tailwind-merge, no field context, no bits-ui), so the auth chunk stays about 2 KB and does not
   pull the 25 KB `field` chunk before the first sign-in. App.svelte routes them through the same
   loader as every other page (chunk-failure reload, "Loading…" after 300 ms).
2. **The password toggle is named by hidden text, not `aria-label`,** and uses `aria-pressed`
   with a fixed name ("Show password" / "Passwort anzeigen"). `getByLabel(/Password|Passwort/)`
   in `helpers.ts` would otherwise match the German toggle too (strict-mode failure).
3. **The toast is our own 30-line live region, not shadcn's sonner.** svelte-sonner 1.2.1 is 101 KB
   unpacked (several KB gzip plus CSS), half the round's 10 KB budget, for stacking, swipe and
   promise toasts nobody here needs. `toast(text, ms)` shows one message at a time in a
   `role="status"` region that each saving page mounts empty (a region inserted together with its
   text is often not announced). Errors are never toasts: they stay under their field or at the
   top of the page, `role="alert"`, until fixed.
4. **One save model per page.**
   - Settings save themselves: selects and checkboxes on change; text, number and URL fields on
     `change` (blur or Enter), after client-side validation. `autosave()` waits 300 ms, sends one
     request at a time, and sends only the newest value; leaving the page flushes a waiting save.
     Success shows "Saved"; with "Use these preferences only on this device" on, nothing is sent
     and the toast says "Saved on this device". Turning that box off saves the current look to the
     account (what "Apply appearance" did).
   - Explicit buttons stay for side effects: change password, sign out, sign out everywhere,
     pairing code, push on/off, Telegram configure/connect/disconnect/remove, test notification,
     create/copy/revoke token, export, import, retry/discard failed saves, add/remove user,
     test/switch/restart database, save/delete type.
   - Removed buttons: "Apply appearance", Appearance's instance "Save", "Apply delivery time",
     the webhook "Save".
5. **"Exactly one primary button per page" is read as "at most one, and it is the page's main
   action".** A page of settings that save themselves has nothing to press, and inventing a button
   for it would bring the pattern this round removes back. Per page: Hub — "Try again" only while
   saves failed; Appearance — none; Account — "Change password"; Notifications — "Turn on
   notifications" while push is off; API access — "Create token"; Data — "Export everything";
   Users — "Add user"; Database — "Copy data and switch"; Types — "Add type", or "Save type" while
   the form is open. An e2e test counts filled amber buttons per page.
6. **Statistics: the summary is a year, the charts are the selection.** The cards describe the
   selected year, or the current year under "All years"; the charts below keep describing the
   selection ("All years" draws one bar per year). "Change vs last year" is computed from
   `/stats?year=Y` answers the API already gives: for the running year it compares the same months
   (January to the current month) of the year before, labelled "vs Jan–Sep 2025"; for a past year
   it compares whole years. With nothing spent the year before, the card says so instead of a
   percentage. Cost: one or two extra `GET /stats?year=` per change of year or purchases toggle.
   The total of the selection stays on screen (`data-testid="stats-total"`) beside "Spend over
   time".
7. **`BarList` stays for ranked lists; every series over time becomes a `Chart`.** Spend over
   time, energy, litres, gallons and water per month use `Chart` (hides empty months, the audit's
   zero rows). By object (tree), by type, by category and water per object stay `BarList`,
   restyled: 44 px expand toggle, fill in `brand-ink` (3:1 against the track, like Chart's bars;
   the amber fill is 2.1:1), `labelClass` prop instead of Stats' `:global(.label)` override.
   Insights on the object page gets the new look for free.
8. **Settings icons come from lucide, in the lazy settings chunk,** not from `Icon.svelte`, which
   is in the eager chunk: Appearance `Palette`, Account `UserRound`, Notifications `Bell`, Types
   `Shapes`, API `KeyRound`, Data `Archive`, Users `UsersRound`, Database `Database`.
   `settings-rows.ts` names them with its own `SettingsIcon` union.
9. **`.tag`, `.tag-N` and `.tags` keep their names** and move to `app.tw.css`'s `components` layer
   with the `--tag-*` tokens. They are a design-system primitive with eight hashed hues, not a
   legacy class; the 16 e2e `.tag` locators in `23-tags` and `36-dashboard` stay valid.
10. **The shell is CSS, not utilities in App.svelte.** `.app-content` and `.app-content > main`
    (padding, widths, the phone's tab-bar clearance) move to `app.tw.css`'s `components` layer, so
    they cost the entry chunk nothing and a page's own utilities still override them. Auth pages
    and App's loading screens sit outside `.app-content` and set their own classes. The floating
    buttons (`LogPicker`, Reminders' "+ New reminder", ObjectDetail's "+ Log" row) share a
    `fab-pos` `@utility` with app.css's `.fab` maths.
11. **`interactive-widget=resizes-content` is added to the viewport meta.** On Chrome for Android
    the layout viewport then shrinks with the keyboard, so the sticky Save bar sits above it, as
    the spec intends. iOS Safari ignores the key and keeps its current behaviour. Playwright cannot
    open an on-screen keyboard; the test emulates the resized viewport (a 390×420 page with the
    title field focused) and checks the Save bar and the field are both on screen. A real-phone
    check remains (see the open question).
12. **Line height.** Deleting the revert block hands `html` back to Tailwind's 1.5 line height for
    text without a `text-*` utility (every `text-*` utility sets its own). Accepted as the better
    default for reading; Task 9 compares every screen before and after and fixes any row that
    breaks with an explicit `leading-*`.
13. **`tests/scale.test.ts` becomes an invariant that no `.svelte` file has a `<style>` block**
    (and that `app.css` is gone). Its checks guarded scoped styles against app.css's
    `--space-*`/`--text-*`/`--radius-*` scale; after this round neither exists, and its "every
    exemption is still needed" test would fail on an empty set.
14. **App icon: amber plate `#f59e0b`, ink `#1c1300`** (9.2:1). White on amber is 2.1:1. `Logo.svelte`
    swaps the new ink for `currentColor` and draws in `text-brand-ink` in the app; the maskable
    plate in `render-icons.sh` follows; `tests/pwa-icons.test.ts` checks the new ink.
15. **`theme.ts` reads `--ui-background`** for `theme-color`; `index.html`'s comment follows.
16. **Leftovers the spec does not list, migrated in Task 9 because they use app.css:** TopBar
    (`.topbar`, `.ghost`, `.chip.pending/.dead`, its `<style>`), UpdateBanner, WeightHistory,
    Reminders' done dialog and `.error`, Documents' `.error`, Dashboard's `.error`/`.muted`/`.empty`/
    `button.primary`, App.svelte's loading and 404 screens. Dashboard, TopBar and UpdateBanner are
    eager: plain class strings, no `classes.ts` import.
17. **Account:** the signed-in card (with its "Sign out") replaces the plain username and the ghost
    "Sign out"; the new-password field is labelled "New password"; its button says "Change
    password". **i18n:** `settings.save-appearance` and `notify.save-hour` are deleted (unused).

**Changed e2e assertions (all named again in the release report):**
- Behaviour:
  - `20-statistics`: `stats-over-time … .bar-row` count 12 → `getByTestId('chart-bar')` count 3
    (empty months hidden).
  - `17-notifications`: "Save" clicks → leaving the field (`press('Tab')`); the alert and the
    "Saved" status assertions stay.
  - `33-personal-preferences`: no "Apply appearance" / "Apply delivery time" clicks; wait for the
    toast ("Saved" or "Saved on this device") and poll the API before reloading.
  - `08-foundations`: the filled button is found by its computed amber background instead of
    `.primary`; the ring colour is `var(--ui-ring)` instead of `var(--focus)`.
- Locator only: `23-tags` `.hit-row` → `getByTestId('search-hit')`; `24-own-types` `.icon-choice`
  → `eBike.check()`, `.type-row` → `getByTestId('type-row')`, `.bar-row` → `getByTestId('bar-row')`;
  `14-settings` `.card.row` → `getByTestId('user-row')` / `getByTestId('failed-write')`; `13-shell`
  `main .signed-in` → `getByRole('main').getByTestId('signed-in')`, `main dl.about` →
  `getByTestId('about')`, `main.auth form` → `getByTestId('auth-form')`, `--space-5` → `24`;
  `07-api-tokens` `.fresh-token code` → `getByTestId('fresh-token')`, `.list` →
  `getByTestId('token-list')`.
- Unchanged but exercised (run them): `01-smoke`, `03-search`, `10-database`, `11-controls`,
  `25-offline-cache`, `26-date-format`, `31-water`, `32-calendar-reminders`, `30-weight`.

**Open question for the user:** the keyboard-open check on a real phone (Decision 11). The plan
adds `interactive-widget=resizes-content` and an emulated test; only a real Android and a real
iPhone show whether the Save bar and the tab bar above the keyboard leave enough room. Nothing else
needs the user.

## Global Constraints

- **Chunk boundary.**
  - `$lib/components/ui/**`, `bits-ui`, `@lucide/svelte`, `$lib/utils` (`cn`), `autosave.ts`,
    `toast.ts`, `Toaster.svelte`, `PasswordInput.svelte` and `field/classes.ts` are imported only
    by lazy routes and what they import. Nothing `main.ts`, `App.svelte`, `Dashboard.svelte`,
    `TopBar`, `AppNav`, `AppFooter`, `SignedIn`, `AccountMenu`, `LogPicker`, `ObjectCard`,
    `TagChips`, `DashboardReminders`, `UpdateBanner`, `Logo` or `Icon` pulls in eagerly may import
    them. Eager components use plain class strings written out in place.
  - Entry chunk `index-*.js` ≤ 11,988 B gzip (baseline + 200 B; expected to end near 11.1 KB).
  - Eager JS set (entry + every `modulepreload` in `dist/index.html`) grows by ≤ 1,024 B.
  - CSS (controller decision, Task 4 review): CSS is measured as entry CSS and total CSS (sum of
    all `dist/assets/*.css` gzip). Per task: report both; flag only if total CSS grows by more than
    ~300 B net. At round end, after app.css is deleted: entry CSS ≤ 12,262 B and total CSS ≤
    17,081 B. Prefer theme values over arbitrary-value utilities and reuse shared class strings.
  - Round budget: +10 KB gzip JS in total.
- WCAG AA: text ≥ 4.5:1; focus rings, control outlines, chart bars and state indicators ≥ 3:1, in
  both themes. Every new tinted pair gets a computed-blend test in
  `frontend/tests/theme-contrast.test.ts`. Already tested and reused: brand-ink on `primary/10`
  over card (icon tiles, checked tiles), muted-foreground on card/muted/accent, destructive on
  `destructive/10` over background, destructive-foreground on destructive, `input` against card.
- Touch targets: form buttons next to fields and page actions are `h-12` (48 px, `--control`);
  other tap targets `min-h-11` (44 px) and, when icon-only, `size-11`; checkboxes use `CheckField`
  (44 px hit area).
- Every new string goes into both `frontend/src/i18n/en.ts` and `de.ts` (append before
  `} as Record<string, string>;`). `tests/i18n.test.ts` checks the key sets match.
- A migrated component deletes its `<style>` block (scoped styles are unlayered and beat
  utilities). No `.svelte` file has a `<style>` block at the end of the round.
- Until Task 9 deletes it, app.css's revert block reaches new markup: set `display`,
  `list-style` (`list-none` plus `role="list"`), heading sizes and `m-0` explicitly; `p-0 border-0`
  on `fieldset`/`legend`. Never use the legacy class names `field`, `row`, `toggle`, `hint`, `warn`,
  `warning`, `error`, `card`, `list`, `muted`, `empty`, `chip`, `banner`, `button-like`, `primary`,
  `danger`, `ghost`, `tnum`, `small`, `fab`, `topbar`, `auth`, `settings-grid` in new markup.
- Plain `<button>`s and native `<input>`/`<select>`/`<textarea>`/`<a>` in new markup carry
  `data-slot="…"` (exempts them from app.css's element rules while it exists).
- Focus rings on every interactive element that is not a shadcn component:
  `focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring`
  (`-outline-offset-2` inside a clipping or scrolling box). A ring next to an amber fill uses the
  offset so it sits on the page, never on the fill. Label-drawn controls use `has-focus-visible:`.
- Clickable cards: stretched-link pattern, `relative isolate` on the card, the opening button's
  `after:absolute after:inset-0`, siblings that are their own controls raised with `relative z-10`.
- Breakpoints: `desk:` (900 px, the shell) and `wide:` (1024 px, two-column layouts). Between 900
  and 1023 px the sidebar shows while the object page stays single-column; that is intentional.
- Forms that load their values asynchronously set `aria-busy` until loaded.
- Bind-and-react: a control that must run code after its value changes uses a function binding
  (`bind:value={() => x, (v) => { x = v; after(); }}`). A text `Input` may combine `bind:value`
  (the `input` event) with `onchange` (the `change` event): they are different events.
- e2e:
  - No test is deleted. Changed assertions are the ones listed above; any other change found
    necessary is reported with its reason.
  - New tests go into `frontend/tests-e2e/39-screens.spec.ts`.
  - Test ids this round introduces: `search-hit`, `hit-icon`, `stats-summary`, `stats-spent`,
    `stats-change`, `stats-top`, `bar-row`, `toast`, `signed-in`, `about`, `failed-write`,
    `user-row`, `token-list`, `fresh-token`, `type-row`, `icon-choice`, `auth-form`.
- Screenshots before and after each task with `frontend/scripts/shots.mjs`:
  1. Build first: `cd frontend && npm run build && cd .. && cargo build`.
  2. Start the server:
     `rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &`
  3. Capture: `(cd frontend && node scripts/shots.mjs ../shots/r5-tN <filter>)` with the filter the
     task names (a comma list of id substrings).
  4. Kill the server afterwards (`kill %1`). The `/logb` process under uid 65532 is the user's own
     container; never touch it.
- Measuring (used in Task 1 and Task 11):
  ```bash
  measure() { # $1: label
    (cd frontend && for f in dist/assets/*.js dist/assets/*.css; do printf '%s %s\n' "$(gzip -9c "$f" | wc -c)" "$f"; done > ../.superpowers/sdd/bundle-r5-$1.txt
     s=0; for f in $(grep -o '/assets/[^"]*\.js' dist/index.html | sort -u); do s=$((s + $(gzip -9c "dist$f" | wc -c))); done; echo "eager $s" >> ../.superpowers/sdd/bundle-r5-$1.txt)
  }
  ```
- Commits end with these two trailer lines, after a blank line:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NdJo5cgB7BK3PsYZHsHs5T
  ```
- No merge, tag or push: the round stops at a local release commit.

---
### Task 1: Shared tokens, tags, shell and floating buttons move to app.tw.css

**Files:**
- Modify: `frontend/src/app.tw.css`, `frontend/src/app.css`, `frontend/src/lib/theme.ts`,
  `frontend/index.html`, `frontend/src/lib/LogPicker.svelte`, `frontend/src/lib/Reminders.svelte`,
  `frontend/src/routes/ObjectDetail.svelte`, `frontend/scripts/shots.mjs`,
  `frontend/tests/tags.test.ts`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests/theme.test.ts`, `frontend/tests-e2e/13-shell.spec.ts`
- Create: `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Produces:
  - CSS custom properties on `:root`: `--navbar` (56px), `--control` (48px), `--tag-0-bg` …
    `--tag-7-fg` (light and dark).
  - Classes `.tags`, `.tag`, `.tag-0` … `.tag-7`, `.tag.active` (components layer, unchanged names).
  - Shell rules for `.app-content` and `.app-content > main` (components layer).
  - Tailwind utility `fab-pos`.
  - Base rules: `body` colours, `:focus-visible` default ring, reduced motion, no WebKit search ✕.
  - `applyTheme` reads `--ui-background`.
  - Test helper in `theme-contrast.test.ts`: `cssVar(name, theme)`; `ui(name, theme)` built on it.

- [ ] **Step 1: Branch, measure, capture "before"**

```bash
git switch -c ui-round-5 main
cd frontend && npm run build && cd .. && cargo build
mkdir -p .superpowers/sdd
measure() { # see Global Constraints
  (cd frontend && for f in dist/assets/*.js dist/assets/*.css; do printf '%s %s\n' "$(gzip -9c "$f" | wc -c)" "$f"; done > ../.superpowers/sdd/bundle-r5-$1.txt
   s=0; for f in $(grep -o '/assets/[^"]*\.js' dist/index.html | sort -u); do s=$((s + $(gzip -9c "dist$f" | wc -c))); done; echo "eager $s" >> ../.superpowers/sdd/bundle-r5-$1.txt)
}
measure before
grep 'index-\|eager' .superpowers/sdd/bundle-r5-before.txt
```

Expected: `index-*.js` about 11,790, `index-*.css` about 12,262, `eager` about 62,675.

- [ ] **Step 2: More screens in `frontend/scripts/shots.mjs`.** In `SCREENS`, after the
  `16-settings-notifications` line, add:

```js
  ['20-settings-account', '/settings/account'],
  ['21-settings-types', '/settings/types'],
  ['22-settings-api', '/settings/api'],
  ['23-settings-data', '/settings/data'],
  ['24-settings-people', '/settings/people'],
  ['25-settings-database', '/settings/database'],
```

  In `seed`, append to the `entries` array (last year's spend, so the statistics summary has
  something to compare with):

```js
    [car, { date: day(400), category: 'repair', title: 'Clutch replaced', counter_value: 38000, cost_cents: 64000 }],
    [house, { date: day(380), category: 'maintenance', title: 'Roof inspection', cost_cents: 18000 }],
```

  Capture every screen for "before":

```bash
rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &
sleep 2 && (cd frontend && node scripts/shots.mjs ../shots/r5-before); kill %1
```

- [ ] **Step 3: Failing unit tests.**
  - `frontend/tests/tags.test.ts`, inside `describe('palette contrast', …)`, replace the `css` and
    `blocks` constants with:

```ts
  const css = readFileSync(fileURLToPath(new URL('../src/app.tw.css', import.meta.url)), 'utf8');
  // Each theme block, as written in app.tw.css.
  const light = ':root {';
  const dark = ":root[data-theme='dark'] {";
  const blocks = {
    light: css.slice(css.indexOf(light), css.indexOf('}', css.indexOf(light))),
    dark: css.slice(css.indexOf(dark), css.indexOf('}', css.indexOf(dark))),
  };
```

  - `frontend/tests/theme.test.ts`: in `fakeDoc`, `p === '--bg'` → `p === '--ui-background'`.
  - `frontend/tests/theme-contrast.test.ts`: replace the `ui` constant (below `twDark`) with

```ts
/** A custom property's hex value in a theme block of app.tw.css, inherited from :root when the
 *  dark block does not set it. */
const cssVar = (name: string, theme: string): string => {
  const own = theme.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})`))?.[1];
  const value = own ?? twLight.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})`))?.[1];
  expect(value, `--${name}`).toBeTruthy();
  return value!;
};
const ui = (name: string, theme: string): string => cssVar(`ui-${name}`, theme);
```

    and in the test `tag text on its chip and on the due card's tint`, delete the line
    `const legacy = name === 'light' ? light : dark;` and replace both
    `token(\`tag-${n}-fg\`, legacy)` / `token(\`tag-${n}-bg\`, legacy)` calls with
    `cssVar(\`tag-${n}-fg\`, theme)` / `cssVar(\`tag-${n}-bg\`, theme)`.

Run: `cd frontend && npx vitest run tests/tags.test.ts tests/theme.test.ts tests/theme-contrast.test.ts`
Expected: FAIL — no `--tag-*` in app.tw.css; `applyTheme` still reads `--bg`.

- [ ] **Step 4: `frontend/src/app.tw.css`.**
  - At the end of the `:root {` block (after `--ui-warn: #915700;`) add:

```css
  /* The shell's measures: the phone's tab bar, which the Save bar, the floating buttons and the
     toast sit above, and the one control height every form control shares. */
  --navbar: 56px;
  --control: 48px;
  /* Tag chips: eight hues picked by a hash of the tag's name (src/lib/tags.ts). Every pair is
     checked for 4.5:1 by tests/tags.test.ts, and on the due card's tint by theme-contrast. */
  --tag-0-bg: #e3f1ec; --tag-0-fg: #14532d;
  --tag-1-bg: #e4ecfb; --tag-1-fg: #1e3a8a;
  --tag-2-bg: #fbe9e3; --tag-2-fg: #7c2d12;
  --tag-3-bg: #f3e8fb; --tag-3-fg: #581c87;
  --tag-4-bg: #fdf3d8; --tag-4-fg: #713f12;
  --tag-5-bg: #fce7ef; --tag-5-fg: #831843;
  --tag-6-bg: #e0f4f7; --tag-6-fg: #164e63;
  --tag-7-bg: #eceef1; --tag-7-fg: #1f2937;
```

  - At the end of the `:root[data-theme='dark'] {` block (after `--ui-warn: #f0a640;`) add:

```css
  --tag-0-bg: #16392d; --tag-0-fg: #b7f0d2;
  --tag-1-bg: #1c2d55; --tag-1-fg: #c7d7fe;
  --tag-2-bg: #4a2417; --tag-2-fg: #fed7c7;
  --tag-3-bg: #3a1d52; --tag-3-fg: #e9d5ff;
  --tag-4-bg: #45330c; --tag-4-fg: #fde68a;
  --tag-5-bg: #4d1a33; --tag-5-fg: #fbcfe8;
  --tag-6-bg: #123c47; --tag-6-fg: #bae6fd;
  --tag-7-bg: #2b313a; --tag-7-fg: #e5e7eb;
```

  - Append at the end of the file:

```css
@layer base {
  /* `overscroll-behavior-y: contain` turns off Chrome on Android's pull-to-refresh: overscrolling
     at the top of a timeline is easy one-handed, and a reload loses whatever is typed into an
     open form. Scrolling itself is unaffected. */
  body {
    background-color: var(--ui-background);
    color: var(--ui-foreground);
    -webkit-tap-highlight-color: transparent;
    overscroll-behavior-y: contain;
  }
  /* Keyboard focus on anything that does not draw its own ring. `:focus-visible`, so a tap leaves
     no ring behind; the offset puts the ring on the page, where it shows around an amber fill. */
  :focus-visible {
    outline: 2px solid var(--ui-ring);
    outline-offset: 2px;
  }
  /* Chrome draws its own blue ✕ inside a search field; `appearance: none` does not reach it. */
  input[type='search']::-webkit-search-cancel-button {
    appearance: none;
    display: none;
  }
  /* Motion here only answers something the user did; someone who asked for less gets none. */
  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after {
      animation-duration: 0.01ms !important;
      animation-iteration-count: 1 !important;
      transition-duration: 0.01ms !important;
      scroll-behavior: auto !important;
    }
  }
}

@layer components {
  /* The column every signed-in screen renders as its <main> (App.svelte puts pages in
     .app-content). Plain CSS rather than utilities in App.svelte: the first load pays no script
     bytes for it, and a page's own utilities still win over this layer. Auth pages and the
     loading screens sit outside .app-content and set their own classes.
     Range syntax (`width < 900px` / `width >= 900px`), never 899px/900px: at a fractional width
     (display scaling, zoom) the min/max pair both fail and the nav loses its inset. */
  .app-content > main {
    margin-inline: auto;
    width: 100%;
    max-width: 720px;
    padding: 0.75rem 0.75rem calc(96px + env(safe-area-inset-bottom));
  }
  @media (width < 900px) {
    /* The tab bar's clearance belongs to the whole column; `main` keeps only what a floating
       button needs above its last row. `width: 100%` above matters here: an auto margin on a flex
       item's cross axis cancels the stretch. */
    .app-content {
      display: flex;
      flex-direction: column;
      min-height: 100dvh;
      padding-bottom: calc(var(--navbar) + env(safe-area-inset-bottom));
    }
    .app-content > main {
      flex: 1;
      padding-bottom: 96px;
    }
  }
  @media (width >= 900px) {
    .app-content { margin-left: 240px; }
    .app-content > main { max-width: 1100px; }
  }

  /* Tag chips: a shared primitive kept as a class. Eight hashed hues, used by the dashboard,
     timeline, search, reminders and the tag field. */
  .tags { display: flex; flex-wrap: wrap; gap: 0.25rem; }
  .tag {
    display: inline-flex; align-items: center; gap: 0.25rem;
    border: 0; border-radius: 9999px; min-height: 0;
    padding: 2px 0.5rem;
    font-size: var(--text-xs); font-weight: 600; line-height: 1.4;
  }
  /* A 32 px tall hit area around the ~20 px chip without changing how it looks; horizontally only
     half the row's gap, so neighbouring chips' areas meet but never overlap. */
  button.tag { cursor: pointer; position: relative; }
  button.tag::before {
    content: ''; position: absolute;
    top: calc(50% - 16px); bottom: calc(50% - 16px); left: -0.125rem; right: -0.125rem;
  }
  .tag.active { outline: 2px solid currentColor; outline-offset: 1px; }
  .tag-0 { background: var(--tag-0-bg); color: var(--tag-0-fg); }
  .tag-1 { background: var(--tag-1-bg); color: var(--tag-1-fg); }
  .tag-2 { background: var(--tag-2-bg); color: var(--tag-2-fg); }
  .tag-3 { background: var(--tag-3-bg); color: var(--tag-3-fg); }
  .tag-4 { background: var(--tag-4-bg); color: var(--tag-4-fg); }
  .tag-5 { background: var(--tag-5-bg); color: var(--tag-5-fg); }
  .tag-6 { background: var(--tag-6-bg); color: var(--tag-6-fg); }
  .tag-7 { background: var(--tag-7-bg); color: var(--tag-7-fg); }
}

/* Where a floating action button sits (LogPicker, Reminders, the object page's "+ Log"): above
   the tab bar on a phone. On a desktop it stays at the content pane's bottom right: `fixed`
   measures from the viewport, so past 1340 px (240 px sidebar + 1100 px main) it would drift into
   the gutter; half the gutter plus the clearance mirrors main's own auto margins, and `max()`
   falls back to the plain clearance once the gutter closes. */
@utility fab-pos {
  position: fixed;
  z-index: 6;
  right: 1rem;
  bottom: calc(1rem + var(--navbar) + env(safe-area-inset-bottom));
  @media (width >= 900px) {
    right: max(1.5rem, calc((100vw - 240px - 1100px) / 2 + 1.5rem));
    bottom: calc(1rem + env(safe-area-inset-bottom));
  }
}
```

- [ ] **Step 5: Delete what moved from `frontend/src/app.css`.**
  - From `:root {`: `--control: 48px;`, `--navbar: 56px;` and their comment, the tag comment and
    the eight `--tag-N-bg/fg` lines. From `:root[data-theme="dark"] {`: the eight tag lines.
  - The `body { … }` rule and its `overscroll-behavior` comment.
  - `input[type='search']::-webkit-search-cancel-button { … }` and its comment.
  - Every rule from `.tag {` to `.tags { … }` (including `button.tag`, `button.tag::before`,
    `.tag.active`, `.tag-0` … `.tag-7`, `.tags`) and the hit-area comment.
  - `.fab { … }`, the `:focus-visible { … }` rule and its comment, and the whole
    `@media (prefers-reduced-motion: reduce)` block with its comment.
  - In `@media (width < 900px)`: the `.app-content { … }` rule, the `main { … }` rule, the `.fab`
    line and their comments; keep only the "No footer on a phone" comment, or delete the now
    empty block with it. In `@media (width >= 900px)` (the shell one): delete the whole block with
    its comments (`.app-content`, `main`, `.fab`).
  - Keep the plain `main { … }` rule and `main.auth` (auth pages and the loading screen still use
    them until Tasks 8 and 9).
  - Check: `grep -n 'tag-\|\.tag\|\.fab\|app-content\|focus-visible\|reduced-motion\|search-cancel' src/app.css`
    prints nothing. The remaining `var(--control)` and `var(--navbar)` references in app.css,
    `SettingsRow` and `FormActions` keep working: the properties now come from app.tw.css.

- [ ] **Step 6: `frontend/src/lib/theme.ts`.** In `applyTheme`, `getPropertyValue('--bg')` →
  `getPropertyValue('--ui-background')`; in its doc comment, "read back from app.css" → "read back
  from app.tw.css's `--ui-background`". In `frontend/index.html`, the comment
  `(app.css --bg)` → `(app.tw.css --ui-background)`.

Run: `npx vitest run tests/tags.test.ts tests/theme.test.ts tests/theme-contrast.test.ts`
Expected: PASS.

- [ ] **Step 7: The floating buttons on `fab-pos`.**
  - `frontend/src/lib/LogPicker.svelte`: in the "+ Log" button's class string, `fab h-12` →
    `fab-pos h-12`.
  - `frontend/src/lib/Reminders.svelte`: replace
    `<button class="primary fab" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</button>`
    with

```svelte
  <Button class="fab-pos h-12 rounded-full px-5 text-base font-semibold shadow-lg" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</Button>
```

  - `frontend/src/routes/ObjectDetail.svelte`: `<div class="fab-row">` → `<div class="fab-pos flex gap-2">`
    and delete the whole `<style>` block at the end of the file (it held only `.fab-row`).

- [ ] **Step 8: e2e.** Create `frontend/tests-e2e/39-screens.spec.ts`:

```ts
import { test, expect, type Page } from '@playwright/test';
import { signIn, signInFresh } from './helpers';

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

test('the floating buttons clear the tab bar on a phone and stay in the pane on a desktop', async ({ page }, info) => {
  await signInFresh(page, '39-fab');
  const id = await object(page, { name: 'Fab probe', type: 'tool' });
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Fab reminder', due_date: '2099-01-01' } })).ok()).toBe(true);
  for (const [path, name] of [['/', /^\+ Log$/], [`/objects/${id}?tab=reminders`, /^\+ New reminder$/]] as const) {
    await page.goto(path);
    const fab = page.getByRole('button', { name });
    await expect(fab).toBeVisible();
    const box = (await fab.boundingBox())!;
    const viewport = page.viewportSize()!;
    if (info.project.name === 'mobile') {
      const nav = (await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).boundingBox())!;
      expect(box.y + box.height, `${path}: above the tab bar`).toBeLessThanOrEqual(nav.y - 8);
      expect(Math.abs(viewport.width - (box.x + box.width) - 16), `${path}: 16 px from the edge`).toBeLessThan(2);
    } else {
      expect(box.x, `${path}: right of the sidebar`).toBeGreaterThan(240);
      expect(Math.abs(viewport.height - (box.y + box.height) - 16), `${path}: 16 px above the bottom`).toBeLessThan(2);
    }
  }
});

test("the browser's bar takes the page background of the theme chosen", async ({ page }) => {
  await signInFresh(page, '39-theme-color');
  await page.goto('/settings/appearance');
  await page.getByLabel('Theme').selectOption('dark');
  await expect(page.locator('meta[name="theme-color"]').first()).toHaveAttribute('content', '#09090b');
  await page.getByLabel('Theme').selectOption('light');
  await expect(page.locator('meta[name="theme-color"]').first()).toHaveAttribute('content', '#fafafa');
});
```

  In `frontend/tests-e2e/13-shell.spec.ts` ("on a wide desktop viewport, the FAB stays anchored…"),
  replace the `const clearance = await page.evaluate(…--space-5…);` statement and its comment with:

```ts
  // The FAB's clearance from the pane's edge on a desktop: 1.5rem (`fab-pos` in app.tw.css).
  const clearance = 24;
```

  and in that test's comment "see the comment on `.fab` in app.css" → "see `fab-pos` in app.tw.css".
  (Locator-only change: the token it read is gone.)

- [ ] **Step 9: Verify, look, commit**

Run: `cd frontend && npm run check && npm test && npx playwright test 39-screens 13-shell 23-tags 36-dashboard 37-object-detail 15-reading-reminders`
Expected: PASS, both projects.
Capture `../shots/r5-t1 dashboard,object,new,edit` and compare with `shots/r5-before`: identical
apart from anti-aliasing (tags, floating buttons, page padding unchanged).

```bash
git add -A frontend
git commit -m "refactor: shared tokens, tag chips, shell and floating-button position move to app.tw.css"
```

---
### Task 2: Search hits in the dashboard's card style

**Files:**
- Modify: `frontend/src/routes/Search.svelte`, `frontend/tests-e2e/23-tags.spec.ts`,
  `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Consumes: `controlClass`, `sectionHeadingClass` (`field/classes.ts`), `Button`, `CategoryIcon`,
  `Icon`, `TagChips`.
- Produces: `data-testid="search-hit"` on each hit card, `data-testid="hit-icon"` on its tile. The
  opening button's accessible name is the hit's title alone.

- [ ] **Step 1: Failing e2e test.** Append to `39-screens.spec.ts`:

```ts
test('search hits are cards like the dashboard: icon tile, name, facts, tags inside', async ({ page }) => {
  await signInFresh(page, '39-search');
  const id = await object(page, { name: 'Kartenrad', type: 'bike', tags: ['Kartentag'] });
  expect((await page.request.post(`/api/objects/${id}/activities`, {
    data: { date: '2026-03-01', category: 'repair', title: 'Kartenschlauch', notes: '', cost_cents: 1250, tags: ['Kartentag'] },
  })).ok()).toBe(true);

  await page.goto('/search?q=Karten');
  const hits = page.getByTestId('search-hit');
  await expect(hits).toHaveCount(2);
  for (const title of ['Kartenrad', 'Kartenschlauch']) {
    const hit = hits.filter({ hasText: title });
    await expect(hit.getByTestId('hit-icon')).toBeVisible();
    await expect(hit.locator('.tag', { hasText: 'Kartentag' })).toBeVisible();
  }
  // The whole card opens the entry, not only its title.
  const box = (await hits.filter({ hasText: 'Kartenschlauch' }).boundingBox())!;
  await page.mouse.click(box.x + box.width - 12, box.y + 12);
  await expect(page.getByLabel('Title')).toHaveValue('Kartenschlauch');
});
```

Run: `npx playwright test 39-screens -g "search hits"` — FAIL (no `search-hit`).

- [ ] **Step 2: `Search.svelte` script.** Add the imports

```ts
  import SearchIcon from '@lucide/svelte/icons/search';
  import { Button } from '$lib/components/ui/button/index.js';
  import { controlClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import CategoryIcon from '../lib/CategoryIcon.svelte';
```

  and at the end of the script:

```ts
  // The dashboard's object card (ObjectCard.svelte): one box, an icon tile, the name as the
  // button that opens it, stretched over the whole card; tag chips are raised above it.
  const card = 'relative isolate flex gap-3 rounded-lg border border-border bg-card p-3 shadow-xs transition-colors hover:border-input';
  const tile = 'grid size-12 shrink-0 place-items-center rounded-md bg-primary/10 text-brand-ink';
  const open = "min-w-0 cursor-pointer line-clamp-2 break-words text-left text-base font-semibold text-foreground after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
  const facts = 'm-0 text-sm text-muted-foreground tabular-nums';
```

- [ ] **Step 3: `Search.svelte` markup.** Replace everything from `<main>` to the end of the file:

```svelte
<main>
  <TopBar title={$t('search.title')} backTo="/" />

  <div class="relative mb-4">
    <SearchIcon aria-hidden="true" class="pointer-events-none absolute top-1/2 left-3 size-5 -translate-y-1/2 text-muted-foreground" />
    <!-- svelte-ignore a11y_autofocus -->
    <input type="search" data-slot="search" autofocus aria-label={$t('search.placeholder')} placeholder={$t('search.placeholder')}
           bind:value={q} class={`${controlClass} appearance-none pl-10`} />
  </div>

  {#if error}<p role="alert" class="m-0 mb-3 text-sm font-medium text-destructive">{error}</p>{/if}
  {#if loading && !results}
    <p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p>
  {:else if empty}
    <!-- A report about a query, not an invitation: it names what was searched for, and there is
         no action to offer. Before anything is typed nothing is drawn at all. -->
    <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('search.none', { q: searched })}</p>
    </div>
  {:else if results}
    {#if results.objects.length > 0}
      <section aria-labelledby="search-objects" class="mb-6 flex flex-col gap-2">
        <h2 id="search-objects" class={sectionHeadingClass}>{$t('search.objects')}</h2>
        <ul role="list" class="m-0 grid list-none grid-cols-1 gap-3 p-0 wide:grid-cols-2">
          {#each results.objects as o (o.id)}
            <li data-testid="search-hit" class={card}>
              <span data-testid="hit-icon" class={tile} aria-hidden="true"><Icon name={typeIcon(o.type, $customTypes)} size={22} /></span>
              <div class="flex min-w-0 flex-1 flex-col gap-1">
                <button data-slot="hit-open" class={open} onclick={() => go(`/objects/${o.id}`)}>{o.name}</button>
                <p class={facts}>
                  {typeLabel(o.type, $customTypes, $t, $typesLoaded)}{o.parent_name ? ` · ${$t('search.in-parent', { name: o.parent_name })}` : ''}{o.archived_at ? ` · ${$t('search.archived')}` : ''}
                </p>
                {#if (o.tags ?? []).length > 0}
                  <!-- Plain labels: an object's own tag is rarely on its entries, so a tap would
                       open an empty timeline under a "Show entries tagged" label. -->
                  <div class="relative z-10 mt-1 w-fit"><TagChips tags={o.tags} /></div>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if results.activities.length > 0}
      <section aria-labelledby="search-activities" class="mb-6 flex flex-col gap-2">
        <h2 id="search-activities" class={sectionHeadingClass}>{$t('search.activities')}</h2>
        <ul role="list" class="m-0 grid list-none grid-cols-1 gap-3 p-0 wide:grid-cols-2">
          {#each results.activities as a (a.id)}
            <li data-testid="search-hit" class={card}>
              <span data-testid="hit-icon" class={tile} aria-hidden="true"><CategoryIcon category={a.category} size={22} /></span>
              <div class="flex min-w-0 flex-1 flex-col gap-1">
                <button data-slot="hit-open" class={open} onclick={() => go(`/objects/${a.object_id}/activities/${a.id}`)}>{activityTitle(a.title, a.category, $t)}</button>
                <p class={facts}>
                  {a.object_name} · {fmtDate(a.date, $dateFormat)}{a.cost_cents !== null ? ` · ${money(a.cost_cents, $currency, $locale)}` : ''}{a.weight_grams !== null ? ` · ${a.weight_grams} g` : ''}{placesLabel(a.from_place, a.to_place) ? ` · ${placesLabel(a.from_place, a.to_place)}` : ''}
                </p>
                {#if (a.tags ?? []).length > 0}
                  <!-- A tapped chip opens the entry's object with its timeline narrowed to that tag. -->
                  <div class="relative z-10 mt-1 w-fit">
                    <TagChips navigates tags={a.tags} onselect={(tag) => go(`/objects/${a.object_id}?tag=${encodeURIComponent(tag)}`)} />
                  </div>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if results.has_more}
      <Button variant="outline" class="min-h-11 w-full" disabled={loadingMore} onclick={loadMore}>{loadingMore ? $t('nav.loading') : $t('search.more')}</Button>
    {/if}
  {/if}
</main>
```

  Delete the `<style>` block.

- [ ] **Step 4: Move `23-tags` off `.hit-row`** (locator only). In "search shows tags on hits…",
  every `page.locator('.hit-row', { hasText: X })` → `page.getByTestId('search-hit').filter({ hasText: X })`.

- [ ] **Step 5: Verify, look, commit**

Run: `npm run check && npx playwright test 39-screens 03-search 11-controls 23-tags 24-own-types`
Expected: PASS, both projects (`03-search`'s `getByRole('button', { name: /Saab/ })` and
`/Cambelt/` still match the title buttons; `11-controls` still sees a rounded 48 px search box with
`appearance: none` and no ✕).
Capture `../shots/r5-t2 search`; compare `12-search` with `r5-before`: cards with tiles, section
labels small and uppercase, tags inside the cards.

```bash
git add -A frontend
git commit -m "feat: search hits in the dashboard's card style"
```

---
### Task 3: Statistics — summary cards first, charts without empty months, resources last

**Files:**
- Modify: `frontend/src/lib/stats.ts`, `frontend/src/lib/BarList.svelte`,
  `frontend/src/routes/Stats.svelte`, `frontend/src/i18n/en.ts`, `de.ts`,
  `frontend/tests/stats.test.ts`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests-e2e/20-statistics.spec.ts`, `frontend/tests-e2e/24-own-types.spec.ts`,
  `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Produces, in `lib/stats.ts`:
  - `monthLabel(bucket: string, locale: string): string` — "Sep 2026" (a year bucket stays "2026").
  - `type YearSummary = { year: string; spent: number; previous: { year: string; cents: number; through: number | null }; changePct: number | null; top: { id: number; name: string; cents: number } | null }`
  - `yearSummary(focus: Stats, before: Stats, year: string, today: string): YearSummary`
- `BarList` props: `items: Bar[]`, `labelClass?: string` (default `'w-24'`); each row
  `data-testid="bar-row"`.
- Test ids: `stats-summary`, `stats-spent`, `stats-change`, `stats-top` (plus the existing
  `stats-total`, `stats-over-time`, `stats-by-object`, `stats-by-type`, `stats-by-category`,
  `stats-energy`, `stats-fuel`, `stats-water`).

- [ ] **Step 1: Failing unit tests.** Append to `frontend/tests/stats.test.ts` (add `monthLabel`,
  `yearSummary` to its import from `../src/lib/stats`, and `import type { Stats } from '../src/lib/types';`):

```ts
describe('monthLabel', () => {
  it('names a month with its year, and leaves a year alone', () => {
    expect(monthLabel('2026-03', 'en')).toBe('Mar 2026');
    expect(monthLabel('2026', 'en')).toBe('2026');
  });
});

describe('yearSummary', () => {
  const stats = (total: number, months: Array<[string, number]>, roots: Array<[number, string, number]> = []): Stats => ({
    total_cents: total,
    years: [],
    over_time: months.map(([bucket, cost_cents]) => ({ bucket, cost_cents })),
    by_object: roots.map(([id, name, cost_cents]) => ({ id, name, type: 'car', archived: false, cost_cents, children: [] })),
    by_type: [],
    by_category: [],
  });

  it('compares a past year with the whole year before it', () => {
    const s = yearSummary(stats(75_000, []), stats(100_000, [['2024-03', 40_000], ['2024-11', 60_000]]), '2025', '2026-09-15');
    expect(s.previous).toEqual({ year: '2024', cents: 100_000, through: null });
    expect(s.changePct).toBe(-25);
  });

  it('compares the running year with the same months of the year before', () => {
    const before = stats(1_000_000, [['2025-03', 100_000], ['2025-09', 50_000], ['2025-11', 850_000]]);
    const s = yearSummary(stats(300_000, []), before, '2026', '2026-09-15');
    expect(s.previous).toEqual({ year: '2025', cents: 150_000, through: 9 });
    expect(s.changePct).toBe(100);
  });

  it('gives no percentage when the year before spent nothing', () => {
    expect(yearSummary(stats(5_000, []), stats(0, []), '2026', '2026-01-02').changePct).toBeNull();
  });

  it('names the object that cost most, counting what is inside it', () => {
    const s = yearSummary(stats(900, [], [[1, 'House', 600], [2, 'Car', 300]]), stats(0, []), '2026', '2026-05-01');
    expect(s.top).toEqual({ id: 1, name: 'House', cents: 600 });
  });

  it('has no top object in a year without spend', () => {
    expect(yearSummary(stats(0, []), stats(0, []), '2026', '2026-05-01').top).toBeNull();
  });
});
```

Run: `cd frontend && npx vitest run tests/stats.test.ts` — FAIL (not exported).

- [ ] **Step 2: Implement in `frontend/src/lib/stats.ts`.** Change the type import to
  `import type { Stats, StatsObject } from './types';` and append:

```ts
/** A bar's full name: "Sep 2026". A year bucket stays as it is. */
export function monthLabel(bucket: string, locale: string): string {
  if (bucket.length === 4) return bucket;
  const [y, m] = bucket.split('-').map(Number);
  return dateTimeFormat(locale, { month: 'short', year: 'numeric' }).format(new Date(Date.UTC(y, m - 1, 15)));
}

export interface YearSummary {
  year: string;
  spent: number;
  /** What `spent` is compared with: the year before, over the same months (`through` = the last
   *  month counted) while `year` is still running, or all of it once `year` is over. */
  previous: { year: string; cents: number; through: number | null };
  /** Whole percent, or null when the year before has nothing to compare with. */
  changePct: number | null;
  /** The root object with the most spend, its contents included. */
  top: { id: number; name: string; cents: number } | null;
}

/**
 * The statistics page's three cards, from two `/stats?year=` answers. The running year is
 * compared with the same months of the year before -- September against a whole previous year
 * would always read as a drop.
 */
export function yearSummary(focus: Stats, before: Stats, year: string, today: string): YearSummary {
  const through = today.slice(0, 4) === year ? Number(today.slice(5, 7)) : null;
  const cents = through === null
    ? before.total_cents
    : before.over_time.filter((a) => Number(a.bucket.slice(5, 7)) <= through).reduce((sum, a) => sum + a.cost_cents, 0);
  const spent = focus.total_cents;
  const top = focus.by_object.reduce<StatsObject | null>((best, o) => (o.cost_cents > (best?.cost_cents ?? 0) ? o : best), null);
  return {
    year,
    spent,
    previous: { year: String(Number(year) - 1).padStart(4, '0'), cents, through },
    changePct: cents > 0 ? Math.round(((spent - cents) / cents) * 100) : null,
    top: top && { id: top.id, name: top.name, cents: top.cost_cents },
  };
}
```

Run: `npx vitest run tests/stats.test.ts` — PASS.

- [ ] **Step 3: `BarList.svelte`.** Keep the `<script lang="ts" module>` block (the `Bar`
  interface) unchanged; replace the rest of the file with:

```svelte
<script lang="ts">
  import Icon from './Icon.svelte';

  /** `labelClass` sets the name column's width: object names on Statistics need more than the
   *  categories and years Insights lists. */
  let { items, labelClass = 'w-24' }: { items: Bar[]; labelClass?: string } = $props();

  /** Bar width as a share of the largest value, so the widest bar always fills its track. Values
   *  are never negative: the API rejects negative costs at the boundary. */
  const max = $derived(Math.max(...items.map((b) => b.value), 1));

  /** Once any row can expand, every row reserves the toggle's width so names line up. Insights
   *  never sets `expanded`, so its layout has no toggle column. */
  const anyToggle = $derived(items.some((b) => b.expanded !== undefined));
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring';
</script>

<ul role="list" class="m-0 flex list-none flex-col p-0">
  {#each items as b (b.key)}
    <li data-testid="bar-row" class="flex min-h-11 items-center gap-2" style={b.depth ? `padding-left: ${b.depth}rem` : undefined}>
      <span class={`flex min-w-0 shrink-0 items-center gap-1 text-sm ${labelClass}`}>
        {#if b.expanded !== undefined}
          <button type="button" data-slot="bar-toggle" aria-expanded={b.expanded} aria-label={b.toggleLabel} onclick={b.onToggle}
                  class={`grid size-11 shrink-0 cursor-pointer place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground ${focus}`}>
            <span class={['inline-flex transition-transform', b.expanded && 'rotate-90']}><Icon name="chevron" size={14} /></span>
          </button>
        {:else if anyToggle}
          <span class="size-11 shrink-0" aria-hidden="true"></span>
        {/if}
        {#if b.onLabel}
          <button type="button" data-slot="bar-label" onclick={b.onLabel}
                  class={`min-h-11 min-w-0 cursor-pointer truncate text-left text-foreground hover:underline ${focus}`}>{b.label}</button>
        {:else}
          <span class="min-w-0 truncate text-foreground">{b.label}</span>
        {/if}
        {#if b.note}<span class="shrink-0 text-xs text-muted-foreground">{b.note}</span>{/if}
      </span>
      <!-- brand-ink, not the amber fill: a bar is a graphic that carries meaning (1.4.11). -->
      <span class="h-2.5 min-w-8 flex-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
        <span class="block h-full rounded-full bg-brand-ink" style={`width:${Math.round((b.value / max) * 100)}%`}></span>
      </span>
      <span class="shrink-0 text-sm text-foreground tabular-nums">{b.display}</span>
    </li>
  {/each}
</ul>
```

  The file has no `<style>` block afterwards.

- [ ] **Step 4: Contrast.** In `theme-contrast.test.ts`, in `describe('shadcn token contrast')`'s
  3:1 list, add `['brand-ink', 'muted']` with the comment
  `// BarList's fill on its track (Statistics, Insights).`

- [ ] **Step 5: i18n.** Append to `en.ts`:

```ts
  'stats.summary': 'Summary for {year}',
  'stats.spent-in': 'Spent in {year}',
  'stats.change': 'Change',
  'stats.vs': 'vs {period}',
  'stats.vs-none': 'Nothing spent in {year} to compare with',
  'stats.top-object': 'Top object',
```

  and to `de.ts`:

```ts
  'stats.summary': 'Übersicht {year}',
  'stats.spent-in': 'Ausgaben {year}',
  'stats.change': 'Veränderung',
  'stats.vs': 'gegenüber {period}',
  'stats.vs-none': 'Keine Ausgaben {year} zum Vergleich',
  'stats.top-object': 'Teuerstes Objekt',
```

- [ ] **Step 6: Failing e2e test.** Append to `39-screens.spec.ts`:

```ts
test('statistics lead with the year: spent, change against the same months last year, top object', async ({ page }) => {
  await page.clock.setFixedTime(new Date('2026-09-15T12:00:00'));
  await signInFresh(page, '39-stats');
  const car = await object(page, { name: 'Summary Car', type: 'car' });
  const house = await object(page, { name: 'Summary House', type: 'home' });
  const cost = async (id: number, date: string, cost_cents: number) => {
    expect((await page.request.post(`/api/objects/${id}/activities`, { data: { date, category: 'repair', title: 'Work', notes: '', cost_cents } })).ok()).toBe(true);
  };
  await cost(car, '2025-03-10', 100_000);
  await cost(house, '2025-11-10', 900_000); // after September: outside the comparison
  await cost(car, '2026-02-01', 50_000);
  await cost(house, '2026-07-01', 25_000);

  await page.goto('/stats');
  const summary = page.getByTestId('stats-summary');
  await expect(summary.getByTestId('stats-spent')).toContainText('Spent in 2026');
  await expect(summary.getByTestId('stats-spent')).toContainText('750.00');
  await expect(summary.getByTestId('stats-change')).toContainText('-25%');
  await expect(summary.getByTestId('stats-change')).toContainText('vs Jan–Sep 2025');
  await expect(summary.getByTestId('stats-top')).toContainText('Summary Car');

  // Summary first, then the money.
  const order = await page.locator('main [data-testid]').evaluateAll((els) => els.map((e) => e.getAttribute('data-testid')));
  expect(order.indexOf('stats-summary')).toBeLessThan(order.indexOf('stats-over-time'));

  // A past year compares with the whole year before, and the top card opens its object.
  await page.getByLabel('Year').selectOption('2025');
  await expect(summary.getByTestId('stats-spent')).toContainText('Spent in 2025');
  await expect(summary.getByTestId('stats-change')).toContainText('Nothing spent in 2024');
  await summary.getByRole('button', { name: 'Summary House' }).click();
  await page.waitForURL(`**/objects/${house}`);
});
```

Run: `npx playwright test 39-screens -g "statistics lead"` — FAIL (no `stats-summary`).

- [ ] **Step 7: `Stats.svelte` script.** Keep `includePurchases`, `yearFromUrl`, `year`, the URL
  `$effect`, `data`/`energy`/`fuel`/`water`/`years`/`error`/`expanded`, the selection `$effect`,
  `onMount`, `yearOptions`, `toggle`, the `fmt*` helpers, `share`, `bars`, `objectBars` and the
  `has*` deriveds unchanged. Change the imports to:

```ts
  import { onMount } from 'svelte';
  import { numberFormat, dateTimeFormat } from '../lib/intl-cache';
  import { errorMessage } from '../lib/api-error';
  import TopBar from '../lib/TopBar.svelte';
  import BarList, { type Bar } from '../lib/BarList.svelte';
  import Chart from '../lib/Chart.svelte';
  import type { ChartBar } from '../lib/chart';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { money, todayIso } from '../lib/format';
  import { currency } from '../stores/session';
  import { persisted } from '../stores/persisted';
  import { locale, t } from '../i18n';
  import { PURCHASE_PRICE, flattenTree, monthLabel, periodLabel, sharePct, statsPath, yearSummary, type YearSummary } from '../lib/stats';
  import type { Amount, EnergyUsage, FuelUsage, WaterUsage, Stats } from '../lib/types';
  import { customTypes, typeLabel, typesLoaded } from '../lib/type-registry';
```

  and append to the script:

```ts
  // The summary is one year: the chosen one, or this year under "All years". The charts below
  // keep describing the selection. With a year chosen, the selection's own answer is that year.
  const today = todayIso();
  const focusYear = $derived(year ?? today.slice(0, 4));
  const beforeYear = $derived(String(Number(focusYear) - 1).padStart(4, '0'));
  let focusData = $state<Stats | null>(null);
  let beforeData = $state<Stats | null>(null);
  $effect(() => {
    const purchases = $includePurchases;
    const own = year === null;
    const fy = focusYear;
    const by = beforeYear;
    focusData = null;
    beforeData = null;
    let current = true;
    const load = (y: string) => api<Stats>('GET', statsPath(y, purchases));
    Promise.all([own ? load(fy) : Promise.resolve(null), load(by)])
      .then(([f, b]) => { if (current) { focusData = f; beforeData = b; } })
      .catch(() => { /* no summary; the charts below report their own failure */ });
    return () => { current = false; };
  });
  const summary = $derived.by<YearSummary | null>(() => {
    const focus = year === null ? focusData : data;
    return focus && beforeData ? yearSummary(focus, beforeData, focusYear, today) : null;
  });

  const pct = (n: number) => numberFormat($locale, { style: 'percent', signDisplay: 'exceptZero', maximumFractionDigits: 0 }).format(n / 100);
  const signedMoney = (cents: number) => `${cents > 0 ? '+' : ''}${fmt(cents)}`;
  const monthName = (m: number) => dateTimeFormat($locale, { month: 'short' }).format(new Date(Date.UTC(2000, m - 1, 15)));
  /** "Jan–Sep 2025" while the year runs, "2025" once it is over. */
  const period = (s: YearSummary) =>
    s.previous.through === null ? s.previous.year
      : s.previous.through === 1 ? `${monthName(1)} ${s.previous.year}`
      : `${monthName(1)}–${monthName(s.previous.through)} ${s.previous.year}`;
  /** One bar of a series over time. */
  const bar = (bucket: string, value: number, display: string): ChartBar =>
    ({ key: bucket, label: monthLabel(bucket, $locale), tick: periodLabel(bucket, $locale), value, display });

  const panel = 'flex min-w-0 flex-col gap-3 rounded-lg border border-border bg-card p-4 shadow-xs';
  const heading = 'm-0 text-base font-semibold text-foreground';
  const figure = 'flex min-w-0 flex-col gap-1 rounded-lg border border-border bg-card p-4 shadow-xs';
  const figureValue = 'm-0 text-2xl font-semibold tracking-tight text-foreground tabular-nums';
  const line = 'm-0 text-sm text-muted-foreground tabular-nums';
  const strong = 'font-semibold text-foreground';
  const stretched = "min-w-0 cursor-pointer truncate text-left font-semibold text-foreground after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
  const open = `${stretched} text-lg`;
  const openLevel = `${stretched} text-base`;
```

- [ ] **Step 8: `Stats.svelte` markup.** Replace everything from `<main>` to the end of the file:

```svelte
<main>
  <TopBar title={$t('stats.title')} icon="chart" />

  <div class="mb-4 flex flex-wrap items-end gap-x-6 gap-y-2">
    <Field id="stats-year" label={$t('stats.year')} class="w-full sm:w-48">
      <NativeSelect bind:value={() => year ?? '', (v) => (year = v || null)}>
        <option value="">{$t('stats.all-years')}</option>
        {#each yearOptions as y (y)}<option value={y}>{y}</option>{/each}
      </NativeSelect>
    </Field>
    <CheckField id="stats-purchases" label={$t('stats.purchases')} bind:checked={() => $includePurchases, (on) => includePurchases.set(on)} />
  </div>

  {#if summary && (summary.spent > 0 || summary.previous.cents > 0)}
    {@const s = summary}
    <section data-testid="stats-summary" aria-label={$t('stats.summary', { year: s.year })} class="mb-6 grid grid-cols-1 gap-3 sm:grid-cols-3">
      <div data-testid="stats-spent" class={figure}>
        <p class={sectionHeadingClass}>{$t('stats.spent-in', { year: s.year })}</p>
        <p class={figureValue}>{fmt(s.spent)}</p>
      </div>
      <div data-testid="stats-change" class={figure}>
        <p class={sectionHeadingClass}>{$t('stats.change')}</p>
        {#if s.changePct !== null}
          <p class={figureValue}>{pct(s.changePct)}</p>
          <p class={line}>{$t('stats.vs', { period: period(s) })} · {signedMoney(s.spent - s.previous.cents)}</p>
        {:else}
          <p class={figureValue} aria-hidden="true">–</p>
          <p class={line}>{$t('stats.vs-none', { year: s.previous.year })}</p>
        {/if}
      </div>
      <div data-testid="stats-top" class={`relative isolate ${figure}`}>
        <p class={sectionHeadingClass}>{$t('stats.top-object')}</p>
        {#if s.top}
          {@const top = s.top}
          <button data-slot="stats-top-open" class={open} onclick={() => go(`/objects/${top.id}`)}>{top.name}</button>
          <p class={line}>{fmt(top.cents)} · {sharePct(top.cents, s.spent)}%</p>
        {:else}
          <p class={figureValue} aria-hidden="true">–</p>
        {/if}
      </div>
    </section>
  {/if}

  {#if error}
    <p role="alert" class="m-0 mb-4 text-sm font-medium text-destructive">{error}</p>
  {:else if !data}
    <p class="m-0 mb-4 text-sm text-muted-foreground">{$t('nav.loading')}</p>
  {:else if data.total_cents === 0}
    <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('stats.none')}</p>
    </div>
  {:else}
    <div class="mb-6 grid grid-cols-1 gap-4 wide:grid-cols-2">
      <section data-testid="stats-over-time" class={`${panel} wide:col-span-2`}>
        <div class="flex flex-wrap items-baseline justify-between gap-x-3">
          <h2 class={heading}>{$t('stats.over-time')}</h2>
          <p data-testid="stats-total" class="m-0 text-sm text-muted-foreground">{$t('stats.total')}: <b class={`${strong} tabular-nums`}>{fmt(data.total_cents)}</b></p>
        </div>
        <!-- Empty months (or years) are left out; each bar keeps its own label, so a gap shows. -->
        <Chart label={$t('stats.over-time')} items={data.over_time.map((a) => bar(a.bucket, a.cost_cents, fmt(a.cost_cents)))} />
      </section>
      <section data-testid="stats-by-object" class={`${panel} wide:col-span-2`}>
        <h2 class={heading}>{$t('stats.by-object')}</h2>
        <BarList items={objectBars} labelClass="w-44 wide:w-64" />
      </section>
      <section data-testid="stats-by-type" class={panel}>
        <h2 class={heading}>{$t('stats.by-type')}</h2>
        <BarList items={bars(data.by_type, (b) => typeLabel(b, $customTypes, $t, $typesLoaded))} labelClass="w-28" />
      </section>
      <section data-testid="stats-by-category" class={panel}>
        <h2 class={heading}>{$t('stats.by-category')}</h2>
        <BarList items={bars(data.by_category, (b) => (b === PURCHASE_PRICE ? $t('stats.purchase-price') : $t(`cat.${b}`)))} labelClass="w-28" />
      </section>
    </div>
  {/if}

  <!-- Resources after money (round 0 fixed what the fuel section counts). They take neither the
       year nor the purchases switch: always the last twelve months. -->
  <div class="grid grid-cols-1 gap-4 wide:grid-cols-2">
    {#if energy && hasKwh}
      <section data-testid="stats-energy" class={panel}>
        <h2 class={heading}>{$t('stats.energy-title')}</h2>
        <p class={line}>
          {$t('stats.energy-current')}: <b class={strong}>{fmtKwh(energy.current_kwh_milli)}</b> · {$t('stats.energy-previous')}: {fmtKwh(energy.previous_kwh_milli)}{#if energy.target_kwh_milli > 0} · {$t('stats.target')}: {fmtKwh(energy.target_kwh_milli)}{/if}
        </p>
        <Chart label={$t('stats.energy-title')} items={energy.months.map((m) => bar(m.month, m.kwh_milli, fmtKwh(m.kwh_milli)))} />
      </section>
    {/if}
    {#if fuel && hasFuel}
      <section data-testid="stats-fuel" class={panel}>
        <h2 class={heading}>{$t('stats.fuel-title')}</h2>
        {#if hasLiters || hasGallons}
          <p class={line}>
            {$t('stats.energy-current')}: <b class={strong}>{[hasLiters ? fmtLiters(fuel.current_liters_milli) : '', hasGallons ? fmtGallons(fuel.current_gallons_milli) : ''].filter(Boolean).join(' · ')}</b>
            · {$t('stats.energy-previous')}: {[hasLiters ? fmtLiters(fuel.previous_liters_milli) : '', hasGallons ? fmtGallons(fuel.previous_gallons_milli) : ''].filter(Boolean).join(' · ')}
          </p>
        {/if}
        {#if fuel.levels.length > 0}
          <h3 class={sectionHeadingClass}>{$t('stats.fuel-levels')}</h3>
          <ul role="list" class="m-0 grid list-none grid-cols-1 gap-2 p-0 sm:grid-cols-2">
            {#each fuel.levels as level (level.object_id)}
              <li class="relative isolate flex flex-col gap-1 rounded-lg border border-border bg-background p-3">
                <div class="flex items-baseline justify-between gap-2">
                  <button data-slot="level-open" class={openLevel} onclick={() => go(`/objects/${level.object_id}`)}>{level.object_name}</button>
                  <b class="shrink-0 font-semibold text-foreground tabular-nums">{level.level_pct}%{#if level.remaining_milli !== null} · {level.unit === 'l' ? fmtLiters(level.remaining_milli) : fmtGallons(level.remaining_milli)}{/if}</b>
                </div>
                <p class={line}>
                  {periodLabel(level.date, $locale)}{#if level.estimated_days_remaining !== null} · {$t('stats.fuel-days', { n: level.estimated_days_remaining })}{/if}{#if level.low} · <span class="font-medium text-warn">{$t('stats.fuel-low')}</span>{/if}
                </p>
              </li>
            {/each}
          </ul>
        {/if}
        {#if hasLiters}
          <h3 class={sectionHeadingClass}>{$t('stats.fuel-liters')}</h3>
          <Chart label={$t('stats.fuel-liters')} items={fuel.months.map((m) => bar(m.month, m.liters_milli, fmtLiters(m.liters_milli)))} />
        {/if}
        {#if hasGallons}
          <h3 class={sectionHeadingClass}>{$t('stats.fuel-gallons')}</h3>
          <Chart label={$t('stats.fuel-gallons')} items={fuel.months.map((m) => bar(m.month, m.gallons_milli, fmtGallons(m.gallons_milli)))} />
        {/if}
      </section>
    {/if}
    {#if water && hasWater}
      <section data-testid="stats-water" class={panel}>
        <h2 class={heading}>{$t('stats.water-title')}</h2>
        <p class={line}>
          {$t('stats.energy-current')}: <b class={strong}>{fmtWater(water.current_liters_milli)}</b> · {$t('stats.energy-previous')}: {fmtWater(water.previous_liters_milli)} · {$t('water.daily-average')}: {fmtWater(water.daily_average_liters_milli)}{#if water.current_cost_cents > 0} · {$t('activity.cost')}: {fmt(water.current_cost_cents)}{/if}
        </p>
        {#if water.anomalies > 0}<p class="m-0 text-sm text-warn">{$t('water.anomalies', { n: water.anomalies })}</p>{/if}
        <Chart label={$t('stats.water-title')} items={water.months.map((m) => bar(m.month, m.liters_milli, `${fmtWater(m.liters_milli)}${m.estimated ? ` · ${$t('water.estimated-short')}` : ''}`))} />
        {#if water.objects.length > 1}
          <h3 class={sectionHeadingClass}>{$t('stats.by-object')}</h3>
          <BarList labelClass="w-36" items={water.objects.map((o) => ({ key: o.object_id, label: o.object_name, value: o.liters_milli, display: o.target_liters_milli ? `${fmtWater(o.liters_milli)} / ${fmtWater(o.target_liters_milli)}` : fmtWater(o.liters_milli) }))} />
        {/if}
      </section>
    {/if}
  </div>
</main>
```

  Delete the `<style>` block. `Amount` stays imported (used by `bars`).

- [ ] **Step 9: Move the specs.**
  - `20-statistics.spec.ts` (behaviour change, empty months hidden):
    `await expect(page.getByTestId('stats-over-time').locator('.bar-row')).toHaveCount(12);` →

```ts
  // Only the months with spend are drawn: February, June and July.
  await expect(page.getByTestId('stats-over-time').getByTestId('chart-bar')).toHaveCount(3);
```

  - `24-own-types.spec.ts` (locator only): `byType.locator('.bar-row', { hasText: 'E-Scooter' })` →
    `byType.getByTestId('bar-row').filter({ hasText: 'E-Scooter' })`.

- [ ] **Step 10: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 39-screens 20-statistics 24-own-types 31-water 21-object-cost-depth 25-offline-cache`
Expected: PASS, both projects (`31-water`'s "2.5 m³" is in the section's figures and the chart's
table; `25-offline-cache` still finds the `Stats-*` chunk and a styled heading; `21` sees Insights'
BarList in the new look).
Capture `../shots/r5-t3 stats,info`; compare `13-stats` with `r5-before` (summary cards, charts
with only months that have spend, fuel/water after money) and `07-object-info` (Insights' bars).

```bash
git add -A frontend
git commit -m "feat: statistics lead with a year summary; series are charts without empty months"
```

---
### Task 4: The settings hub, and the patterns every settings page shares

**Files:**
- Create: `frontend/src/lib/autosave.ts`, `frontend/src/lib/toast.ts`,
  `frontend/src/lib/Toaster.svelte`, `frontend/tests/autosave.test.ts`, `frontend/tests/toast.test.ts`
- Modify: `frontend/src/lib/settings-rows.ts`, `frontend/src/lib/SettingsRow.svelte`,
  `frontend/src/lib/SignedIn.svelte`, `frontend/src/lib/AccountMenu.svelte`,
  `frontend/src/routes/Settings.svelte`, `frontend/tests/settings-rows.test.ts`,
  `frontend/tests/theme-contrast.test.ts`, `frontend/tests-e2e/13-shell.spec.ts`,
  `frontend/tests-e2e/14-settings.spec.ts`, `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Produces:
  - `autosave<T>(save: (value: T) => Promise<void>, opts?: { delay?: number; onsaved?: () => void; onerror?: (e: unknown) => void }): { push(value: T): void; flush(): Promise<void> }`
  - `toast(text: string, ms?: number): void` and the store `toastMessage` in `lib/toast.ts`.
  - `Toaster.svelte`: no props; `role="status"`, `data-testid="toast"`.
  - `type SettingsIcon = 'palette' | 'user' | 'bell' | 'shapes' | 'key' | 'archive' | 'users' | 'database'`;
    `SettingsRowModel.icon: SettingsIcon`.
  - `SignedIn.svelte` props `compact?: boolean`, `framed?: boolean` (default true);
    `data-testid="signed-in"`.
  - Hub test ids: `failed-write` (a failed save's row), `about` (the About list). Lists named by
    their group heading (`getByRole('list', { name: 'You' })`).
- Consumed by: Tasks 5–7 (every settings page mounts `<Toaster />` where it saves or reports).

- [ ] **Step 1: Failing unit tests.**
  - `frontend/tests/autosave.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { autosave } from '../src/lib/autosave';

describe('autosave', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('waits for a pause and sends only the newest value', async () => {
    const save = vi.fn(async (_v: number) => {});
    const onsaved = vi.fn();
    const s = autosave(save, { delay: 300, onsaved });
    s.push(1); s.push(2); s.push(3);
    await vi.advanceTimersByTimeAsync(299);
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(save.mock.calls).toEqual([[3]]);
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('sends one request at a time, then the newest value that arrived meanwhile', async () => {
    let release!: () => void;
    const save = vi.fn((v: number) => (v === 1 ? new Promise<void>((r) => { release = r; }) : Promise.resolve()));
    const onsaved = vi.fn();
    const s = autosave(save, { delay: 0, onsaved });
    s.push(1);
    await vi.advanceTimersByTimeAsync(0);
    s.push(2); s.push(3);
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([[1]]);
    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([[1], [3]]);
    // "Saved" once, for the value that is now on the server -- not for the one it replaced.
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('reports the failure of the newest value', async () => {
    const onerror = vi.fn();
    const s = autosave(async () => { throw new Error('refused'); }, { delay: 0, onerror });
    s.push(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(onerror).toHaveBeenCalledWith(expect.any(Error));
  });

  it('sends a waiting value at once when flushed (leaving the page)', async () => {
    const save = vi.fn(async (_v: string) => {});
    const s = autosave(save, { delay: 10_000 });
    s.push('x');
    await s.flush();
    expect(save.mock.calls).toEqual([['x']]);
  });
});
```

  - `frontend/tests/toast.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { toast, toastMessage } from '../src/lib/toast';

describe('toast', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('shows one message and clears it after its time', () => {
    toast('Saved', 1000);
    expect(get(toastMessage)?.text).toBe('Saved');
    vi.advanceTimersByTime(999);
    expect(get(toastMessage)?.text).toBe('Saved');
    vi.advanceTimersByTime(1);
    expect(get(toastMessage)).toBeNull();
  });

  it('a newer message replaces the older one and keeps its own time', () => {
    toast('A', 1000);
    vi.advanceTimersByTime(800);
    toast('B', 1000);
    vi.advanceTimersByTime(300);
    expect(get(toastMessage)?.text).toBe('B');
  });

  it('the same text twice is two messages, so it is announced twice', () => {
    toast('Saved');
    const first = get(toastMessage)?.id;
    toast('Saved');
    expect(get(toastMessage)?.id).not.toBe(first);
  });
});
```

  - `frontend/tests/settings-rows.test.ts`: in "gives every user a Types row", `icon: 'object'` →
    `icon: 'shapes'`; add:

```ts
  // The audit found one cube for Objects, Types and Data: every row draws its own icon.
  it('gives every row an icon of its own', () => {
    const icons = settingsRows(input({ isAdmin: true })).map((r) => r.icon);
    expect(new Set(icons).size).toBe(icons.length);
  });
```

Run: `cd frontend && npx vitest run tests/autosave.test.ts tests/toast.test.ts tests/settings-rows.test.ts`
Expected: FAIL (modules missing; types icon is `object`).

- [ ] **Step 2: `frontend/src/lib/autosave.ts`**

```ts
/**
 * Saves a setting a moment after the last change, one request at a time, and only ever the
 * newest value: two quick changes are one request, and a change made while a request is out is
 * sent when it returns, so an older answer can never land after a newer one. `onsaved` runs when
 * the newest value is on the server; `onerror` when sending it failed. `flush` sends a waiting
 * value now -- a page calls it when it is left.
 */
export function autosave<T>(
  save: (value: T) => Promise<void>,
  opts: { delay?: number; onsaved?: () => void; onerror?: (e: unknown) => void } = {},
): { push(value: T): void; flush(): Promise<void> } {
  const delay = opts.delay ?? 300;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: { value: T } | null = null;
  let running: Promise<void> | null = null;

  async function run(): Promise<void> {
    while (pending) {
      const { value } = pending;
      pending = null;
      try {
        await save(value);
        if (!pending) opts.onsaved?.();
      } catch (e) {
        if (!pending) opts.onerror?.(e);
      }
    }
    running = null;
  }
  function start(): void {
    timer = undefined;
    if (!running) running = run();
  }

  return {
    push(value: T) {
      pending = { value };
      clearTimeout(timer);
      timer = setTimeout(start, delay);
    },
    flush() {
      if (timer !== undefined) { clearTimeout(timer); start(); }
      return running ?? Promise.resolve();
    },
  };
}
```

- [ ] **Step 3: `frontend/src/lib/toast.ts`**

```ts
import { writable } from 'svelte/store';

/** The one message a page's `Toaster` shows: "Saved", or what an action did. `id` is new with
 *  every message, so the same text twice is announced twice. */
export const toastMessage = writable<{ id: number; text: string } | null>(null);

let next = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Shows `text` for `ms`, replacing whatever was showing. Not for errors: an error stays on the
 *  page, next to what caused it, until it is fixed. */
export function toast(text: string, ms = 3000): void {
  clearTimeout(timer);
  const id = ++next;
  toastMessage.set({ id, text });
  timer = setTimeout(() => toastMessage.update((m) => (m?.id === id ? null : m)), ms);
}
```

- [ ] **Step 4: `frontend/src/lib/Toaster.svelte`**

```svelte
<script lang="ts">
  import { toastMessage } from './toast';
</script>

<!-- Always in the page, empty until there is something to say: a live region inserted together
     with its text is often not announced. Above the tab bar on a phone, bottom centre of the
     content pane on a desktop. Foreground on background, inverted: the pair is already tested. -->
<div role="status" aria-live="polite" aria-atomic="true" data-testid="toast"
     class="pointer-events-none fixed inset-x-0 z-30 flex justify-center px-4 max-desk:bottom-[calc(var(--navbar)+env(safe-area-inset-bottom)+0.75rem)] desk:bottom-6 desk:left-60">
  {#if $toastMessage}
    {#key $toastMessage.id}
      <p class="m-0 max-w-md rounded-full bg-foreground px-4 py-2 text-center text-sm font-medium text-background shadow-lg">{$toastMessage.text}</p>
    {/key}
  {/if}
</div>
```

- [ ] **Step 5: `frontend/src/lib/settings-rows.ts`.** Replace `import type { IconName } from './Icon.svelte';` with

```ts
/** A row's icon, drawn by SettingsRow.svelte from lucide (the settings chunk, not the eager
 *  Icon.svelte). Every row has its own. */
export type SettingsIcon = 'palette' | 'user' | 'bell' | 'shapes' | 'key' | 'archive' | 'users' | 'database';
```

  change `icon: IconName;` to `icon: SettingsIcon;`, and the rows' icons: account `'person'` →
  `'user'`, types `'object'` → `'shapes'`, data `'box'` → `'archive'`, people `'people'` →
  `'users'` (appearance `'palette'`, notifications `'bell'`, api `'key'`, database `'database'`
  stay).

Run: `npx vitest run tests/autosave.test.ts tests/toast.test.ts tests/settings-rows.test.ts` — PASS.

- [ ] **Step 6: `frontend/src/lib/SettingsRow.svelte`** (replace the file):

```svelte
<script lang="ts">
  import Archive from '@lucide/svelte/icons/archive';
  import Bell from '@lucide/svelte/icons/bell';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import Database from '@lucide/svelte/icons/database';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Palette from '@lucide/svelte/icons/palette';
  import Shapes from '@lucide/svelte/icons/shapes';
  import UserRound from '@lucide/svelte/icons/user-round';
  import UsersRound from '@lucide/svelte/icons/users-round';
  import { go } from './router';
  import { t } from '../i18n';
  import type { SettingsIcon, SettingsRowModel } from './settings-rows';

  let { row }: { row: SettingsRowModel } = $props();
  const ICONS: Record<SettingsIcon, typeof Bell> = {
    palette: Palette, user: UserRound, bell: Bell, shapes: Shapes, key: KeyRound,
    archive: Archive, users: UsersRound, database: Database,
  };
  const Glyph = $derived(ICONS[row.icon]);
</script>

<!-- One row of a grouped list: the icon tile, the name, the current value, a chevron. The whole
     row is the button, so its name is "Account ben" and the e2e suite finds it by /Account/.
     A row with no value renders no value element at all, rather than an empty one. -->
<button data-slot="settings-row" onclick={() => go(row.path)}
        class="flex min-h-14 w-full cursor-pointer items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring">
  <span aria-hidden="true" class="grid size-9 shrink-0 place-items-center rounded-md bg-primary/10 text-brand-ink"><Glyph class="size-5" /></span>
  <span class="min-w-0 flex-1 truncate text-base font-medium text-foreground">{$t(row.label)}</span>
  {#if row.value}<span class="min-w-0 max-w-[45%] truncate text-sm text-muted-foreground">{row.value}</span>{/if}
  <ChevronRight aria-hidden="true" class="size-4 shrink-0 text-muted-foreground" />
</button>
```

- [ ] **Step 7: `frontend/src/lib/SignedIn.svelte`** (eager: plain classes only). Change the props
  line to

```ts
  /** `compact` is the sidebar's version: the name and an icon-only sign-out button. `framed`
   *  draws the card's own border; the account menu's popover already is one. */
  let { compact = false, framed = true }: { compact?: boolean; framed?: boolean } = $props();
```

  and replace everything after `</script>` with:

```svelte
{#if $user}
  <div data-testid="signed-in"
       class={['grid grid-cols-[auto_minmax(0,1fr)_auto] items-center', compact ? 'gap-2' : 'gap-3', framed && !compact && 'rounded-lg border border-border bg-card py-2 pr-2 pl-3 shadow-xs']}>
    <!-- The initial is decoration; the name beside it is what is read out. -->
    <span aria-hidden="true" class={['grid shrink-0 place-items-center rounded-full bg-primary font-bold text-primary-foreground', compact ? 'size-7 text-sm' : 'size-9']}>{$user.username.slice(0, 1).toUpperCase()}</span>
    <span class="flex min-w-0 flex-col">
      {#if !compact}<span class="text-xs text-muted-foreground">{$t('account.signed-in-as')}</span>{/if}
      <span class="flex min-w-0 items-center gap-2">
        <b class="truncate font-semibold text-foreground">{$user.username}</b>
        {#if $user.is_admin}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('settings.user-admin')}</span>{/if}
      </span>
    </span>
    <button data-slot="sign-out" onclick={signOut} disabled={busy} aria-label={$t('login.logout')} title={$t('login.logout')}
            class={['inline-flex min-h-11 min-w-11 cursor-pointer items-center justify-center gap-2 rounded-md text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:cursor-default disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring', !compact && 'px-3']}>
      <Icon name="logout" />{#if !compact}<span>{$t('login.logout')}</span>{/if}
    </button>
  </div>
  {#if error}<p role="alert" class="m-0 mt-2 text-sm font-medium text-destructive">{error}</p>{/if}
{/if}
```

  Delete the `<style>` block.

- [ ] **Step 8: `frontend/src/lib/AccountMenu.svelte`** (eager: plain classes). Keep the script;
  replace everything from `{#if $user}` to the end with:

```svelte
{#if $user}
  <!-- Zero layout height, so the 44 px button overhangs the bar's centre line instead of making
       the top bar taller. Hidden on desktop, where the sidebar's foot carries all of it. -->
  <div class="relative flex h-0 shrink-0 items-center desk:hidden" bind:this={root}>
    <button data-slot="account-avatar" bind:this={avatar} aria-label={$t('account.menu', { name: $user.username })}
            aria-expanded={open} aria-haspopup="true" onclick={() => (open ? hide(false) : show())}
            class="grid size-11 cursor-pointer place-items-center rounded-full focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">
      <span aria-hidden="true" class="grid size-9 place-items-center rounded-full bg-primary text-sm font-bold text-primary-foreground">{$user.username.slice(0, 1).toUpperCase()}</span>
    </button>
    {#if open}
      <div role="group" aria-label={$t('account.menu', { name: $user.username })} bind:this={panel}
           class="absolute top-[calc(22px+0.25rem)] right-0 z-20 flex w-[min(320px,calc(100vw-1.5rem))] flex-col gap-2 rounded-lg border border-border bg-popover p-2 text-popover-foreground shadow-xl">
        <SignedIn framed={false} />
        <button data-slot="account-settings" onclick={() => { hide(false); go('/settings/account'); }}
                class="flex min-h-11 w-full cursor-pointer items-center rounded-md px-3 text-left text-sm font-medium text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring">{$t('account.settings')}</button>
      </div>
    {/if}
  </div>
{/if}
```

  Delete the `<style>` block.

- [ ] **Step 9: Contrast.** In `theme-contrast.test.ts`'s 4.5:1 text list add
  `['destructive', 'muted']` with the comment
  `// A ghost button in destructive text on its hover fill (Discard, Remove, Revoke, Delete).`

- [ ] **Step 10: Failing e2e test.** Append to `39-screens.spec.ts`:

```ts
test('the settings hub groups its rows, each with its own icon, and says who is signed in', async ({ page }) => {
  await signInFresh(page, '39-hub');
  await page.goto('/settings');
  const rows = page.getByRole('list', { name: 'You' }).getByRole('button');
  await expect(rows).toHaveCount(6);
  const icons = await rows.evaluateAll((els) => els.map((e) => e.querySelector('svg')?.innerHTML ?? ''));
  expect(new Set(icons).size, 'every row draws a different icon').toBe(6);
  for (const h of await rows.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height))) expect(h).toBeGreaterThanOrEqual(44);
  await expect(page.getByRole('main').getByTestId('signed-in')).toContainText('Signed in as');
  await expect(page.getByTestId('about')).toContainText('Version');
});
```

Run: `npx playwright test 39-screens -g "settings hub"` — FAIL (no named list).

- [ ] **Step 11: `frontend/src/routes/Settings.svelte`.** Add the imports

```ts
  import { Button } from '$lib/components/ui/button/index.js';
  import { hintClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
```

  and at the end of the script

```ts
  const you = $derived(rows.filter((r) => r.group === 'you'));
  const instance = $derived(rows.filter((r) => r.group === 'instance'));
  const group = 'm-0 list-none divide-y divide-border overflow-hidden rounded-lg border border-border bg-card p-0 shadow-xs';
  const pair = 'flex justify-between gap-3';
  const value = 'm-0 text-right text-foreground tabular-nums [overflow-wrap:anywhere]';
```

  Replace everything from `<main>` to the end of the file:

```svelte
<main>
  <TopBar title={$t('settings.title')} backTo="/" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    <!-- A failed write is an alert and the only time-sensitive thing here, absent when the queue is
         clean. It expands in place rather than behind a route of its own. -->
    {#if dead.length > 0}
      <section aria-labelledby="settings-failed" class="flex flex-col gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3">
        <h2 id="settings-failed" class="m-0 text-base font-semibold text-destructive">{$t('outbox.failed')}</h2>
        <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
          {#each dead as op (op.id)}
            {@const d = describeFailedWrite(op, $t)}
            <li data-testid="failed-write" class="flex items-center gap-3 rounded-md bg-card p-3">
              <span class="flex min-w-0 flex-1 flex-col gap-1 [overflow-wrap:anywhere]">
                <b class="font-semibold text-foreground">{d.what}{#if d.name}: {d.name}{/if}</b>
                {#if d.reason}<span class="text-sm text-destructive">{d.reason}</span>{/if}
              </span>
              <Button variant="ghost" class="min-h-11 shrink-0 text-destructive" onclick={() => discardOp(op.id)}>{$t('outbox.discard')}</Button>
            </li>
          {/each}
        </ul>
        <Button class="h-12 self-start" onclick={retryOutbox}>{$t('outbox.retry')}</Button>
      </section>
    {/if}

    <!-- Who this is, and the way out, first: on a phone this screen is one tap from the tab bar. -->
    <SignedIn />

    <section class="flex flex-col gap-2">
      <h2 id="settings-you" class={sectionHeadingClass}>{$t('settings.you')}</h2>
      <ul role="list" aria-labelledby="settings-you" class={group}>
        {#each you as row (row.id)}<li><SettingsRow {row} /></li>{/each}
      </ul>
    </section>

    {#if isAdmin}
      <section class="flex flex-col gap-2">
        <h2 id="settings-instance" class={sectionHeadingClass}>{$t('settings.instance')}</h2>
        <ul role="list" aria-labelledby="settings-instance" class={group}>
          {#each instance as row (row.id)}<li><SettingsRow {row} /></li>{/each}
        </ul>
      </section>
    {/if}

    <section class="flex flex-col gap-2">
      <h2 id="settings-about" class={sectionHeadingClass}>{$t('settings.about')}</h2>
      <dl data-testid="about" aria-labelledby="settings-about" class="m-0 flex flex-col gap-2 rounded-lg border border-border bg-card p-4 text-sm shadow-xs">
        <div class={pair}><dt class="text-muted-foreground">{$t('settings.version')}</dt><dd class={value}>{__APP_VERSION__}</dd></div>
        <div class={pair}><dt class="text-muted-foreground">{$t('settings.built')}</dt><dd class={value}>{built}</dd></div>
        {#if __BUILD_COMMIT__}<div class={pair}><dt class="text-muted-foreground">{$t('settings.commit')}</dt><dd class={value}><code class="font-mono text-xs">{__BUILD_COMMIT__}</code></dd></div>{/if}
        {#if serverVersion}<div class={pair}><dt class="text-muted-foreground">{$t('settings.server')}</dt><dd class={value}>{serverVersion}{#if backendLabel} · {backendLabel}{/if}</dd></div>{/if}
      </dl>
      {#if serverVersion && serverVersion !== __APP_VERSION__}
        <p class={hintClass}>{$t('settings.server-differs', { version: serverVersion })}</p>
      {/if}
    </section>
  </div>
</main>
```

  Delete the `<style>` block.

- [ ] **Step 12: Move the specs** (locator only).
  - `13-shell.spec.ts`: `page.locator('main .signed-in')` →
    `page.getByRole('main').getByTestId('signed-in')`; `page.locator('main dl.about')` →
    `page.getByTestId('about')`.
  - `14-settings.spec.ts` ("the failed-sync banner…"): `page.locator('.card.row', { hasText: 'Doomed entry' })` →
    `page.getByTestId('failed-write').filter({ hasText: 'Doomed entry' })`.

- [ ] **Step 13: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 39-screens 13-shell 14-settings 01-smoke 25-offline-cache`
Expected: PASS, both projects.
Capture `../shots/r5-t4 settings,dashboard`; compare `14-settings` with `r5-before` (grouped rows,
tinted icon tiles, all eight icons different) and `03-dashboard` (avatar unchanged).

```bash
git add -A frontend
git commit -m "feat: settings hub as grouped rows with an icon each; autosave and toast for settings pages"
```

---
### Task 5: Settings that save themselves — Appearance and Notifications

**Files:**
- Modify: `frontend/src/routes/settings/Appearance.svelte`,
  `frontend/src/routes/settings/Notifications.svelte`, `frontend/src/i18n/en.ts`, `de.ts`,
  `frontend/tests-e2e/17-notifications.spec.ts`,
  `frontend/tests-e2e/33-personal-preferences.spec.ts`, `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Consumes: `autosave`, `toast`, `Toaster` (Task 4); `Field`, `CheckField`, `Input`,
  `NativeSelect`, `Button`, `field/classes.ts`.
- Labels (unchanged for the e2e suite): "Language"/"Sprache", "Date format", "First day of the
  week", "Theme", "Use these preferences only on this device", "Currency", "Timezone", "Daily
  delivery hour", "Timezone for that hour", "URL", "Format", "Bot token". Button names: "Turn on
  notifications", "Turn off on this device", "Send a test notification", "Configure bot",
  "Connect Telegram", "Disconnect Telegram", "Remove bot".
- Removed: "Apply appearance", Appearance's instance "Save", "Apply delivery time", the webhook
  "Save"; i18n keys `settings.save-appearance`, `notify.save-hour`.

- [ ] **Step 1: i18n.** Append to `en.ts`:

```ts
  'settings.saved-device': 'Saved on this device',
  'settings.device-only-hint': 'Changes then stay on this device, and the account keeps what it had.',
  'settings.currency-invalid': 'Use a three-letter code such as EUR.',
  'notify.digest-title': 'Daily digest',
  'notify.hour-invalid': 'Choose an hour from 0 to 23.',
```

  and to `de.ts`:

```ts
  'settings.saved-device': 'Auf diesem Gerät gespeichert',
  'settings.device-only-hint': 'Änderungen bleiben dann auf diesem Gerät, das Konto behält seine Einstellungen.',
  'settings.currency-invalid': 'Bitte einen dreistelligen Code wie EUR eingeben.',
  'notify.digest-title': 'Tägliche Zusammenfassung',
  'notify.hour-invalid': 'Bitte eine Stunde von 0 bis 23 wählen.',
```

  Delete the `settings.save-appearance` and `notify.save-hour` lines from both files.

- [ ] **Step 2: Failing e2e test.** Append to `39-screens.spec.ts`:

```ts
test('appearance saves itself: a change applies at once and says so', async ({ page }) => {
  await signInFresh(page, '39-appearance');
  await page.goto('/settings/appearance');
  await expect(page.getByRole('main').getByRole('button', { name: /Apply|Save/ })).toHaveCount(0);

  await page.getByLabel('Theme').selectOption('dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.getByRole('status')).toHaveText('Saved');
  await expect.poll(async () => (await (await page.request.get('/api/me/appearance')).json()).theme).toBe('dark');

  // The device-only box sits level with its label (the audit found it misaligned).
  const box = (await page.getByRole('checkbox', { name: 'Use these preferences only on this device' }).boundingBox())!;
  const label = (await page.getByText('Use these preferences only on this device', { exact: true }).boundingBox())!;
  expect(Math.abs(box.y + box.height / 2 - (label.y + label.height / 2))).toBeLessThan(4);
});
```

Run: `npx playwright test 39-screens -g "appearance saves itself"` — FAIL ("Apply appearance" exists).

- [ ] **Step 3: `Appearance.svelte`** (replace the file):

```svelte
<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import TopBar from '../../lib/TopBar.svelte';
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { autosave } from '../../lib/autosave';
  import { errorMessage } from '../../lib/api-error';
  import { api } from '../../lib/api';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { errorClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { locale, navigatorLangs, t } from '../../i18n';
  import { LANG_NAMES, SUPPORTED } from '../../i18n/detect';
  import { settings, appearance, setDeviceOverride, applyAccountAppearance, type LocalSettings } from '../../stores/settings';
  import { currency, rememberCurrentCurrency, user } from '../../stores/session';
  import { DATE_FORMATS, fmtDate, resolveDateFormat, type DateFormat } from '../../lib/format';
  import type { Settings } from '../../lib/types';

  const EXAMPLE = '2026-09-15';
  /** What `auto` resolves to right now, shown in its own label. `navigatorLangs` so a
   *  `languagechange` updates it live. */
  const resolvedAuto = $derived(resolveDateFormat('auto', $locale, $navigatorLangs));
  const dateFormatLabel = (id: (typeof DATE_FORMATS)[number]): string =>
    id === 'auto'
      ? $t('settings.date-format-auto', { example: fmtDate(EXAMPLE, resolvedAuto) })
      : fmtDate(EXAMPLE, id as DateFormat);

  let error = $state('');
  // Read from the store itself, so the box follows the stored record.
  const overridden = $derived(!!$appearance[String($user?.id)]?.override);

  /** The account's appearance, a moment after the last change, one request at a time. The answer
   *  is applied only if nothing changed while it was out (`issuedWith`). */
  const accountSave = autosave<LocalSettings>(async (value) => {
    const me = get(user);
    if (!me) return;
    const saved = await api<LocalSettings>('PUT', '/me/appearance', value);
    applyAccountAppearance(me.id, saved, value);
  }, {
    onsaved: () => { error = ''; toast($t('object.saved')); },
    onerror: (e) => { error = errorMessage(e, $t); },
  });

  /** A setting applies at once (the store is what the app reads) and is saved to the account,
   *  unless this device keeps its own. */
  function set<K extends keyof LocalSettings>(key: K, value: LocalSettings[K]) {
    settings.update((s) => ({ ...s, [key]: value }));
    if (overridden) toast($t('settings.saved-device'));
    else accountSave.push(get(settings));
  }

  /** On: changes stay here. Off: this device follows the account again, and what it shows now
   *  becomes the account's -- what "Apply appearance" used to do. */
  function setOverride(enabled: boolean) {
    const me = get(user);
    if (!me) return;
    setDeviceOverride(me.id, enabled);
    if (enabled) toast($t('settings.saved-device'));
    else accountSave.push(get(settings));
  }

  const isAdmin = $derived($user?.is_admin === true);
  /** Every zone the browser knows, when it can say; a plain text field otherwise. */
  const zones: string[] = (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.('timeZone') ?? [];

  let currencyText = $state('');
  let currencyError = $state('');
  let instanceError = $state('');
  let timezone = $state('');
  let timezoneLocked = $state(false);
  let instanceLoaded = $state(false);

  const instanceSave = autosave<Record<string, string>>(async (body) => {
    const s = await api<Settings>('PUT', '/settings', body);
    currency.set(s.currency);
    // Otherwise an offline start right after this change would show the old currency.
    rememberCurrentCurrency(s.currency);
  }, {
    onsaved: () => { instanceError = ''; toast($t('object.saved')); },
    onerror: (e) => { instanceError = errorMessage(e, $t); },
  });

  /** Currency and timezone go together, as the server takes them; a currency that cannot be one
   *  is refused here, under its field, and nothing is sent. */
  function saveInstance() {
    const code = currencyText.trim().toUpperCase();
    if (!/^[A-Z]{3}$/.test(code)) { currencyError = $t('settings.currency-invalid'); return; }
    currencyError = '';
    currencyText = code;
    const body: Record<string, string> = { currency: code };
    if (!timezoneLocked && timezone.trim()) body.timezone = timezone.trim();
    instanceSave.push(body);
  }

  onMount(async () => {
    currencyText = $currency;
    if (!isAdmin) return;
    try {
      const s = await api<Settings>('GET', '/settings');
      timezone = s.timezone;
      timezoneLocked = s.timezone_locked;
    } catch { /* the field stays empty and saving leaves the timezone alone */ }
    instanceLoaded = true;
  });
  // A change made just before leaving still reaches the server.
  onDestroy(() => { void accountSave.flush(); void instanceSave.flush(); });

  const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';
</script>

<main>
  <TopBar title={$t('settings.appearance')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}

    <section aria-labelledby="appearance-you" class={card}>
      <h2 id="appearance-you" class={sectionHeadingClass}>{$t('settings.you')}</h2>
      <Field id="set-language" label={$t('settings.language')}>
        <NativeSelect bind:value={() => $settings.locale, (v) => set('locale', v as LocalSettings['locale'])}>
          <option value="auto">{$t('settings.language-auto')}</option>
          {#each SUPPORTED as l (l)}<option value={l}>{LANG_NAMES[l]}</option>{/each}
        </NativeSelect>
      </Field>
      <Field id="set-date-format" label={$t('settings.date-format')}>
        <NativeSelect bind:value={() => $settings.dateFormat, (v) => set('dateFormat', v as LocalSettings['dateFormat'])}>
          {#each DATE_FORMATS as id (id)}<option value={id}>{dateFormatLabel(id)}</option>{/each}
        </NativeSelect>
      </Field>
      <Field id="set-first-day" label={$t('settings.first-day')}>
        <NativeSelect bind:value={() => $settings.firstDayOfWeek, (v) => set('firstDayOfWeek', v as LocalSettings['firstDayOfWeek'])}>
          <option value="locale">{$t('settings.first-day-auto')}</option>
          <option value="monday">{$t('weekday.1')}</option>
          <option value="sunday">{$t('weekday.7')}</option>
        </NativeSelect>
      </Field>
      <Field id="set-theme" label={$t('settings.theme')}>
        <NativeSelect bind:value={() => $settings.theme, (v) => set('theme', v as LocalSettings['theme'])}>
          <option value="auto">{$t('settings.theme-auto')}</option>
          <option value="light">{$t('settings.theme-light')}</option>
          <option value="dark">{$t('settings.theme-dark')}</option>
        </NativeSelect>
      </Field>
      <CheckField id="set-device-only" label={$t('settings.device-only')} hint={$t('settings.device-only-hint')}
                  bind:checked={() => overridden, (on) => setOverride(on)} />
    </section>

    {#if isAdmin}
      <section aria-labelledby="appearance-instance" aria-busy={!instanceLoaded} class={card}>
        <h2 id="appearance-instance" class={sectionHeadingClass}>{$t('settings.instance')}</h2>
        <Field id="set-currency" label={$t('settings.currency')} hint={$t('settings.currency-hint')} error={currencyError}>
          <!-- `bind:value` follows typing (input); saving waits for `change` (blur or Enter). -->
          <Input bind:value={currencyText} maxlength={3} autocomplete="off" autocapitalize="characters" spellcheck="false" onchange={saveInstance} />
        </Field>
        <Field id="set-timezone" label={$t('settings.timezone')} hint={timezoneLocked ? $t('settings.timezone-locked') : $t('settings.timezone-hint')}>
          {#if zones.length > 0}
            <NativeSelect disabled={timezoneLocked} bind:value={() => timezone, (v) => { timezone = v ?? ''; saveInstance(); }}>
              <!-- The current value stays selectable even when this browser's list lacks it. -->
              {#if timezone && !zones.includes(timezone)}<option value={timezone}>{timezone}</option>{/if}
              {#each zones as z (z)}<option value={z}>{z}</option>{/each}
            </NativeSelect>
          {:else}
            <Input bind:value={timezone} disabled={timezoneLocked} autocomplete="off" onchange={saveInstance} />
          {/if}
        </Field>
        {#if instanceError}<p role="alert" class={errorClass}>{instanceError}</p>{/if}
      </section>
    {/if}
  </div>
  <Toaster />
</main>
```

- [ ] **Step 4: `Notifications.svelte` script.** Keep `data`, `push`, `url`, `format`, `timezone`,
  `zones`, `busy`, `error`, the Telegram state (`telegramLink`, `telegramToken`, `now`,
  `telegramSeconds`), `load`, `onMount`, the countdown `$effect`, `togglePush`, `linkTelegram`,
  `removeTelegram` and `unlinkTelegram`. Then:
  - Imports become:

```ts
  import { errorMessage } from '../../lib/api-error';
  import { onDestroy, onMount } from 'svelte';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import TopBar from '../../lib/TopBar.svelte';
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { autosave } from '../../lib/autosave';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { errorClass, hintClass, sectionHeadingClass, warnClass } from '$lib/components/ui/field/classes.js';
  import { disablePush, enablePush, pushState, type PushState } from '../../lib/push';
  import type { NotificationSettings, NotificationTest, TelegramLink } from '../../lib/types';
```

  - `let hour = $state(8);` → `let hour = $state<number | null>(8);`
  - Delete `let message = $state('');`, the old `saveHour`, `saveWebhook`; remove every
    `message = ''` and `message = …` assignment left in the kept functions.
  - Add:

```ts
  let hourError = $state('');
  let urlError = $state('');

  const hourSave = autosave<{ hour: number; timezone: string | null }>(async (body) => {
    data = await api<NotificationSettings>('PUT', '/me/notifications/hour', body);
  }, { onsaved: () => { hourError = ''; toast($t('object.saved')); }, onerror: (e) => { hourError = errorMessage(e, $t); } });

  const webhookSave = autosave<{ url: string | null; format: 'text' | 'json' }>(async (body) => {
    data = await api<NotificationSettings>('PUT', '/me/notifications', body);
  }, { onsaved: () => { urlError = ''; toast($t('object.saved')); }, onerror: (e) => { urlError = errorMessage(e, $t); } });

  onDestroy(() => { void hourSave.flush(); void webhookSave.flush(); });

  /** The hour and its timezone are one setting on the server. An hour that cannot be one is
   *  refused here, under its field. */
  function saveHour() {
    if (hour === null || !Number.isInteger(hour) || hour < 0 || hour > 23) { hourError = $t('notify.hour-invalid'); return; }
    hourError = '';
    hourSave.push({ hour, timezone: timezone || null });
  }

  /** A bad URL comes back from the server ("http or https") and stays under the field. */
  function saveWebhook() {
    webhookSave.push({ url: url.trim() || null, format });
  }
```

  - `sendTest`: `message = parts.length > 0 ? parts.join(' ') : $t('notify.test-nowhere');` →
    `toast(parts.length > 0 ? parts.join(' ') : $t('notify.test-nowhere'), 8000);`
  - `saveTelegram`: `message = $t('object.saved');` → `toast($t('object.saved'));`
  - Append: `const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';`
    and `const heading = 'm-0 text-base font-semibold text-foreground';`

- [ ] **Step 5: `Notifications.svelte` markup.** Replace everything from `<main>` to the end:

```svelte
<main>
  <TopBar title={$t('settings.notifications')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6" aria-busy={data === null}>
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}

    <section aria-labelledby="notify-digest" class={card}>
      <h2 id="notify-digest" class={heading}>{$t('notify.digest-title')}</h2>
      <Field id="delivery-hour" label={$t('notify.delivery-hour')} error={hourError}>
        <Input type="number" min={0} max={23} inputmode="numeric" bind:value={hour} onchange={saveHour} />
      </Field>
      <Field id="delivery-timezone" label={$t('notify.delivery-timezone')} hint={$t('notify.delivery-timezone-hint')}>
        {#if zones.length > 0}
          <NativeSelect bind:value={() => timezone, (v) => { timezone = v ?? ''; saveHour(); }}>
            <option value="">{$t('notify.instance-timezone')}</option>
            <!-- The stored value stays selectable even when this browser's list lacks it. -->
            {#if timezone && !zones.includes(timezone)}<option value={timezone}>{timezone}</option>{/if}
            {#each zones as z (z)}<option value={z}>{z}</option>{/each}
          </NativeSelect>
        {:else}
          <Input bind:value={timezone} placeholder={$t('notify.instance-timezone')} onchange={saveHour} />
        {/if}
      </Field>
      {#if data?.deliveries?.length}
        <div class="flex flex-col gap-1">
          <h3 class={sectionHeadingClass}>{$t('notify.delivery-status')}</h3>
          <ul role="list" class="m-0 flex list-none flex-col gap-1 p-0 text-sm text-foreground">
            {#each data.deliveries as delivery}
              <li>
                {delivery.target.startsWith('telegram:') ? 'Telegram' : delivery.target.startsWith('push:') ? $t('notify.push-title') : $t('notify.webhook-title')}{#if delivery.last_success} · {$t('notify.delivery-ok')}: {delivery.last_success}{/if}{#if delivery.last_error}<span class="text-destructive"> · {$t('notify.delivery-error')}</span>{/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </section>

    <section aria-labelledby="notify-push" class={card}>
      <h2 id="notify-push" class={heading}>{$t('notify.push-title')}</h2>
      {#if data}<p class={hintClass}>{$t('notify.push-hint', { hour: data.hour })}</p>{/if}
      {#if push === 'unsupported'}
        <p class={hintClass}>{$t('notify.push-unsupported')}</p>
      {:else if push === 'denied'}
        <p class={warnClass}>{$t('notify.push-denied')}</p>
      {:else if push}
        {#if push === 'on'}<p class="m-0 text-sm text-foreground">{$t('notify.push-on')}</p>{/if}
        <!-- The page's one primary action, while there is something to turn on. -->
        <Button variant={push === 'on' ? 'outline' : 'default'} class="h-12 self-start" onclick={togglePush} disabled={busy || !data}>
          {push === 'on' ? $t('notify.push-disable') : $t('notify.push-enable')}
        </Button>
      {/if}
      {#if data && data.push_devices > 0}
        <p class={hintClass}>{data.push_devices === 1 ? $t('notify.push-devices-one') : $t('notify.push-devices', { n: data.push_devices })}</p>
      {/if}
    </section>

    <section aria-labelledby="notify-telegram" class={card}>
      <h2 id="notify-telegram" class={heading}>{$t('notify.telegram-title')}</h2>
      {#if !data?.telegram_configured}
        <p class={hintClass}>{$t('notify.telegram-setup')}</p>
        <Field id="telegram-token" label={$t('notify.telegram-token')}>
          <Input type="password" autocomplete="off" bind:value={telegramToken} />
        </Field>
        <Button variant="outline" class="h-12 self-start" onclick={saveTelegram} disabled={busy || !telegramToken.trim()}>{$t('notify.telegram-save')}</Button>
      {:else}
        <p class={hintClass}>{$t('notify.telegram-bot', { name: data.telegram_bot_username ? `@${data.telegram_bot_username}` : 'Telegram' })}</p>
        {#if data.telegram_legacy}<p class={warnClass}>{$t('notify.telegram-legacy')}</p>{/if}
        {#if data.telegram_connected}
          <p class="m-0 text-sm text-foreground">{$t('notify.telegram-connected', { name: data.telegram_display_name ?? 'Telegram' })}</p>
          {#if data.telegram_last_error}<p class={errorClass}>{data.telegram_last_error}</p>{/if}
          <Button variant="outline" class="h-12 self-start" onclick={unlinkTelegram} disabled={busy}>{$t('notify.telegram-disconnect')}</Button>
        {:else}
          <p class={hintClass}>{$t('notify.telegram-hint')}</p>
          {#if data.telegram_last_error}<p class={errorClass}>{data.telegram_last_error}</p>{/if}
          <Button variant="outline" class="h-12 self-start" onclick={linkTelegram} disabled={busy}>{$t('notify.telegram-connect')}</Button>
          {#if telegramLink}
            <div class="flex flex-col items-start gap-2">
              <!-- Server-generated SVG (src/domain/pairing.rs), never user input. -->
              <div class="w-60 max-w-full [&_svg]:block [&_svg]:h-auto [&_svg]:w-full" role="img" aria-label={$t('notify.telegram-title')}>{@html telegramLink.qr_svg}</div>
              <a data-slot="telegram-open" href={telegramLink.url} target="_blank" rel="noreferrer"
                 class="inline-flex min-h-11 items-center text-sm font-medium text-brand-ink underline underline-offset-4 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('notify.telegram-open')}</a>
              <span class={hintClass}>{$t('notify.telegram-expires', { n: telegramSeconds })}</span>
            </div>
          {/if}
        {/if}
        {#if !data.telegram_legacy}
          <details class="group rounded-lg border border-border">
            <summary class="flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-lg px-3 text-sm font-medium text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring [&::-webkit-details-marker]:hidden">
              <ChevronRight aria-hidden="true" class="size-4 transition-transform group-open:rotate-90" />{$t('notify.telegram-replace')}
            </summary>
            <div class="flex flex-col gap-3 px-3 pb-3">
              <Field id="telegram-replacement" label={$t('notify.telegram-token')}>
                <Input type="password" autocomplete="off" bind:value={telegramToken} />
              </Field>
              <Button variant="outline" class="h-12 self-start" onclick={saveTelegram} disabled={busy || !telegramToken.trim()}>{$t('notify.telegram-save')}</Button>
            </div>
          </details>
        {/if}
        <Button variant="destructive" class="h-12 self-start" onclick={removeTelegram} disabled={busy}>{$t('notify.telegram-remove')}</Button>
      {/if}
    </section>

    <section aria-labelledby="notify-webhook" class={card}>
      <h2 id="notify-webhook" class={heading}>{$t('notify.webhook-title')}</h2>
      <p class={hintClass}>{data?.instance_webhook ? $t('notify.webhook-hint-instance') : $t('notify.webhook-hint')}</p>
      <Field id="wu" label={$t('notify.webhook-url')} error={urlError}>
        <Input type="url" inputmode="url" autocomplete="off" placeholder="https://ntfy.sh/…" bind:value={url} onchange={saveWebhook} />
      </Field>
      <Field id="wf" label={$t('notify.format')}>
        <NativeSelect bind:value={() => format, (v) => { format = v as 'text' | 'json'; saveWebhook(); }}>
          <option value="text">{$t('notify.format-text')}</option>
          <option value="json">{$t('notify.format-json')}</option>
        </NativeSelect>
      </Field>
    </section>

    <!-- A side effect (it sends something), so an explicit button, for every channel at once. -->
    <Button variant="outline" class="h-12 self-start" onclick={sendTest} disabled={busy}>{$t('notify.test')}</Button>
  </div>
  <Toaster />
</main>
```

  Delete the `<style>` block.

- [ ] **Step 6: Move the specs** (behaviour: no Save/Apply buttons).
  - `17-notifications.spec.ts`: both
    `await page.getByRole('button', { name: 'Save', exact: true }).click();` →
    `await page.getByLabel('URL').press('Tab');` (leaving the field saves it). The two
    `getByRole('status')` and the `getByRole('alert')` assertions stay.
  - `33-personal-preferences.spec.ts`:
    - "appearance applied to the account…": delete the "Apply appearance" click; after
      `await expect(page.getByText('Saved', { exact: true })).toBeVisible();` add
      `await expect.poll(async () => (await (await page.request.get('/api/me/appearance')).json())).toMatchObject({ firstDayOfWeek: 'sunday', dateFormat: 'iso' });`
    - "a device-only choice…": delete both "Apply appearance" clicks. After the first
      `selectOption('iso')` keep the "Saved" assertion and add
      `await expect.poll(async () => (await (await page.request.get('/api/me/appearance')).json()).dateFormat).toBe('iso');`;
      the second "Saved" assertion becomes
      `await expect(page.getByText('Saved on this device', { exact: true })).toBeVisible();`.
    - "a personal delivery hour…": delete the "Apply delivery time" click; before
      `await page.reload();` replace `await expect(page.getByRole('status')).toHaveText('Saved');` with

```ts
  await expect(page.getByRole('status')).toHaveText('Saved');
  // Two quick changes are saved in order, the newest last: wait for the server to hold both.
  await expect.poll(async () => (await (await page.request.get('/api/me/notifications')).json())).toMatchObject({ hour: 17, timezone: 'America/New_York' });
```

- [ ] **Step 7: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 39-screens 17-notifications 33-personal-preferences 01-smoke 26-date-format 32-calendar-reminders`
Expected: PASS, both projects.
Capture `../shots/r5-t5 appearance,notifications`; compare `15-settings-appearance` and
`16-settings-notifications` with `r5-before` (cards, no Apply buttons, checkbox level with its
label, at most one amber button).

```bash
git add -A frontend
git commit -m "feat: appearance and notification settings save themselves, with a Saved toast"
```

---
### Task 6: Pages of explicit actions, part 1 — Account, API access, Users, Data

**Files:**
- Create: `frontend/src/lib/PasswordInput.svelte`
- Modify: `frontend/src/routes/settings/Account.svelte`, `ApiAccess.svelte`, `People.svelte`,
  `Data.svelte`, `frontend/src/i18n/en.ts`, `de.ts`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests-e2e/07-api-tokens.spec.ts`, `frontend/tests-e2e/14-settings.spec.ts`,
  `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Produces: `PasswordInput.svelte` — props `id: string`, `value?: string` (bindable),
  `autocomplete: 'current-password' | 'new-password' | 'off'`, `required?: boolean`,
  `minlength?: number`, `describedby?: string`. Plain class strings, inline SVG; no `cn`, no field
  context, no bits-ui, no lucide (Task 8 uses it on the auth screens). Its toggle is a
  `<button aria-pressed>` named by hidden text "Show password".
- Test ids: `user-row`, `token-list`, `fresh-token`.
- Button names kept: "Sign out" (in the signed-in card), "Sign out everywhere", "Show QR code",
  "Create token", "Copy", "Revoke", "Add user", "Remove", "Export everything (zip)" (a link),
  "Import zip", "Retry failed saves", "Discard". New: "Change password" (was "Save").

- [ ] **Step 1: i18n.** Append to `en.ts`:

```ts
  'settings.new-password': 'New password',
  'settings.password-changed': 'Password changed',
  'settings.sessions': 'Sessions',
  'login.show-password': 'Show password',
  'setup.password-hint': 'At least 8 characters.',
```

  and to `de.ts`:

```ts
  'settings.new-password': 'Neues Passwort',
  'settings.password-changed': 'Passwort geändert',
  'settings.sessions': 'Sitzungen',
  'login.show-password': 'Passwort anzeigen',
  'setup.password-hint': 'Mindestens 8 Zeichen.',
```

- [ ] **Step 2: Failing e2e test.** Append to `39-screens.spec.ts`:

```ts
test('changing the password is one explicit action, and a password field can show what was typed', async ({ page }) => {
  const username = await signInFresh(page, '39-account');
  await page.goto('/settings/account');
  await page.getByLabel('Current password').fill('password123');
  const fresh = page.getByLabel('New password');
  await fresh.fill('password456');
  await expect(fresh).toHaveAttribute('type', 'password');
  const show = page.getByRole('button', { name: 'Show password' }).nth(1);
  await expect(show).toHaveAttribute('aria-pressed', 'false');
  await show.click();
  await expect(fresh).toHaveAttribute('type', 'text');
  await expect(show).toHaveAttribute('aria-pressed', 'true');
  await expect(fresh).toHaveValue('password456');

  await page.getByRole('button', { name: 'Change password' }).click();
  await expect(page.getByRole('status')).toHaveText('Password changed');
  await expect(page.getByLabel('Current password')).toHaveValue('');
  expect((await page.request.post('/api/auth/login', { data: { username, password: 'password456' } })).ok()).toBe(true);
});
```

Run: `npx playwright test 39-screens -g "changing the password"` — FAIL (no "New password" label).

- [ ] **Step 3: `frontend/src/lib/PasswordInput.svelte`**

```svelte
<script lang="ts">
  import { controlClass } from '$lib/components/ui/field/classes.js';
  import { t } from '../i18n';

  /**
   * A password field with a show/hide toggle. Plain class strings and inline icons, nothing from
   * the component library: Login and Setup use it, and they are the first screen a fresh install
   * shows. The caller draws the label (`for={id}`).
   */
  let { id, value = $bindable(''), autocomplete, required = false, minlength, describedby }: {
    id: string; value?: string; autocomplete: 'current-password' | 'new-password' | 'off';
    required?: boolean; minlength?: number; describedby?: string;
  } = $props();
  let shown = $state(false);
</script>

<div data-slot="password" class="relative min-w-0">
  <input {id} data-slot="password-input" type={shown ? 'text' : 'password'} bind:value {autocomplete} {required} {minlength}
         autocapitalize="off" spellcheck="false" aria-describedby={describedby} class={`${controlClass} pr-12`} />
  <!-- Named by its text, not `aria-label`: `getByLabel(/Password|Passwort/)` must find only the
       field, and "Passwort anzeigen" would match it. The name stays; `aria-pressed` says whether
       the password is shown. 48 px square, the field's own height. -->
  <button type="button" data-slot="password-toggle" aria-pressed={shown} aria-controls={id} onclick={() => (shown = !shown)}
          class="absolute inset-y-0 right-0 grid w-12 cursor-pointer place-items-center rounded-r-lg text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring">
    <span class="sr-only">{$t('login.show-password')}</span>
    <svg aria-hidden="true" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      {#if shown}
        <path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49" />
        <path d="M14.084 14.158a3 3 0 0 1-4.242-4.242" />
        <path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143" />
        <path d="m2 2 20 20" />
      {:else}
        <path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" />
        <circle cx="12" cy="12" r="3" />
      {/if}
    </svg>
  </button>
</div>
```

- [ ] **Step 4: `Account.svelte`** (replace the file):

```svelte
<script lang="ts">
  import { errorMessage } from '../../lib/api-error';
  import { onDestroy } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import SignedIn from '../../lib/SignedIn.svelte';
  import PasswordInput from '../../lib/PasswordInput.svelte';
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { Button } from '$lib/components/ui/button/index.js';
  import { errorClass, hintClass, labelClass } from '$lib/components/ui/field/classes.js';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { user, logoutEverywhere, signOutErrorMessage } from '../../stores/session';
  import { createPairing } from '../../lib/pairing';

  let currentPass = $state('');
  let ownPass = $state('');
  let busy = $state(false);
  let passwordError = $state('');
  let error = $state('');

  const pairing = createPairing();
  const pair = pairing.state;
  onDestroy(pairing.stop);

  async function signOutEverywhere() {
    if (!confirm($t('settings.logout-all-confirm'))) return;
    error = '';
    try { await logoutEverywhere(); } catch (e) { error = signOutErrorMessage(e, $t); }
  }

  // The server wants the password being replaced (`current_password`), so a session left open on
  // a shared computer is not enough to take the account over. A wrong one comes back as a 403
  // `wrong_password`, said here in the reader's language.
  async function changeOwnPassword(e: SubmitEvent) {
    e.preventDefault();
    if (!$user) return;
    busy = true; passwordError = '';
    try {
      await api('PATCH', `/users/${$user.id}`, { password: ownPass, current_password: currentPass });
      currentPass = ''; ownPass = '';
      toast($t('settings.password-changed'));
    } catch (err) { passwordError = errorMessage(err, $t); } finally { busy = false; }
  }

  async function requestPairCode() {
    error = '';
    try { await pairing.request(); } catch (e) { error = errorMessage(e, $t); }
  }

  const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';
  const heading = 'm-0 text-base font-semibold text-foreground';
</script>

<main>
  <TopBar title={$t('settings.account')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
    <!-- Who this is, and "Sign out" (the e2e suite finds it inside `main`). -->
    <SignedIn />

    <form aria-labelledby="account-password" onsubmit={changeOwnPassword} class={`m-0 ${card}`}>
      <h2 id="account-password" class={heading}>{$t('settings.change-password')}</h2>
      <div class="flex flex-col gap-1.5">
        <label for="cp" class={labelClass}>{$t('settings.current-password')}</label>
        <PasswordInput id="cp" bind:value={currentPass} autocomplete="current-password" required />
      </div>
      <div class="flex flex-col gap-1.5">
        <label for="op" class={labelClass}>{$t('settings.new-password')}</label>
        <PasswordInput id="op" bind:value={ownPass} autocomplete="new-password" required minlength={8} describedby="op-hint" />
        <p id="op-hint" class={hintClass}>{$t('setup.password-hint')}</p>
      </div>
      {#if passwordError}<p role="alert" class={errorClass}>{passwordError}</p>{/if}
      <!-- The page's one primary action. -->
      <Button type="submit" class="h-12 self-start" disabled={busy || ownPass.length < 8 || currentPass.length === 0}>{$t('settings.change-password')}</Button>
    </form>

    <section aria-labelledby="account-sessions" class={card}>
      <h2 id="account-sessions" class={heading}>{$t('settings.sessions')}</h2>
      <p class={hintClass}>{$t('settings.logout-all-hint')}</p>
      <Button variant="destructive" class="h-12 self-start" onclick={signOutEverywhere}>{$t('settings.logout-all')}</Button>
    </section>

    <section aria-labelledby="account-pair" class={card}>
      <h2 id="account-pair" class={heading}>{$t('account.pair-title')}</h2>
      {#if $pair.phase === 'idle' || $pair.phase === 'expired'}
        <Button variant="outline" class="h-12 self-start" onclick={requestPairCode}>
          {$t($pair.phase === 'expired' ? 'account.pair-new' : 'account.pair-show')}
        </Button>
      {:else}
        <p class={hintClass}>{$t('account.pair-hint')}</p>
        <p class={hintClass} aria-live="polite">{$t('account.pair-expires', { s: $pair.secondsLeft })}</p>
        {#if $pair.phase === 'qr'}
          <!-- Server-generated SVG (src/domain/pairing.rs) from the pairing URI, never user input. -->
          <div class="w-60 max-w-full [&_svg]:block [&_svg]:h-auto [&_svg]:w-full" role="img" aria-label={$t('account.pair-title')}>{@html $pair.pair?.qr_svg}</div>
          <Button variant="outline" class="min-h-11 self-start" onclick={pairing.showCode}>{$t('account.pair-code')}</Button>
        {:else}
          <code class="block rounded-md bg-muted p-2 font-mono text-sm break-all text-foreground">{$pair.pair?.uri}</code>
          <Button variant="outline" class="min-h-11 self-start" onclick={pairing.showQr}>{$t('account.pair-show')}</Button>
        {/if}
      {/if}
    </section>
  </div>
  <Toaster />
</main>
```

- [ ] **Step 5: `ApiAccess.svelte`.** Keep the script, but change `createToken` to take the form's
  submit (`async function createToken(e: SubmitEvent) { e.preventDefault(); error = ''; copied = false; … }`)
  and add the imports

```ts
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass } from '$lib/components/ui/field/classes.js';
```

  plus `const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';`.
  Replace everything from `<main>` to the end:

```svelte
<main>
  <TopBar title={$t('tokens.title')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
    <p class={hintClass}>{$t('tokens.intro')}</p>

    {#if freshToken}
      <div class="flex flex-col gap-2 rounded-lg border border-brand-ink bg-primary/10 p-4">
        <p class="m-0 text-sm font-medium text-foreground">{$t('tokens.created')}</p>
        <!-- Long, and never shown again: readable in full. -->
        <code data-testid="fresh-token" class="block rounded-md bg-card p-2 font-mono text-sm break-all text-foreground">{freshToken}</code>
        <Button variant="outline" class="min-h-11 self-start" onclick={copyToken}>{copied ? $t('tokens.copied') : $t('tokens.copy')}</Button>
      </div>
    {/if}

    <ul role="list" data-testid="token-list" class="m-0 flex list-none flex-col gap-2 p-0">
      {#each tokens as tok (tok.id)}
        <li class="flex min-h-14 items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 shadow-xs">
          <span class="flex min-w-0 flex-1 flex-col">
            <span class="truncate font-medium text-foreground">{tok.name}</span>
            <span class="truncate text-sm text-muted-foreground tabular-nums">
              {tok.prefix}… · {tok.last_used_at ? $t('tokens.last-used', { date: fmtDate(tok.last_used_at.slice(0, 10), $dateFormat) }) : $t('tokens.never-used')}
            </span>
          </span>
          <Button variant="ghost" class="min-h-11 shrink-0 text-destructive" onclick={() => revokeToken(tok)}>{$t('tokens.revoke')}</Button>
        </li>
      {:else}
        <li class="text-sm text-muted-foreground">{$t('tokens.none')}</li>
      {/each}
    </ul>

    <form onsubmit={createToken} class={`m-0 ${card}`}>
      <Field id="tn" label={$t('tokens.name')}>
        <Input bind:value={tokenName} placeholder={$t('tokens.name-placeholder')} maxlength={64} autocomplete="off" />
      </Field>
      <!-- The page's one primary action. -->
      <Button type="submit" class="h-12 self-start" disabled={tokenName.trim().length === 0}>{$t('tokens.create')}</Button>
      <p class={hintClass}>{$t('tokens.password-note')}</p>
    </form>
  </div>
</main>
```

  Delete the `<style>` block.

- [ ] **Step 6: `People.svelte`.** Change `addUser` to
  `async function addUser(e: SubmitEvent) { e.preventDefault(); try { … } catch … }` (body
  unchanged), add the imports

```ts
  import PasswordInput from '../../lib/PasswordInput.svelte';
  import { Button } from '$lib/components/ui/button/index.js';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass, labelClass } from '$lib/components/ui/field/classes.js';
```

  and replace everything from `<main>` to the end:

```svelte
<main>
  <TopBar title={$t('settings.users')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
    <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
      {#each users as u (u.id)}
        <li data-testid="user-row" class="flex min-h-14 items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 shadow-xs">
          <span class="min-w-0 flex-1 truncate font-medium text-foreground">{u.username}</span>
          {#if u.is_admin}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('settings.user-admin')}</span>{/if}
          {#if u.id !== $user?.id}<Button variant="ghost" class="min-h-11 shrink-0 text-destructive" onclick={() => removeUser(u)}>{$t('settings.user-delete')}</Button>{/if}
        </li>
      {/each}
    </ul>

    <form aria-labelledby="people-new" onsubmit={addUser} class="m-0 flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs">
      <h2 id="people-new" class="m-0 text-base font-semibold text-foreground">{$t('settings.user-new')}</h2>
      <Field id="nu" label={$t('login.username')}>
        <Input bind:value={newName} autocomplete="off" autocapitalize="off" spellcheck="false" />
      </Field>
      <div class="flex flex-col gap-1.5">
        <label for="np" class={labelClass}>{$t('login.password')}</label>
        <PasswordInput id="np" bind:value={newPass} autocomplete="new-password" describedby="np-hint" />
        <p id="np-hint" class={hintClass}>{$t('setup.password-hint')}</p>
      </div>
      <CheckField id="nu-admin" label={$t('settings.user-admin')} bind:checked={newAdmin} />
      <!-- The page's one primary action. -->
      <Button type="submit" class="h-12 self-start" disabled={newName.length < 3 || newPass.length < 8}>{$t('settings.user-new')}</Button>
    </form>
  </div>
</main>
```

  Delete the `<style>` block.

- [ ] **Step 7: `Data.svelte`.** Delete `let message = $state('');`; in `doImport` replace
  `message = $t('settings.import-done', …)` with
  `toast($t('settings.import-done', counts as unknown as Record<string, number>), 8000);`. Add the
  imports

```ts
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { Button } from '$lib/components/ui/button/index.js';
  import { CheckField } from '$lib/components/ui/field/index.js';
  import { errorClass, hintClass } from '$lib/components/ui/field/classes.js';
```

  and `const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';`.
  Replace everything from `<main>` to the end:

```svelte
<main>
  <TopBar title={$t('settings.data')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}

    <section class={card}>
      <CheckField id="export-exclude-body" label={$t('settings.export-exclude-body')} bind:checked={excludeBody} />
      <div class="flex flex-wrap gap-2">
        <!-- The page's one primary action: a link, so the browser downloads it. -->
        <Button href={excludeBody ? '/api/export?exclude_body=true' : '/api/export'} class="h-12">{$t('settings.export')}</Button>
        <Button variant="outline" class="h-12" onclick={() => fileEl.click()}>{$t('settings.import')}</Button>
      </div>
      <input bind:this={fileEl} data-slot="import-file" type="file" accept=".zip,application/zip" hidden
             onchange={(e) => doImport((e.currentTarget as HTMLInputElement).files)} />
      <!-- Beside the buttons, not in the Backup section: the export is what somebody reaches for
           when they mean "keep a copy", and it is not a database backup. -->
      <p class={hintClass}>{$t('settings.export-not-backup')}</p>
    </section>

    <section aria-labelledby="recovery-title" class={card}>
      <h2 id="recovery-title" class="m-0 text-base font-semibold text-foreground">{$t('settings.sync-recovery')}</h2>
      {#if pending > 0}<p class={hintClass}>{$t('settings.sync-pending', { n: pending })}</p>{/if}
      {#if failed.length === 0}
        <p class={hintClass}>{$t('settings.sync-clear')}</p>
      {:else}
        <p class={errorClass}>{$t('settings.sync-failed', { n: failed.length })}</p>
        <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
          {#each failed as op (op.id)}
            <li class="flex flex-wrap items-center gap-2 rounded-md border border-border p-3 text-sm">
              <span class="font-medium text-foreground">{op.kind}</span>
              <span class="min-w-0 flex-[1_1_16rem] text-muted-foreground [overflow-wrap:anywhere]">{op.lastError ?? $t('error.generic')}</span>
              <Button variant="ghost" class="min-h-11 text-destructive" disabled={recoveryBusy} onclick={() => discard(op.id)}>{$t('settings.sync-discard')}</Button>
            </li>
          {/each}
        </ul>
        <Button variant="outline" class="h-12 self-start" disabled={recoveryBusy} onclick={retryFailed}>{$t('settings.sync-retry')}</Button>
      {/if}
    </section>
  </div>
  <Toaster />
</main>
```

  Delete the `<style>` block.

- [ ] **Step 8: Contrast.** In `theme-contrast.test.ts`, inside `'tinted highlight contrast'`, add:

```ts
    // The new API token's card: foreground text on primary/10 over the page.
    it(`${name}: text on the fresh-token card (primary/10 over background) (>= 4.5:1)`, () => {
      expect(contrastRatio(ui('foreground', theme), blend(ui('primary', theme), ui('background', theme), 0.1))).toBeGreaterThanOrEqual(4.5);
    });
```

- [ ] **Step 9: Move the specs** (locator only).
  - `07-api-tokens.spec.ts`: `page.locator('.fresh-token code')` → `page.getByTestId('fresh-token')`;
    `page.locator('.list').getByText(token)` → `page.getByTestId('token-list').getByText(token)`.
  - `14-settings.spec.ts` ("a row carries its current value"):
    `page.locator('.card.row').filter({ has: page.locator('span', { hasText: /^ben\b/ }) })` →
    `page.getByTestId('user-row').filter({ has: page.locator('span', { hasText: /^ben\b/ }) })`.

- [ ] **Step 10: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 39-screens 07-api-tokens 10-database 14-settings 01-smoke 25-offline-cache`
Expected: PASS, both projects (`10-database`'s `getByLabel('Password', { exact: true })` finds only
the field; `25-offline-cache` finds "Sign out" in the signed-in card inside `main` and its
"Signing out needs a connection." line).
Capture `../shots/r5-t6 account,api,people,data`; compare with `r5-before`.

```bash
git add -A frontend
git commit -m "feat: account, API access, users and data pages in the new look; password fields can show their text"
```

---
### Task 7: Pages of explicit actions, part 2 — Database and Types; one primary per page

**Files:**
- Modify: `frontend/src/routes/settings/Database.svelte`, `frontend/src/routes/settings/Types.svelte`,
  `frontend/tests-e2e/24-own-types.spec.ts`, `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Test ids: `type-row`, `icon-choice`, `type-form`.
- Labels and names kept: "Name" (exact), the icon radios by name ("E-bike" …), every category by
  name, "Default counter unit", "Save type", "Cancel", "Add type", "Edit {name}", "Delete {name}",
  "PostgreSQL connection string", "Test connection", "Copy data and switch", "Restart now";
  headings "Database" (h1), "Backup".

- [ ] **Step 1: Failing e2e test.** Append to `39-screens.spec.ts`:

```ts
test('no settings page shows more than one primary button', async ({ page }) => {
  await signIn(page); // the administrator sees every page
  const pages = ['/settings', '/settings/appearance', '/settings/account', '/settings/notifications', '/settings/types',
    '/settings/api', '/settings/data', '/settings/people', '/settings/database'];
  for (const path of pages) {
    await page.goto(path);
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    await page.waitForLoadState('networkidle');
    const primaries = await page.getByRole('main').locator('button, a').evaluateAll((els) => {
      const probe = document.createElement('div');
      probe.style.backgroundColor = 'var(--ui-primary)';
      document.body.append(probe);
      const amber = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return els.filter((e) => (e as HTMLElement).offsetParent !== null && getComputedStyle(e).backgroundColor === amber).length;
    });
    expect(primaries, path).toBeLessThanOrEqual(1);
  }
});
```

Run: `npx playwright test 39-screens -g "more than one primary"` — expected PASS for the pages
Tasks 4–6 migrated and for these two only once Steps 2–4 are done; it is the round's guard (on
0.22.1 it fails on `/settings/notifications`, which showed "Turn on notifications" and "Connect
Telegram" in amber). Run it again in Step 6.

- [ ] **Step 2: `Database.svelte`.** Keep the script; add the imports

```ts
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass } from '$lib/components/ui/field/classes.js';
```

  and at the end of the script

```ts
  const card = 'flex flex-col gap-3 rounded-lg border border-border bg-card p-4 shadow-xs';
  const heading = 'm-0 text-base font-semibold text-foreground';
  /** A host, a file path, an epoch or a driver's message: long, and never cut off. */
  const long = 'm-0 text-sm text-muted-foreground [overflow-wrap:anywhere]';
```

  Replace everything from `<main>` to the end:

```svelte
<main>
  <TopBar title={$t('db.title')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6" aria-busy={db === null}>
    {#if dbError}<p role="alert" class={errorClass}>{dbError}</p>{/if}
    {#if db}
      <section aria-labelledby="db-current" class={card}>
        <h2 id="db-current" class={heading}>{$t('db.current')}</h2>
        <p class="m-0 font-semibold text-foreground">{backendName(db)}</p>
        <p class={long}>{place(db)}</p>
      </section>

      <!-- Above the field, not below it and not in a tooltip: a migrated instance looks healthy
           until somebody opens a photo, and by then the source machine may be gone. Marked in the
           warning colour so it cannot be read as another hint. -->
      <div class={`${card} border-l-4 border-l-warn`}>
        <p class="m-0 font-semibold text-warn">{$t('db.blobs-title')}</p>
        <p class="m-0 text-sm text-foreground">{$t('db.blobs')}</p>
      </div>

      <div class={card}>
        <!-- No placeholder and no hint when the field cannot be used: an example URL in a
             read-only box reads like a value that is already saved. -->
        <Field id="dburl" label={$t('db.url')} hint={canChooseDb ? $t('db.url-hint') : ''}>
          <Input bind:value={dbUrl} placeholder={canChooseDb ? $t('db.url-placeholder') : ''} readonly={!canChooseDb}
                 autocomplete="off" autocapitalize="off" spellcheck="false" />
        </Field>
        {#if !canChooseDb}
          <p class="m-0 text-sm text-muted-foreground"><b class="font-semibold text-foreground">{$t('db.env')}</b> — {$t('db.env-hint')}</p>
        {:else}
          <div class="flex flex-wrap gap-2">
            <Button variant="outline" class="h-12" onclick={testDatabase} disabled={probing || switching || dbUrl.trim().length === 0}>
              {probing ? $t('db.testing') : $t('db.test')}
            </Button>
            <!-- The page's one primary action. -->
            <Button class="h-12" onclick={switchDatabase} disabled={probing || switching || dbUrl.trim().length === 0}>
              {switching ? $t('db.switching') : $t('db.switch')}
            </Button>
          </div>
          {#if switching}<p class={hintClass}>{$t('db.switching-hint')}</p>{/if}
        {/if}
      </div>

      {#if probe}
        <div class={card}>
          <p class="m-0 font-semibold text-foreground">{probe.reachable ? $t('db.reachable') : $t('db.unreachable')}</p>
          {#if probe.version}<p class={long}>{$t('db.version', { version: probe.version })}</p>{/if}
          {#if probe.state === 'empty'}<p class={long}>{$t('db.state-empty')}</p>{/if}
          {#if probe.state === 'holds_logb_data'}<p class={long}>{$t('db.state-holds')}</p>{/if}
          {#if probe.message}<p class={long}>{probe.message}</p>{/if}
        </div>
      {/if}
      {#if switched}
        <div class={card}>
          <p class="m-0 font-semibold text-foreground">{$t('db.switched')}</p>
          <ul role="list" class="m-0 flex list-none flex-col gap-1 p-0 text-sm">
            {#each switched.tables as tb (tb.table)}
              <li class="flex justify-between gap-2"><span class="text-foreground">{tb.table}</span><span class="text-muted-foreground tabular-nums">{$t('db.rows', { rows: tb.rows })}</span></li>
            {/each}
          </ul>
          <p class={long}>{$t('db.epoch', { epoch: switched.epoch })}</p>
          <p class={long}>{$t('db.pointer', { path: switched.pointer })}</p>
        </div>
      {/if}
      {#if db.pending || switched}
        <div class={`${card} border-l-4 border-l-destructive`}>
          <p class="m-0 font-semibold text-foreground">{$t('db.pending')}</p>
          {#if db.pending}<p class={long}>{$t('db.pending-at', { where: `${backendName(db.pending)} · ${place(db.pending)}` })}</p>{/if}
          <!-- Nothing here can check that a supervisor exists, so the button does not promise one. -->
          <p class="m-0 text-sm text-foreground">{$t('db.restart-hint')}</p>
          <Button variant="destructive" class="h-12 self-start" onclick={restartNow} disabled={restarting}>{$t('db.restart')}</Button>
          {#if restartNote}<p class={hintClass}>{restartNote}</p>{/if}
        </div>
      {/if}
    {/if}

    <section aria-labelledby="backup-title" class="flex flex-col gap-2">
      <h2 id="backup-title" class={heading}>{$t('backup.title')}</h2>
      <!-- Three states; on PostgreSQL LogB backs up nothing, said as a division of responsibility
           (accent ink), not as a fault. -->
      {#if backup}
        <div class={[card, backup.state === 'not_ours' && 'border-l-4 border-l-brand-ink']}>
          {#if backup.state === 'scheduled' || backup.state === 'stale'}
            <p class="m-0 font-semibold text-foreground">{$t('backup.scheduled-title')}</p>
            <p class="m-0 text-sm text-foreground [overflow-wrap:anywhere]">{$t('backup.scheduled', { directory: backup.directory ?? '', hour: hourText(backup.hour) })}</p>
            <p class={hintClass}>
              {backup.last_at ? $t('backup.last', { date: fmtDate(backup.last_at, $dateFormat) }) : $t('backup.last-none', { hour: hourText(backup.hour) })}
            </p>
            {#if backup.state === 'stale'}<p class={errorClass}>{$t('backup.stale')}</p>{/if}
          {:else if backup.state === 'off'}
            <p class="m-0 font-semibold text-foreground">{$t('backup.off-title')}</p>
            <p class="m-0 text-sm text-foreground">{$t('backup.off')}</p>
          {:else}
            <p class="m-0 font-semibold text-brand-ink">{$t('backup.not-ours-title')}</p>
            <p class="m-0 text-sm text-foreground">{$t('backup.not-ours')}</p>
            <p class="m-0 text-sm text-foreground">{$t('backup.not-ours-how')}</p>
          {/if}
        </div>
      {/if}
    </section>
  </div>
</main>
```

  Delete the `<style>` block. (`warn` and `brand-ink` as text on a card, and `destructive`/`warn`
  edges against the card, are pairs already tested.)

- [ ] **Step 3: `Types.svelte` script.** Add the imports

```ts
  import { Button } from '$lib/components/ui/button/index.js';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { errorClass, labelClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
```

  and at the end of the script

```ts
  // Icon tiles as TypeTiles draws the object form's: the invisible radio fills its label, which
  // carries the look (brand-ink on primary/10 over card when checked, tested).
  const iconTile = 'relative flex min-h-12 cursor-pointer items-center justify-center rounded-lg border border-input bg-card text-muted-foreground transition-colors hover:not-has-checked:bg-accent has-checked:border-brand-ink has-checked:bg-primary/10 has-checked:text-brand-ink has-focus-visible:outline-2 has-focus-visible:outline-solid has-focus-visible:outline-offset-2 has-focus-visible:outline-ring';
```

- [ ] **Step 4: `Types.svelte` markup.** Replace everything from `{#snippet form()}` to the end:

```svelte
{#snippet form()}
  <form data-testid="type-form" class="m-0 flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs"
        onsubmit={(e) => { e.preventDefault(); void save(); }}>
    <Field id="tn" label={$t('types.name')}>
      <Input bind:value={name} maxlength={40} autocomplete="off" required />
    </Field>
    <fieldset class="m-0 flex min-w-0 flex-col border-0 p-0">
      <legend class={`${labelClass} mb-1.5 p-0`}>{$t('types.icon')}</legend>
      <div class="grid grid-cols-[repeat(auto-fill,minmax(3rem,1fr))] gap-2">
        {#each CUSTOM_TYPE_ICONS as i (i)}
          <label data-testid="icon-choice" class={iconTile} title={iconLabel(i)}>
            <input type="radio" data-slot="icon-radio" name="type-icon" value={i} bind:group={icon}
                   class="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-lg opacity-0" />
            <Icon name={i} size={24} />
            <span class="sr-only">{iconLabel(i)}</span>
          </label>
        {/each}
      </div>
    </fieldset>
    <fieldset class="m-0 flex min-w-0 flex-col border-0 p-0">
      <legend class={`${labelClass} mb-1.5 p-0`}>{$t('types.categories')}</legend>
      <div class="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-3">
        {#each CATEGORIES as c (c)}
          <!-- "Other" fits any entry, so it is always on and cannot be taken off. -->
          <CheckField id={`type-cat-${c}`} label={$t(`cat.${c}`)} disabled={c === 'other'}
                      bind:checked={() => c === 'other' || categories.includes(c), (on) => toggle(c, on)} />
        {/each}
      </div>
    </fieldset>
    <Field id="tu" label={$t('types.unit')}>
      <NativeSelect bind:value={unit}>
        <option value={null}>{$t('types.unit-none')}</option>
        <option value="km">{$t('object.counter-km')}</option>
        <option value="mi">{$t('object.counter-mi')}</option>
        <option value="h">{$t('object.counter-h')}</option>
      </NativeSelect>
    </Field>
    {#if formError}<p role="alert" class={errorClass}>{formError}</p>{/if}
    <div class="flex gap-2">
      <!-- While the form is open, "Add type" is hidden: this is the page's one primary. -->
      <Button type="submit" class="h-12 flex-1 sm:min-w-28 sm:flex-none" disabled={busy || name.trim() === ''}>{$t('types.save')}</Button>
      <Button variant="outline" class="h-12 flex-1 sm:min-w-28 sm:flex-none" onclick={cancelForm}>{$t('nav.cancel')}</Button>
    </div>
  </form>
{/snippet}

<main>
  <TopBar title={$t('settings.types')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    <section class="flex flex-col gap-2">
      <h2 id="types-yours" class={sectionHeadingClass}>{$t('types.yours')}</h2>
      <ul role="list" aria-labelledby="types-yours" class="m-0 flex list-none flex-col gap-2 p-0">
        {#each $customTypes as ty (ty.id)}
          <li>
            {#if editing === ty.id}
              {@render form()}
            {:else}
              <div data-testid="type-row" class="grid grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-2 rounded-lg border border-border bg-card p-3 shadow-xs">
                <span class="grid size-10 place-items-center rounded-md bg-primary/10 text-brand-ink" aria-hidden="true"><Icon name={ty.icon} /></span>
                <span class="flex min-w-0 flex-col">
                  <b class="font-semibold text-foreground [overflow-wrap:anywhere]">{ty.name}</b>
                  <span class="truncate text-sm text-muted-foreground">{summary(ty)}</span>
                </span>
                <Button variant="ghost" class="min-h-11" aria-label={$t('types.edit-named', { name: ty.name })} onclick={() => open(ty)}>{$t('nav.edit')}</Button>
                <Button variant="ghost" class="min-h-11 text-destructive" aria-label={$t('types.delete-named', { name: ty.name })} onclick={() => remove(ty)}>{$t('types.delete')}</Button>
                {#if deleteError?.id === ty.id}<p role="alert" class={`${errorClass} col-span-full`}>{deleteError.message}</p>{/if}
              </div>
            {/if}
          </li>
        {:else}
          {#if editing !== 'new'}<li class="px-1 py-4 text-sm text-muted-foreground">{$t('types.empty')}</li>{/if}
        {/each}
      </ul>
      {#if editing === 'new'}
        {@render form()}
      {:else}
        <!-- The page's one primary action. -->
        <Button class="h-12 self-start" onclick={() => open('new')}>{$t('types.add')}</Button>
      {/if}
    </section>

    <section class="flex flex-col gap-2">
      <h2 id="types-built-in" class={sectionHeadingClass}>{$t('types.built-in')}</h2>
      <ul role="list" aria-labelledby="types-built-in" class="m-0 grid list-none grid-cols-1 gap-x-4 rounded-lg border border-border bg-card p-3 shadow-xs sm:grid-cols-2">
        {#each OBJECT_TYPES as ty (ty)}
          <li class="flex min-h-11 items-center gap-3 text-foreground"><span class="text-muted-foreground" aria-hidden="true"><Icon name={typeIcon(ty, [])} /></span>{$t(`type.${ty}`)}</li>
        {/each}
      </ul>
    </section>
  </div>
</main>
```

  Delete the `<style>` block. `toggle(c, on)` and `open`, `save`, `cancelForm`, `remove` stay as
  they are.

- [ ] **Step 5: Move `24-own-types`** (locator only):
  `await page.locator('.icon-choice').filter({ has: eBike }).click();` → `await eBike.check();`
  (keep its comment, reworded: "The radio fills its icon tile, invisibly; checking it is the tap.");
  both `page.locator('.type-row', { hasText: 'E-Scooter' })` →
  `page.getByTestId('type-row').filter({ hasText: 'E-Scooter' })`.

- [ ] **Step 6: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 39-screens 10-database 24-own-types 14-settings`
Expected: PASS, both projects (`10-database`: one heading matches /Database/, one text matches
/SQLite/, one heading /Backup/, "LOGB_BACKUP_DIR" shown).
Capture `../shots/r5-t7 types,database`; compare with `r5-before`.

```bash
git add -A frontend
git commit -m "feat: database and types pages in the new look; at most one primary button per settings page"
```

---
### Task 8: Sign-in and setup — a centred card, lazy, with a password toggle

**Files:**
- Modify: `frontend/src/App.svelte`, `frontend/src/routes/Login.svelte`,
  `frontend/src/routes/Setup.svelte`, `frontend/src/lib/Logo.svelte`,
  `frontend/src/lib/components/ui/field/classes.ts`, `frontend/src/app.css`,
  `frontend/src/i18n/en.ts`, `de.ts`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests-e2e/13-shell.spec.ts`, `frontend/tests-e2e/39-screens.spec.ts`

**Interfaces:**
- Produces: `primaryButtonClass` in `field/classes.ts` (a filled amber button as a plain string);
  routes `/login` and `/setup` in App.svelte's lazy route table; `data-testid="auth-form"`.
- Labels and names kept: "Username"/"Benutzername" (`#u`), "Password"/"Passwort" (`#p`),
  "Sign in", "Create admin", heading "Sign in", the logo `img` named "LogB".
- Constraint: Login and Setup import only `field/classes.ts`, `PasswordInput`, `Logo`, the router,
  i18n, the session store, `api`/`api-error`. No `cn`, no `$lib/components/ui/*` components.

- [ ] **Step 1: i18n.** `en.ts`: `'setup.username-hint': 'At least 3 characters.',`; `de.ts`:
  `'setup.username-hint': 'Mindestens 3 Zeichen.',`.

- [ ] **Step 2: Failing e2e test.** Append to `39-screens.spec.ts`:

```ts
test('signing in is a centred card, and the password can be shown while typing', async ({ page }) => {
  // A fresh instance lands on setup instead; both screens share the card and the field.
  await page.goto('/login');
  await expect(page.getByTestId('auth-form')).toBeVisible();
  const password = page.getByLabel(/^(Password|Passwort)$/);
  await password.fill('secret words');
  await expect(password).toHaveAttribute('type', 'password');
  const toggle = page.getByRole('button', { name: 'Show password' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await expect(password).toHaveAttribute('type', 'text');
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(password).toHaveValue('secret words');
  const box = (await toggle.boundingBox())!;
  expect(box.width).toBeGreaterThanOrEqual(44);
  expect(box.height).toBeGreaterThanOrEqual(44);
});
```

Run: `npx playwright test 39-screens -g "centred card"` — FAIL (no `auth-form`).

- [ ] **Step 3: `field/classes.ts`.** Append:

```ts
/** A filled amber button as a plain class string, for screens that must not load the Button
 *  component (Login and Setup, the first screen of a fresh install). The ring sits on the page,
 *  off the fill. */
export const primaryButtonClass =
  'inline-flex h-12 w-full cursor-pointer items-center justify-center rounded-lg bg-primary px-4 text-base font-semibold text-primary-foreground transition-colors hover:bg-primary/80 disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
```

  In `theme-contrast.test.ts`, inside `'tinted highlight contrast'`, add:

```ts
    it(`${name}: text on a filled button under the pointer (primary/80 over card) (>= 4.5:1)`, () => {
      expect(contrastRatio(ui('primary-foreground', theme), blend(ui('primary', theme), ui('card', theme), 0.8))).toBeGreaterThanOrEqual(4.5);
    });
```

- [ ] **Step 4: `Logo.svelte`** (replace the markup and delete the `<style>` block; the script
  stays):

```svelte
<!-- Labelled rather than decorative by default: on the auth card this is the only thing naming
     the application. Next to the sidebar wordmark it would say "LogB" twice, so that caller
     passes `decorative`. Amber ink, not the amber fill: the fill is 2.1:1 on the page. -->
<svg
  class={['shrink-0 text-brand-ink', inline ? 'inline-block' : 'block']}
  viewBox="8 8 48 48"
  width={size}
  height={size}
  role={decorative ? undefined : 'img'}
  aria-label={decorative ? undefined : 'LogB'}
  aria-hidden={decorative ? 'true' : undefined}
>{@html mark}</svg>
```

- [ ] **Step 5: `Login.svelte`** (replace the markup; the script keeps its state and `submit`, and
  its imports become):

```ts
  import { go } from '../lib/router';
  import Logo from '../lib/Logo.svelte';
  import PasswordInput from '../lib/PasswordInput.svelte';
  import { controlClass, errorClass, labelClass, primaryButtonClass } from '$lib/components/ui/field/classes.js';
  import { t } from '../i18n';
  import { login } from '../stores/session';
```

```svelte
<!-- Signed out, so no shell: a card in the middle of the screen both ways. `min-h-dvh`, not a
     fixed height, so a phone with the keyboard up scrolls instead of clipping. Plain class
     strings only: this is its own small chunk, and the component library stays out of it. -->
<main class="mx-auto flex min-h-dvh w-full max-w-sm flex-col justify-center px-4 py-8">
  <div class="flex flex-col gap-6 rounded-xl border border-border bg-card p-6 shadow-sm">
    <div class="flex flex-col items-center gap-3 text-center">
      <Logo size={56} />
      <h1 tabindex="-1" class="m-0 text-2xl font-semibold tracking-tight text-foreground focus:outline-none">{$t('login.title')}</h1>
    </div>
    <form data-testid="auth-form" onsubmit={submit} class="m-0 flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="u" class={labelClass}>{$t('login.username')}</label>
        <input id="u" data-slot="auth-input" class={controlClass} bind:value={username} autocomplete="username" autocapitalize="off" spellcheck="false" required />
      </div>
      <div class="flex flex-col gap-1.5">
        <label for="p" class={labelClass}>{$t('login.password')}</label>
        <PasswordInput id="p" bind:value={password} autocomplete="current-password" required />
      </div>
      {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
      <button data-slot="auth-submit" disabled={busy} class={primaryButtonClass}>{$t('login.submit')}</button>
    </form>
  </div>
</main>
```

- [ ] **Step 6: `Setup.svelte`** (same imports as Login plus `api`, `errorMessage`,
  `loadSession`, minus `login`; the script keeps its state and `submit`). Replace the markup:

```svelte
<!-- The first screen of a fresh install: the same card as sign-in. -->
<main class="mx-auto flex min-h-dvh w-full max-w-sm flex-col justify-center px-4 py-8">
  <div class="flex flex-col gap-6 rounded-xl border border-border bg-card p-6 shadow-sm">
    <div class="flex flex-col items-center gap-3 text-center">
      <Logo size={56} />
      <h1 tabindex="-1" class="m-0 text-2xl font-semibold tracking-tight text-foreground focus:outline-none">{$t('setup.title')}</h1>
      <p class="m-0 text-sm text-muted-foreground">{$t('setup.intro')}</p>
    </div>
    <form data-testid="auth-form" onsubmit={submit} class="m-0 flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="u" class={labelClass}>{$t('login.username')}</label>
        <input id="u" data-slot="auth-input" class={controlClass} bind:value={username} autocomplete="username" autocapitalize="off" spellcheck="false"
               required minlength="3" aria-describedby="u-hint" />
        <p id="u-hint" class={hintClass}>{$t('setup.username-hint')}</p>
      </div>
      <div class="flex flex-col gap-1.5">
        <label for="p" class={labelClass}>{$t('login.password')}</label>
        <PasswordInput id="p" bind:value={password} autocomplete="new-password" required minlength={8} describedby="p-hint" />
        <p id="p-hint" class={hintClass}>{$t('setup.password-hint')}</p>
      </div>
      {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
      <button data-slot="auth-submit" disabled={busy} class={primaryButtonClass}>{$t('setup.submit')}</button>
    </form>
  </div>
</main>
```

  (Setup's class import also takes `hintClass`.)

- [ ] **Step 7: `App.svelte` — Login and Setup become lazy.**
  - Delete `import Setup from './routes/Setup.svelte';` and `import Login from './routes/Login.svelte';`.
  - In `routes`, after `['/', eager(Dashboard)],` add:

```ts
    // Signed out only: their own small chunk, so a signed-in start never downloads them.
    ['/login', () => import('./routes/Login.svelte')],
    ['/setup', () => import('./routes/Setup.svelte')],
```

  - Replace

```svelte
{#if $user === undefined}
  <main><p class="muted">{$t('nav.loading')}</p></main>
{:else if $path === '/setup'}
  <Setup />
{:else if $path === '/login'}
  <Login />
{:else if $user}
```

    with

```svelte
{#if $user === undefined}
  <main class="p-3"><p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p></main>
{:else if $path === '/setup' || $path === '/login'}
  <!-- The same loader as every page: a failed chunk reloads once, "Loading…" only if slow. -->
  {#if current && page?.pattern === current.pattern}
    {@const Page = page.comp}
    <Page />
  {:else if slow}
    <main class="p-3"><p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p></main>
  {/if}
{:else if $user}
```

- [ ] **Step 8: app.css.** Delete the `main.auth { … }`, `main.auth > h1, main.auth > p`,
  `main.auth > p`, `main.auth form`, `main.auth form button.primary` rules and their comment.

- [ ] **Step 9: `13-shell.spec.ts`** (locator only): `page.locator('main.auth form')` →
  `page.getByTestId('auth-form')`.

- [ ] **Step 10: Verify, measure, look, commit**

```bash
cd frontend && npm run check && npm test && npm run build
gzip -9c dist/assets/index-*.js | wc -c
for f in dist/assets/Login-*.js dist/assets/Setup-*.js; do echo "$f $(gzip -9c "$f" | wc -c)"; grep -o 'from"./[^"]*"' "$f"; done
grep -l 'twMerge\|tailwind-merge\|bits-ui' dist/assets/Login-*.js dist/assets/Setup-*.js
```

Expected: the entry chunk ≤ 11,988 B (about 11.0–11.2 KB); each auth chunk under 2 KB, importing
only small shared chunks (`index`, the classes chunk, `PasswordInput`/`Logo` if split out); the last
`grep` prints nothing. If an auth chunk statically imports the `field-*` chunk (bits-ui), the
bundler has put `classes.ts` into it: stop and report the import list rather than work around it.

Run: `npx playwright test 39-screens 13-shell 01-smoke 04-offline 10-database 25-offline-cache`
Expected: PASS, both projects.
Capture `../shots/r5-t8 login,dashboard`; compare `02-login` with `r5-before` (card, logo in amber
ink, eye button) and `03-dashboard` (sidebar wordmark).

```bash
git add -A frontend
git commit -m "feat: sign-in and setup as a centred card with a password toggle, loaded only when signed out"
```

---
### Task 9: The last screens off app.css, then app.css deleted

**Files:**
- Modify: `frontend/src/lib/TopBar.svelte`, `frontend/src/lib/UpdateBanner.svelte`,
  `frontend/src/lib/WeightHistory.svelte`, `frontend/src/lib/Reminders.svelte`,
  `frontend/src/lib/Documents.svelte`, `frontend/src/routes/Dashboard.svelte`,
  `frontend/src/App.svelte`, `frontend/src/app.tw.css`, `frontend/tests/scale.test.ts`,
  `frontend/tests/theme-contrast.test.ts`, `frontend/tests-e2e/08-foundations.spec.ts`
- Delete: `frontend/src/app.css`

**Interfaces:**
- Consumes: everything above. Eager files (TopBar, UpdateBanner, Dashboard, App) use plain class
  strings written out in place.
- Produces: `app.tw.css` with `@layer theme, base, components, utilities;` and no `legacy` layer.

- [ ] **Step 1: Failing unit test.** Replace `frontend/tests/scale.test.ts` with:

```ts
import { describe, expect, it } from 'vitest';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';

const SRC = fileURLToPath(new URL('../src', import.meta.url));

function svelteFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const full = join(dir, e.name);
    return e.isDirectory() ? svelteFiles(full) : e.name.endsWith('.svelte') ? [full] : [];
  });
}

/**
 * Styling is Tailwind utilities and the tokens and primitives in app.tw.css, nothing else. A
 * scoped `<style>` block is unlayered and beats every utility, so one left behind silently
 * overrides the class list beside it. The checks this file used to hold kept those blocks on
 * app.css's spacing, type and radius scale; the blocks and the scale are both gone.
 */
describe('styling', () => {
  it('no component carries a <style> block', () => {
    const withStyle = svelteFiles(SRC).filter((f) => /<style[\s>]/.test(readFileSync(f, 'utf8'))).map((f) => relative(SRC, f));
    expect(withStyle).toEqual([]);
  });

  it('app.css is gone, and app.tw.css keeps no layer or mention of it', () => {
    expect(existsSync(join(SRC, 'app.css'))).toBe(false);
    expect(readFileSync(join(SRC, 'app.tw.css'), 'utf8')).not.toMatch(/legacy|app\.css/);
  });

  it('no markup uses a class name app.css used to define', () => {
    const old = /class="(?:[^"]*\s)?(field|row|toggle|hint|warn|warning|error|card|list|muted|empty|empty-icon|chip|banner|button-like|primary|danger|ghost|tnum|small|fab|topbar|auth|settings-grid|thumb-grid)(?:\s[^"]*)?"|class:(primary|ghost|danger)=/;
    const hits = svelteFiles(SRC).filter((f) => old.test(readFileSync(f, 'utf8'))).map((f) => relative(SRC, f));
    expect(hits).toEqual([]);
  });
});
```

Run: `npx vitest run tests/scale.test.ts` — FAIL (TopBar, UpdateBanner, WeightHistory keep
`<style>`; app.css exists; Dashboard, Reminders, Documents, App use old classes).

- [ ] **Step 2: `TopBar.svelte`** (eager). Replace the markup from `<header class="topbar">` to
  the end, and delete the `<style>` block:

```svelte
<header class="sticky top-0 z-[5] flex items-center gap-2 bg-background py-2">
  {#if backTo !== null}
    <button data-slot="topbar-back" aria-label={$t('nav.back')} onclick={() => (backTo ? go(backTo) : back())}
            class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"><Icon name="back" /></button>
  {/if}
  <!-- tabindex -1: focusable by script only (`focusPageHeading` in ./router.ts), no Tab stop and
       no ring -- it is not a control. -->
  <div class="min-w-0 flex-1">
    <h1 tabindex="-1" class="m-0 flex min-w-0 items-center gap-2 text-xl font-semibold tracking-tight focus:outline-none desk:text-2xl">{#if icon}<Icon name={icon} />{/if}<span class="truncate">{title}</span></h1>
    {#if subtitle}<p class="m-0 truncate text-sm text-muted-foreground">{subtitle}</p>{/if}
  </div>
  <!-- In offline mode what shows is what was cached -- worth saying. `servingSaved` covers the
       other way this happens (see `servedFromCache` in ./api.ts). -->
  {#if $offline || $servingSaved}<span role="status" class="min-w-0 shrink truncate text-xs whitespace-nowrap text-muted-foreground">{$t('nav.offline-mode')}</span>{/if}
  {#if pending > 0}<span class="shrink-0 rounded-full bg-warn px-3 py-0.5 text-xs whitespace-nowrap text-background">{$t('outbox.pending', { n: pending })}</span>{/if}
  {#if dead > 0}<span class="shrink-0 rounded-full bg-destructive px-3 py-0.5 text-xs whitespace-nowrap text-destructive-foreground">{$t('outbox.dead-chip', { n: dead })}</span>{/if}
  {#if children}{@render children()}{/if}
  <AccountMenu />
</header>
```

- [ ] **Step 3: `UpdateBanner.svelte`** (eager, mounted by main.ts). Replace everything after
  `</script>`:

```svelte
<!-- A new build is waiting (see ./sw-update.ts). Offered, never forced: reloading on its own used
     to throw away a half-filled form. Mounted beside the app by main.ts, so it shows on every
     screen, the sign-in card included. -->
{#if $updateReady}
  <div role="status"
       class="fixed inset-x-4 top-[calc(0.5rem+env(safe-area-inset-top))] z-20 mx-auto flex max-w-[560px] flex-wrap items-center gap-2 rounded-lg border border-border bg-popover px-3 py-2 text-popover-foreground shadow-lg">
    <span class="min-w-0 flex-[1_1_12rem] text-sm">{$t('app.update-ready')}</span>
    <button data-slot="update-later" onclick={dismissUpdate}
            class="inline-flex min-h-11 shrink-0 cursor-pointer items-center rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('app.update-later')}</button>
    <button data-slot="update-reload" onclick={() => void applyUpdate()}
            class="inline-flex min-h-11 shrink-0 cursor-pointer items-center rounded-md bg-primary px-3 text-sm font-semibold text-primary-foreground hover:bg-primary/80 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('app.update-reload')}</button>
  </div>
{/if}
```

- [ ] **Step 4: `Dashboard.svelte`** (eager; plain strings, no `classes.ts`).
  - Both `<p class="error">` → `<p class="m-0 mb-3 text-sm font-medium text-destructive">`.
  - Both `<p class="muted">` (loading, no match) → `<p class="m-0 text-sm text-muted-foreground">`.
  - The empty state:

```svelte
    <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <span class="text-muted-foreground opacity-40" aria-hidden="true"><Icon name="object" size={40} /></span>
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('dash.empty')}</p>
      <button data-slot="dash-action" onclick={() => go('/objects/new')}
              class="inline-flex h-12 cursor-pointer items-center justify-center rounded-lg bg-primary px-4 text-base font-semibold text-primary-foreground hover:bg-primary/80 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">+ {$t('dash.new')}</button>
    </div>
```

  - `<div class="empty"><p>{$t('dash.none-archived')}</p></div>` →
    `<div class="flex flex-col items-center gap-3 px-4 py-10 text-center"><p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('dash.none-archived')}</p></div>`.

- [ ] **Step 5: `App.svelte`.** Inside `.app-content`:
  `{#if slow}<main><p class="muted">{$t('nav.loading')}</p></main>{/if}` →
  `{#if slow}<main><p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p></main>{/if}`;
  the 404 `main` →

```svelte
        <main>
          <p class="m-0 mb-2 text-sm text-muted-foreground">404</p>
          <a data-slot="not-found-home" href="/" class="text-brand-ink underline underline-offset-4" onclick={(e) => { e.preventDefault(); go('/'); }}>{$t('dash.title')}</a>
        </main>
```

- [ ] **Step 6: `Reminders.svelte`** (lazy). Add
  `import { Field } from '$lib/components/ui/field/index.js';` and
  `import { NativeSelect } from '$lib/components/ui/native-select/index.js';`.
  - `{#if error}<p class="error" role="alert">{error}</p>{/if}` →
    `{#if error}<p role="alert" class="m-0 mb-3 text-sm font-medium text-destructive">{error}</p>{/if}`.
  - Replace the whole `<dialog …>…</dialog>`:

```svelte
<dialog bind:this={dialog} aria-labelledby="reminder-done-title"
        class="m-auto w-[min(92vw,28rem)] rounded-xl border border-border bg-popover p-0 text-popover-foreground shadow-xl backdrop:bg-black/45">
  <div class="flex flex-col gap-4 p-4">
    <h2 id="reminder-done-title" class="m-0 text-lg font-semibold">{$t('reminder.done-title')}</h2>
    {#if doneError}<p role="alert" class="m-0 text-sm font-medium text-destructive">{doneError}</p>{/if}
    <Field id="link" label={$t('reminder.done-link')}>
      <NativeSelect bind:value={linkId}>
        <option value="">{$t('reminder.done-none')}</option>
        {#each linkable as a (a.id)}
          <option value={String(a.id)}>{fmtDate(a.date, $dateFormat)} — {activityTitle(a.title, a.category, $t)}</option>
        {/each}
      </NativeSelect>
    </Field>
    <div class="flex justify-end gap-2">
      <Button variant="outline" class="h-12" onclick={() => dialog?.close()}>{$t('nav.cancel')}</Button>
      <Button class="h-12" onclick={confirmDone} disabled={doneBusy}>{$t('reminder.done')}</Button>
    </div>
  </div>
</dialog>
```

- [ ] **Step 7: `Documents.svelte`.** `{#if error}<p class="error" role="alert">{error}</p>{/if}` →
  `{#if error}<p role="alert" class="m-0 mb-3 text-sm font-medium text-destructive">{error}</p>{/if}`.

- [ ] **Step 8: `WeightHistory.svelte`** (lazy, object page). Add the imports

```ts
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
```

  and at the end of the script

```ts
  const label = 'text-sm text-muted-foreground';
  const figure = 'text-lg font-semibold text-foreground tabular-nums';
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
```

  Replace everything from `<section class="weight-history"` to the end (the `<style>` block
  included):

```svelte
<section aria-label={$t('weight.history')} class="my-3 flex flex-col gap-3 rounded-lg border border-border bg-card p-3 shadow-xs">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <h2 class="m-0 text-base font-semibold text-foreground">{$t('weight.history')}</h2>
    <Button class="min-h-11" onclick={() => go(`/objects/${objectId}/activities/new?category=weight`)}>{$t('weight.log')}</Button>
  </div>
  {#if latest}
    <div class="flex flex-wrap gap-x-6 gap-y-2">
      <div class="flex flex-col gap-0.5">
        <span class={label}>{$t('weight.latest')}</span>
        <strong class={figure}>{formatWeight(latest.weight_grams, unit, $locale)}</strong>
        <span class={label}>{fmtDate(latest.date, $dateFormat)}{#if latest.pending} · {$t('weight.pending')}{/if}</span>
      </div>
      {#if previous}
        <div class="flex flex-col gap-0.5">
          <span class={label}>{$t('weight.change')}</span>
          <strong class={figure}>{latest.weight_grams > previous.weight_grams ? '+' : ''}{formatWeight(latest.weight_grams - previous.weight_grams, unit, $locale)}</strong>
        </div>
      {/if}
      {#if summary?.average_grams != null}
        <div class="flex flex-col gap-0.5">
          <span class={label}>{$t('weight.average')}</span>
          <strong class={figure}>{formatWeight(summary.average_grams, unit, $locale)}</strong>
        </div>
      {/if}
    </div>
  {:else if loaded && !failed}
    <p class={`m-0 ${label}`}>{$t('weight.empty')}</p>
  {:else if !loaded}
    <p class={`m-0 ${label}`}>{$t('nav.loading')}</p>
  {/if}
  {#if failed}<p role="status" class="m-0 text-sm text-foreground">{$t('weight.unavailable')}</p>{/if}
  {#if all.length > 0}
    <!-- The dashboard's segmented control: the chosen range on a card-coloured segment. -->
    <div role="group" aria-label={$t('weight.history')} class="flex w-fit gap-1 rounded-md bg-muted p-1">
      {#each [1, 3, 0] as months (months)}
        <button type="button" data-slot="weight-range" aria-pressed={range === months}
                onclick={() => { range = months as 1 | 3 | 0; selected = null; }}
                class={['min-h-11 cursor-pointer rounded px-3 text-sm font-medium', focus, range === months ? 'bg-card text-foreground shadow-xs' : 'text-muted-foreground hover:text-foreground']}>
          {$t(months === 1 ? 'weight.month' : months === 3 ? 'weight.three-months' : 'weight.all')}
        </button>
      {/each}
    </div>
    {#if visible.length}
      <svg viewBox="0 0 500 185" role="img" aria-label={$t('weight.history')} class="block max-h-[230px] w-full text-brand-ink">
        <title>{$t('weight.history')} ({unit})</title>
        <text x="4" y="38" class="fill-muted-foreground text-xs">{weightValue(high, unit).toFixed(1)}</text>
        {#if high !== low}<text x="4" y="148" class="fill-muted-foreground text-xs">{weightValue(low, unit).toFixed(1)}</text>{/if}
        <polyline points={line} fill="none" stroke="currentColor" stroke-width="2" />
        {#each visible as point (point.id)}
          <circle cx={x(point)} cy={y(point)} r={focused?.id === point.id ? 6 : 4} fill="currentColor" />
        {/each}
        <text x="48" y="178" class="fill-muted-foreground text-xs">{fmtDate(visible[0].date, $dateFormat)}</text>
        {#if visible.length > 1}<text x="458" y="178" text-anchor="end" class="fill-muted-foreground text-xs">{fmtDate(visible[visible.length - 1].date, $dateFormat)}</text>{/if}
      </svg>
      <Field id="weight-point-chooser" label={$t('weight.chart-hint')}>
        <NativeSelect bind:value={() => focused?.id, (v) => (selected = Number(v))}>
          {#each visible as point (point.id)}<option value={point.id}>{fmtDate(point.date, $dateFormat)} · {formatWeight(point.weight_grams, unit, $locale)}{point.pending ? ` · ${$t('weight.pending')}` : ''}</option>{/each}
        </NativeSelect>
      </Field>
      <ol aria-label={$t('weight.history')} class="m-0 flex list-none flex-col gap-1 p-0">
        {#each visible as point (point.id)}
          <li>
            <button type="button" data-slot="weight-point" aria-label={$t('weight.chart-point')} onclick={() => (selected = point.id)}
                    class={['flex min-h-11 w-full cursor-pointer items-center justify-between gap-2 rounded-md px-3 text-left text-sm text-foreground hover:bg-accent', focus, focused?.id === point.id && 'outline-2 outline-solid outline-brand-ink']}>
              <span>{fmtDate(point.date, $dateFormat)}</span>
              <span><strong class="font-semibold tabular-nums">{formatWeight(point.weight_grams, unit, $locale)}</strong>{#if point.pending}<span class="text-muted-foreground"> · {$t('weight.pending')}</span>{/if}</span>
            </button>
          </li>
        {/each}
      </ol>
    {:else}
      <p class={`m-0 ${label}`}>{$t('weight.no-range')}</p>
    {/if}
  {/if}
  <Button variant="outline" class="min-h-11 self-start" onclick={() => go(`/objects/${objectId}/reminders/new?kind=reading`)}>{$t('weight.reminder')}</Button>
</section>
```

- [ ] **Step 9: Delete app.css and the layer.**

```bash
git rm frontend/src/app.css
```

  In `frontend/src/app.tw.css` replace the opening comment, the `@layer` statement and the three
  `@import` lines with:

```css
/* The stylesheet entry: Tailwind, the Inter font, and LogB's tokens (shadcn's names as `--ui-*`,
   the tag hues, the shell's measures). Screens are styled with utilities; the few rules below --
   the page column, the tag chips, the floating-button position -- are shared primitives. The
   first @layer statement fixes the order; the imports only fill the layers. */
@layer theme, base, components, utilities;
@import 'tailwindcss';
@import '@fontsource-variable/inter/wght.css';
```

  and reword the three comments that still name the old file:
  - above `:root {`: `/* shadcn's tokens, prefixed `--ui-*` (the plain names collided with the first stylesheet's). Every pair is checked by tests/theme-contrast.test.ts. */`
  - on `--ui-warn`: `/* Warning text (a counter lower than the last one). */`
  - in `@theme`: `/* The shell's breakpoint (`.app-content` below), so `desk:` and `max-desk:` switch exactly where the sidebar does. Tailwind writes these as range queries, which the shell requires. */`

  Then `grep -rn 'app\.css' frontend/src frontend/index.html` — reword any comment still pointing
  at it to the rule's new home (`app.tw.css` or the component), so the grep prints nothing.

- [ ] **Step 10: `theme-contrast.test.ts`.** Delete the app.css reader and its describe: the
  `/** WCAG 2.1 AA for the colour pairs the stylesheet itself sets … */` comment, the `css`,
  `block`, `light`, `dark`, `token`, `rule` and `varIn` constants, and
  `describe('theme contrast', …)`. Every pair it checked has a `--ui-*` twin already tested below
  (control border = `input`, amber text = `brand-ink`, focus = `ring`, muted, warn,
  primary-foreground on primary), except the TopBar chips: in `describe('shadcn token contrast')`'s
  4.5:1 list add `['background', 'warn']` with the comment
  `// TopBar's "waiting to send" chip; "could not be sent" is destructive-foreground on destructive.`

Run: `npx vitest run` — PASS (scale, theme-contrast, tags, theme, and the rest).

- [ ] **Step 11: `08-foundations.spec.ts`** (behaviour: no `.primary` class, no `--focus`). Replace
  everything from the comment `// A regression that drops \`var(--focus)\`` to the end of the test
  with:

```ts
  // A regression that drops the ring's colour -- bare `outline: 2px solid` -- resolves to
  // `currentColor` and would still pass the checks above. The one place that coincidence does not
  // hold is a filled amber button, whose text colour is far from the ring, and it is also the
  // control the offset exists for: tab to one (the empty dashboard's "+ New object") and pin the
  // ring colour there. Both colours are resolved from the live tokens, not hard-coded.
  const resolve = (value: string, prop: 'color' | 'backgroundColor') => page.evaluate(([v, p]) => {
    const probe = document.createElement('div');
    probe.style[p] = v;
    document.body.appendChild(probe);
    const out = getComputedStyle(probe)[p];
    probe.remove();
    return out;
  }, [value, prop] as const);
  const amber = await resolve('var(--ui-primary)', 'backgroundColor');

  let onAccentButton = false;
  for (let i = 0; i < 40 && !onAccentButton; i++) {
    await page.keyboard.press('Tab');
    onAccentButton = await page.evaluate((fill) => {
      const el = document.activeElement as HTMLElement | null;
      return !!el && el.tagName === 'BUTTON' && getComputedStyle(el).backgroundColor === fill;
    }, amber);
  }
  expect(onAccentButton, 'expected to reach a filled amber button by tabbing').toBe(true);

  const outline = await page.evaluate(() => getComputedStyle(document.activeElement as HTMLElement).outlineColor);
  expect(outline, "the amber button's ring is the ring token, not currentColor").toBe(await resolve('var(--ui-ring)', 'color'));
});
```

  Also reword the comment above the three ring assertions: "What the app's rule in app.css
  actually adds" → "What the app's rule (app.tw.css's base layer, and every component's
  `focus-visible:` utilities) adds".

- [ ] **Step 12: Gates.**

```bash
cd frontend
grep -rln "<style" src                                         # nothing
grep -rn "var(--\(bg\|surface\|surface-2\|text\|muted\|accent\|accent-text\|accent-ink\|danger\|warn\|on-danger\|on-warn\|border\|control-border\|shadow\|focus\|transition\|space-[0-9]\|radius-full\))" src   # nothing
grep -rn "legacy" src/app.tw.css                               # nothing
grep -rln "components/ui\|lib/utils\|autosave\|toast\|PasswordInput\|field/classes" src/App.svelte src/main.ts src/routes/Dashboard.svelte src/lib/TopBar.svelte src/lib/AppNav.svelte src/lib/AppFooter.svelte src/lib/SignedIn.svelte src/lib/AccountMenu.svelte src/lib/LogPicker.svelte src/lib/ObjectCard.svelte src/lib/TagChips.svelte src/lib/DashboardReminders.svelte src/lib/UpdateBanner.svelte src/lib/Logo.svelte src/lib/Icon.svelte   # nothing
```

- [ ] **Step 13: Look at everything.** Build, capture every screen, and compare each with
  `shots/r5-before` and the task captures:

```bash
cd frontend && npm run build && cd .. && cargo build
rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &
sleep 2 && (cd frontend && node scripts/shots.mjs ../shots/r5-t9); kill %1
```

  What changes with the revert block gone, and how to fix it where it shows:
  - Line height 1.5 for text without a `text-*` utility (Decision 12): a row that grew past its
    design (timeline entry, reminder card, object card) gets `leading-snug` on that text.
  - Headings without a size utility fall to the body size: add the size (`text-base font-semibold`
    for sections, `text-lg` for dialogs).
  - Inline `svg`/`img` are now `display: block`: an icon that was meant to sit in a line of text
    gets `inline-block align-middle` (or its wrapper `inline-flex`).
  - Lists lose bullets and padding everywhere: already set explicitly by rounds 2–5.
  - The browser's link underline and colour on unclassed `<a>`: none should remain (Step 12).
  Record every fix in the task report with the screen it was for.

- [ ] **Step 14: Verify and commit**

Run: `npm run check && npm test && npm run e2e`
Expected: PASS, both projects, the whole suite.

```bash
git add -A frontend
git commit -m "refactor: delete app.css and the legacy layer; top bar, update banner, weight history, reminders dialog in utilities"
```

---
### Task 10: Amber app icon, and Save above the on-screen keyboard

**Files:**
- Modify: `frontend/public/icon.svg`, `frontend/public/pwa-192.png`, `pwa-512.png`,
  `pwa-512-maskable.png`, `apple-touch-icon.png` (regenerated), `frontend/scripts/icon.sha256`,
  `frontend/scripts/render-icons.sh`, `frontend/src/lib/Logo.svelte`, `frontend/index.html`,
  `frontend/tests/pwa-icons.test.ts`, `frontend/tests-e2e/39-screens.spec.ts`
- Create: `frontend/tests/viewport.test.ts`

**Interfaces:**
- Produces: the mark's ink `#1c1300` on an `#f59e0b` plate; `Logo.svelte` swaps `#1c1300` for
  `currentColor`; viewport meta with `interactive-widget=resizes-content`.

- [ ] **Step 1: Failing unit tests.**
  - `frontend/tests/pwa-icons.test.ts`, "keeps the markers the render script and Logo.svelte slice
    on": `expect(mark).toContain('#ffffff');` → `expect(mark).toContain('#1c1300');`,
    `mark.replaceAll('#ffffff', '')` → `mark.replaceAll('#1c1300', '')`, and the comment "Drawn in
    any other spelling of white" → "Drawn in any other spelling of that ink". Add:

```ts
  it('is drawn in the app palette: dark ink on the amber plate', () => {
    const svg = readFileSync(new URL('icon.svg', publicDir), 'utf8');
    expect(svg).toContain('<rect width="64" height="64" rx="14" fill="#f59e0b"/>');
    // 9.2:1; white on this amber is 2.1:1.
    expect(svg).not.toMatch(/#ffffff|#1f6f5f/i);
  });
```

  - `frontend/tests/viewport.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';

const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');

describe('viewport', () => {
  // Chrome for Android then shrinks the layout viewport when the keyboard opens, so the sticky
  // Save bar (FormActions) sits above the keyboard instead of behind it. iOS ignores the key.
  it('lets the on-screen keyboard resize the layout', () => {
    expect(html).toMatch(/<meta name="viewport" content="[^"]*interactive-widget=resizes-content[^"]*"/);
  });
});
```

Run: `npx vitest run tests/pwa-icons.test.ts tests/viewport.test.ts` — FAIL.

- [ ] **Step 2: The icon.** `frontend/public/icon.svg`: `fill="#1f6f5f"` → `fill="#f59e0b"`, and
  every `#ffffff` inside the mark → `#1c1300` (three). In `frontend/scripts/render-icons.sh`, the
  maskable plate `<rect width="64" height="64" fill="#1f6f5f"/>` → `fill="#f59e0b"`, and the
  comment "render a plain green plate" → "render a bare amber plate". In `Logo.svelte`,
  `.replaceAll('#ffffff', 'currentColor')` → `.replaceAll('#1c1300', 'currentColor')` and the
  comment "Its white ink becomes currentColor, so in the app it takes the theme's accent instead
  of sitting on a teal plate." → "Its dark ink becomes currentColor, so in the app it takes the
  amber ink of the theme instead of sitting on its plate."

```bash
frontend/scripts/render-icons.sh
```

  Expected: "rendered: pwa-192.png pwa-512.png apple-touch-icon.png pwa-512-maskable.png
  (icon.sha256 updated)". Look at `frontend/public/pwa-512-maskable.png` and `pwa-192.png`.

- [ ] **Step 3: The viewport.** In `frontend/index.html`:
  `<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />` →
  `<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content" />`

Run: `npx vitest run tests/pwa-icons.test.ts tests/viewport.test.ts` — PASS.

- [ ] **Step 4: e2e.** Append to `39-screens.spec.ts`:

```ts
test('with the on-screen keyboard up, the field being typed in and Save both stay on screen', async ({ page }, info) => {
  test.skip(info.project.name !== 'mobile', 'the on-screen keyboard is a phone matter');
  await signInFresh(page, '39-keyboard');
  const id = await object(page, { name: 'Keyboard probe', type: 'tool' });
  await page.goto(`/objects/${id}/activities/new`);
  // What `interactive-widget=resizes-content` makes Chrome for Android do when the keyboard opens:
  // the layout viewport shrinks to what is left above it. Playwright has no keyboard to open.
  await page.setViewportSize({ width: 412, height: 420 });
  const title = page.getByLabel('Title');
  await title.focus();
  await title.fill('Typed with the keyboard up');
  const bar = (await page.getByTestId('form-actions').boundingBox())!;
  const field = (await title.boundingBox())!;
  expect(bar.y + bar.height, 'the Save bar ends above the keyboard').toBeLessThanOrEqual(421);
  expect(field.y + field.height, 'the field is above the Save bar').toBeLessThanOrEqual(bar.y);
  await expect(title).toBeInViewport();
  await expect(page.getByTestId('form-actions').getByRole('button', { name: 'Save' })).toBeInViewport();
});
```

- [ ] **Step 5: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 39-screens 38-forms 13-shell`
Expected: PASS, both projects.
Capture `../shots/r5-t10 login,dashboard` (the logo on the card and in the sidebar).

```bash
git add -A frontend
git commit -m "feat: amber app icon; the layout makes room for the on-screen keyboard"
```

The real-phone check (Android Chrome and iOS Safari: open "Log activity", tap Title, is Save above
the keyboard with room to see the field?) goes to the user in the release report.

---
### Task 11: Budget check and release 0.23.0

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, `frontend/package-lock.json`,
  `docs/openapi.json`, `docs/upgrading.md`

- [ ] **Step 1: Measure** (with `measure` from Global Constraints):

```bash
cd frontend && npm run build && cd ..
measure after
for f in before after; do awk -v n=$f '/\.js$/ {s+=$1} END {print n, "js", s}' .superpowers/sdd/bundle-r5-$f.txt; done
grep 'index-\|eager' .superpowers/sdd/bundle-r5-before.txt .superpowers/sdd/bundle-r5-after.txt
grep -l "bits-ui\|floating-ui\|lucide\|twMerge" frontend/dist/assets/index-*.js
```

Expected:
- JS total grows by ≤ 10,240 B.
- `index-*.js` ≤ 11,988 B.
- `eager` grows by ≤ 1,024 B.
- `index-*.css` smaller than before.
- The `grep -l` prints nothing.

If a limit is exceeded, report the numbers and the biggest contributors (compare Settings, the
nine settings chunks, Stats, Search, Login/Setup, the TopBar chunk) instead of bumping.

- [ ] **Step 2: Advisories.** `0.22.0` was blocked by new advisories on build-only packages the
  day it was tagged, so check before anything is tagged:

```bash
(cd frontend && npm audit --audit-level=high)
```

  Expected: exit 0. Otherwise update the flagged packages without `--force` (as 0.22.1 did), rerun
  `npm run check && npm test && npm run build`, include the lockfile change in the release commit,
  and name the packages in `docs/upgrading.md`. If a fix needs a breaking upgrade, stop and report.

- [ ] **Step 3: Bump to 0.23.0**

```bash
sed -i 's/^version = "0.22.1"/version = "0.23.0"/' Cargo.toml
cargo build   # updates the logb entry in Cargo.lock
(cd frontend && npm version 0.23.0 --no-git-tag-version)
sed -i '0,/"version": "0.22.1"/s//"version": "0.23.0"/' docs/openapi.json
git diff --stat   # exactly the six files, Cargo.lock only the logb version
```

  Add above `## 0.22.1` in `docs/upgrading.md`:

```markdown
## 0.23.0: new look, search, statistics, settings, sign-in

**Search** shows its hits as cards, like the object list: an icon, the name, the facts, the tags.

**Statistics** opens with three cards for the year: what was spent, how that compares with the
year before (over the same months while the year is still running), and which object cost most.
The charts leave out months with nothing in them. Energy, fuel and water come after the money.

**Settings** are grouped rows, each with its own icon. Settings save themselves as you change them
and say "Saved"; the Apply buttons are gone. A text field (currency, webhook URL, delivery hour)
saves when you leave it. Buttons remain for things that act: changing the password, signing out,
sending a test notification, connecting Telegram, exporting, switching the database.

**Sign-in and setup** are a card in the middle of the screen, and the password can be shown while
you type it.

**The app icon is amber.** A phone that installed LogB may keep the old icon until the app is
added to the home screen again.

On Android, the screen now makes room for the on-screen keyboard, so Save stays visible above it.

No migration.
```

- [ ] **Step 4: Verify and commit**

```bash
cargo clippy --all-targets -- -D warnings
cargo test --test it openapi::
(cd frontend && npm run check && npm test && npm run e2e)
git add Cargo.toml Cargo.lock frontend/package.json frontend/package-lock.json docs/openapi.json docs/upgrading.md
git commit -m "chore: release 0.23.0"
```

(plus the two trailer lines.)

Report:
- the bundle deltas: JS total, entry chunk, eager set, entry CSS, and the Login/Setup, Stats,
  Search and settings chunks;
- the unit and e2e test counts;
- every changed e2e assertion (the list under Decisions), plus any Task 9 visual fix;
- before/after screenshots: `shots/r5-before` against `shots/r5-t9` (all screens) and `r5-t10`;
- the open question: the keyboard check on a real Android phone and a real iPhone;
- for the user's release: push the tag on its own (`git push origin v0.23.0` after tagging), wait
  for its release workflow to finish, and check that `ghcr.io/13/logb:latest` has the same digest as
  `:0.23.0` and that GitHub marks v0.23.0 "Latest". Never push two release tags at once.

Stop there: merge, tag and push are the user's decision.
