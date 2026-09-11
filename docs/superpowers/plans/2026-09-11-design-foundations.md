# Design Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `LogB` a spacing, type and radius scale, visible focus, respected reduced-motion, and real SVG icons — so the screens built next inherit a system instead of extending an improvisation.

**Architecture:** Nearly all of this lives in `frontend/src/app.css`, an 87-line stylesheet that has carried the whole app since the first commit. Tokens are added at `:root`, then the existing rules are rewritten to use them. Only the four emoji and two layout defects reach into components.

**Tech Stack:** Plain CSS custom properties, Svelte 5, vitest, Playwright. No new dependency — the frontend has zero runtime dependencies and keeps it.

## Global Constraints

- **The palette does not change.** Every existing colour token keeps its value in both themes.
- No new dependency, runtime or dev. No webfont: this is an offline-first PWA and a display face would cost a network round trip before first paint.
- No component is restructured, no screen relaid out beyond the two named defects, no copy changed. Before-and-after screenshots should show the same app, slightly tidier.
- `npm run check` (svelte-check + tsc) must pass; all 18 Playwright tests must pass.
- Tabular numerals are `font-variant-numeric: tabular-nums` **within the existing family** — not a monospace face for data.

---

### Task 1: The token system

**Files:**
- Modify: `frontend/src/app.css`

**Interfaces:**
- Consumes: nothing.
- Produces: `--text-xs|sm|base|lg|xl|data`, `--space-1`…`--space-6`, `--radius-sm|md|full`, `--focus`, `--transition`. Later tasks use `--focus` and `--transition` by name.

- [ ] **Step 1: Add the tokens**

In `frontend/src/app.css`, inside the existing `:root` block, after `--shadow`:

```css
  /* Type: six steps at roughly a 1.2 ratio, sized for UI density rather than editorial
     reading. Replaces nine improvised sizes (1.4/1.15/1.1/1.05/1/.9/.85/.8/.75rem). */
  --text-xs: .75rem;
  --text-sm: .875rem;
  --text-base: 1rem;
  --text-lg: 1.125rem;
  --text-xl: 1.375rem;
  /* Figures get their own step. This is a logbook -- costs and odometer readings are the
     content, and they are read down a column, so they are set slightly larger than body. */
  --text-data: 1.25rem;

  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 24px;
  --space-6: 32px;

  /* Three steps, not one. A control and a container should not read as the same object. */
  --radius-sm: 8px;
  --radius-md: 12px;
  --radius-full: 999px;

  /* The ring sits on the page behind the control (see outline-offset below), so it is the
     text colour rather than the accent -- an accent ring on a filled accent button is
     invisible, which is exactly where focus matters most. */
  --focus: #1c1c1a;
  --transition: 150ms ease;
```

And inside `:root[data-theme="dark"]`, after its `--shadow`:

```css
  --focus: #ecebe6;
```

- [ ] **Step 2: Retire the old radius token**

`--radius: 12px` is referenced by `button`, `.card`, `.banner`, `dialog` and `.fab`. Replace the
declaration in `:root` with nothing, and update each use:

- `button` → `border-radius: var(--radius-sm);`
- `.card` → `border-radius: var(--radius-md);`
- `.banner` → `border-radius: var(--radius-md);`
- `dialog` → `border-radius: var(--radius-md);`
- `.fab` → already `border-radius: 999px` → `var(--radius-full)`
- `.chip` → already `999px` → `var(--radius-full)`
- `.field input, .field select, .field textarea` → already `8px` → `var(--radius-sm)`
- `.thumb`, `.doc-icon` → already `8px` → `var(--radius-sm)`

Confirm with `grep -n "radius\|border-radius" frontend/src/app.css` that no literal `12px`,
`8px` or `999px` radius remains and `--radius:` is gone.

- [ ] **Step 3: Put every size and space on the scale**

Rewrite the existing rules in `frontend/src/app.css` to use the tokens. The mapping, applied
throughout — nothing else about these rules changes:

| Was | Becomes |
|---|---|
| `1.4rem` (`h1`) | `var(--text-xl)` |
| `1.15rem` (`.topbar h1`) | `var(--text-lg)` |
| `1.1rem` (`h2`) | `var(--text-lg)` |
| `1.05rem` (`.stat b`) | `var(--text-data)` |
| `.9rem` (`.muted`) | `var(--text-sm)` |
| `.85rem` (`.field label`) | `var(--text-sm)` |
| `.8rem` (`.hint`, `.warn`, `.chip`) | `var(--text-xs)` |
| `.75rem` (`.stat span`) | `var(--text-xs)` |
| `4px` | `var(--space-1)` |
| `6px`, `8px` | `var(--space-2)` |
| `10px`, `12px`, `14px` | `var(--space-3)` |
| `16px`, `20px` | `var(--space-4)` |
| `24px` | `var(--space-5)` |

Leave alone, because they are not spacing: `min-height: 44px` (the touch target), `1px`
borders, `2px` tab underline, `64px` thumbnails, `min(92vw, 420px)`, `env(safe-area-inset-*)`,
and the `minmax()` values in `.grid` and `.stats`.

Where a rule has `padding: 10px 16px`, both numbers move to the nearest step —
`padding: var(--space-3) var(--space-4)`.

- [ ] **Step 4: Set figures in tabular numerals**

Add to `frontend/src/app.css`, after the `.stat` rules:

```css
/* Figures line up when stacked. A column of costs in proportional numerals does not, and this
   app is mostly costs, odometer readings and dates read down a list. Tabular figures are a
   property of the existing family, not a second typeface. */
.stat b, .tnum { font-variant-numeric: tabular-nums; }
```

Then in `frontend/src/lib/Timeline.svelte`, add the class to the meta line — the only other
place figures are stacked. Change:

```svelte
          <div class="muted">
```

to:

```svelte
          <div class="muted tnum">
```

- [ ] **Step 5: Check nothing moved that should not have**

Run: `cd frontend && npm run check && npm run build && npx playwright test`
Expected: 0 errors, a clean build, 18 passed. This task changes no markup except one class
attribute, so a failure here means a token was mistyped — `grep -n "var(--" frontend/src/app.css`
and look for a name that does not exist in `:root`.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/app.css frontend/src/lib/Timeline.svelte
git commit -m "style: put type, space and radius on a scale"
```

---

### Task 2: Visible focus, and motion that can be turned off

**Files:**
- Modify: `frontend/src/app.css`
- Test: `frontend/tests-e2e/08-foundations.spec.ts` (create)

**Interfaces:**
- Consumes: `--focus` and `--transition` from Task 1.
- Produces: a `:focus-visible` rule covering every interactive element.

`:focus-visible` appears nowhere in the codebase today. Someone tabbing through the app has no
idea where they are — this is the single largest accessibility gap in it.

- [ ] **Step 1: Write the failing test**

Create `frontend/tests-e2e/08-foundations.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('keyboard focus is visible', async ({ page }) => {
  await signIn(page);

  // A real Tab press, not .focus(): `:focus-visible` deliberately does not match a programmatic
  // or mouse focus, so focusing by script would pass while a keyboard user still saw nothing.
  await page.keyboard.press('Tab');

  const ring = await page.evaluate(() => {
    const el = document.activeElement;
    if (!el || el === document.body) return null;
    const s = getComputedStyle(el);
    return { tag: el.tagName, width: s.outlineWidth, style: s.outlineStyle };
  });

  expect(ring, 'something should be focused after one Tab').not.toBeNull();
  expect(ring!.style, `${ring!.tag} has no outline style`).not.toBe('none');
  expect(parseFloat(ring!.width), `${ring!.tag} has a zero-width outline`).toBeGreaterThan(0);
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npm run build && npx playwright test 08-foundations`
Expected: FAIL — the focused element's `outlineStyle` is `none`, because nothing in the app
styles focus.

- [ ] **Step 3: Add the focus and motion rules**

Append to `frontend/src/app.css`:

```css
/* Nothing in this app styled focus, so keyboard users had no cursor. `:focus-visible` rather
   than `:focus` so a tap or a click does not leave a ring behind on a touch screen.
   The offset puts the ring on the page rather than on the control, which is what makes it
   legible on a filled accent button. */
:focus-visible {
  outline: 2px solid var(--focus);
  outline-offset: 2px;
}

button, a, .chip, .tabs button {
  transition: background-color var(--transition), color var(--transition),
              border-color var(--transition);
}

/* Motion here only ever answers something the user did. Someone who has asked their system for
   less of it should get none. */
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: .01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: .01ms !important;
    scroll-behavior: auto !important;
  }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend && npm run build && npx playwright test 08-foundations`
Expected: PASS.

- [ ] **Step 5: Check the ring is visible on the accent button too**

The case the offset exists for. Run:

```bash
cd frontend && npx playwright test 08-foundations --debug
```

or capture it: temporarily add to the test, run once, then remove —

```ts
  await page.locator('button.primary, button.fab').first().focus();
  await page.keyboard.press('Tab');
  await page.screenshot({ path: '/tmp/focus-on-accent.png' });
```

Open the screenshot and confirm the ring is legible against the teal fill. If it is not, the
offset is too small, not the colour.

- [ ] **Step 6: Run the whole suite**

Run: `cd frontend && npm run check && npx playwright test`
Expected: 0 errors, 19 passed (18 existing plus the new one).

- [ ] **Step 7: Commit**

```bash
git add frontend/src/app.css frontend/tests-e2e/08-foundations.spec.ts
git commit -m "feat: show keyboard focus, and honour reduced motion"
```

---

### Task 3: Real icons

**Files:**
- Create: `frontend/src/lib/Icon.svelte`
- Modify: `frontend/src/lib/TopBar.svelte`, `frontend/src/routes/Dashboard.svelte`, `frontend/src/lib/Timeline.svelte`
- Test: `frontend/tests/icons.test.ts` (create)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `Icon.svelte` taking `name: 'back' | 'settings' | 'search' | 'document'`, rendering a 20px inline SVG in `currentColor`.

Four emoji stand in for icons: `←` back and `⚙` settings in `TopBar.svelte`, `🔍` search in
`Dashboard.svelte`, and `📄` for a document thumbnail in `Timeline.svelte`. They render
differently on every platform and ignore the theme's colour.

- [ ] **Step 1: Write the failing test**

Create `frontend/tests/icons.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';

// The four that were there, plus the variation selector that often trails an emoji in source.
const EMOJI = /[\u{1F300}-\u{1FAFF}\u{2190}-\u{21FF}\u{2600}-\u{27BF}\u{FE0F}]/u;

const FILES = [
  '../src/lib/TopBar.svelte',
  '../src/routes/Dashboard.svelte',
  '../src/lib/Timeline.svelte',
];

describe('icons', () => {
  it('no emoji stand in for icons', () => {
    // They render differently on every platform and ignore the theme colour. This is the test
    // that stops one creeping back in the next time somebody wants a quick glyph.
    for (const f of FILES) {
      const src = readFileSync(new URL(f, import.meta.url), 'utf8');
      const hit = src.match(EMOJI);
      expect(hit, `${f} contains ${hit?.[0]}`).toBeNull();
    }
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run tests/icons.test.ts`
Expected: FAIL, three times over — `TopBar.svelte contains ←`, `Dashboard.svelte contains 🔍`,
`Timeline.svelte contains 📄`.

- [ ] **Step 3: Write the icon component**

Create `frontend/src/lib/Icon.svelte`:

```svelte
<script lang="ts">
  /** Inline SVG rather than a sprite or a library: there are four of them, they must take the
   *  theme's colour through `currentColor`, and the project has no runtime dependencies. */
  let { name, size = 20 }: { name: 'back' | 'settings' | 'search' | 'document'; size?: number } = $props();
</script>

<svg
  width={size} height={size} viewBox="0 0 24 24" fill="none"
  stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"
  aria-hidden="true" focusable="false"
>
  {#if name === 'back'}
    <path d="M19 12H5M12 19l-7-7 7-7" />
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="7" />
    <path d="M21 21l-4.3-4.3" />
  {:else}
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <path d="M14 2v6h6" />
  {/if}
</svg>
```

- [ ] **Step 4: Use it in all four places**

In `frontend/src/lib/TopBar.svelte`, add to the imports in `<script>`:

```ts
  import Icon from './Icon.svelte';
```

Replace `←` with `<Icon name="back" />` and `⚙` with `<Icon name="settings" />`. Both buttons
already carry an `aria-label`, so the icon stays `aria-hidden` and the accessible name is
unchanged.

In `frontend/src/routes/Dashboard.svelte`, add to the imports:

```ts
  import Icon from '../lib/Icon.svelte';
```

and replace `🔍` with `<Icon name="search" />`.

In `frontend/src/lib/Timeline.svelte`, add to the imports:

```ts
  import Icon from './Icon.svelte';
```

and replace `📄` with `<Icon name="document" size={28} />` — larger, because it stands in the
64px thumbnail slot rather than a 44px button.

- [ ] **Step 5: Run the tests**

Run: `cd frontend && npx vitest run tests/icons.test.ts && npm run check && npm run build && npx playwright test`
Expected: the icon test passes, 0 check errors, 19 Playwright tests pass.

- [ ] **Step 6: Look at them**

Icons are the one thing in this plan whose success is visual. Capture the two screens:

```bash
cd frontend && npx playwright test 01-smoke --debug
```

or add a temporary screenshot to any spec. Confirm the back arrow, gear and magnifier are the
same visual weight as the text beside them, and that they take the accent/text colour rather than
staying multicoloured. A gear that looks heavier than the title means `stroke-width` is too high
for its size.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/Icon.svelte frontend/src/lib/TopBar.svelte frontend/src/routes/Dashboard.svelte frontend/src/lib/Timeline.svelte frontend/tests/icons.test.ts
git commit -m "feat: replace the emoji with real icons"
```

---

### Task 4: The two layout defects

**Files:**
- Modify: `frontend/src/app.css`

**Interfaces:**
- Consumes: the space tokens from Task 1.
- Produces: nothing later tasks depend on.

Both are scale problems rather than design decisions, which is why they belong in this slice.

- [ ] **Step 1: Stop the stat row orphaning its last item**

`.stats` is `repeat(auto-fit, minmax(110px, 1fr))`, which fits three across on a phone and drops
the fourth onto a row of its own, left-aligned under a three-column grid. It reads as a bug
because it is one. In `frontend/src/app.css`, replace the `.stats` rule with:

```css
/* Two columns on a phone, four when there is room -- never three-then-one, which is what
   auto-fit produced and which reads as a layout that broke rather than one that wrapped. */
.stats {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
  margin: var(--space-3) 0;
}
@media (min-width: 480px) {
  .stats { grid-template-columns: repeat(4, 1fr); }
}
```

- [ ] **Step 2: Show that the filter chips scroll**

`.chips` scrolls horizontally with nothing to say so, so the row appears to end mid-word. Replace
the `.chips` rule with:

```css
/* The row scrolls; without the mask it just looks cut off. The fade is the affordance -- a
   scrollbar is invisible on a touch device, and an arrow would need a tap target for something
   a swipe already does. */
.chips {
  display: flex;
  gap: var(--space-2);
  overflow-x: auto;
  padding-bottom: var(--space-1);
  scrollbar-width: none;
  mask-image: linear-gradient(to right, #000 calc(100% - 24px), transparent);
}
.chips::-webkit-scrollbar { display: none; }
```

- [ ] **Step 3: Check both, on a phone viewport**

The Playwright project is already `Pixel 7`, so the existing suite renders at phone width. Run:

Run: `cd frontend && npm run build && npx playwright test`
Expected: 19 passed.

Then look at the object detail screen and confirm: the four stats sit two-by-two with nothing
orphaned, and the filter row fades at the right edge rather than ending mid-word.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/app.css
git commit -m "fix: stop the stat row orphaning, and show that the chips scroll"
```

---

## Self-Review

**Spec coverage.** Every section maps to a task: the type, space and radius scales and tabular
numerals (Task 1); `:focus-visible` and `prefers-reduced-motion` (Task 2); the four emoji replaced
by SVG (Task 3); the `.stats` orphan and the `.chips` affordance (Task 4). Both tests the spec
asks for are present — focus genuinely rendered after a real Tab (Task 2 Step 1), and the emoji
unable to return (Task 3 Step 1).

The spec's constraints hold: no colour token changes, no new dependency, no webfont, no component
restructured, no copy touched.

**Placeholder scan.** No TBDs and no "add appropriate styling". Task 1 Step 3 is a mapping table
rather than a rewritten stylesheet, which is deliberate: reproducing all 87 lines would invite a
transcription error in rules the task is not changing, and the table names every value that moves
and every value that must not.

**Type consistency.** `--focus` and `--transition` are defined in Task 1 Step 1 and used by name in
Task 2 Step 3. `--space-*` is defined in Task 1 and used in Task 4. `Icon.svelte`'s `name` union
lists exactly the four values used at the four call sites, and `size` is used at one of them.
`.tnum` is defined in Task 1 Step 4 and applied in the same step.

**One judgement call worth flagging.** Task 3 replaces `←` with an SVG. It is a text character
rather than an emoji, and it renders acceptably today — but it is in the same button row as the
gear, at a different optical weight, and leaving it would mean the top bar mixed a glyph with an
icon. The emoji test's range covers it deliberately.
