# Consistent Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every native control looks like part of LogB because it exists, not because someone remembered a class — and the dashboard row stops reading as two cards.

**Architecture:** Base styles move from `.field input` onto element selectors in `app.css`, so a control is styled by default and native chrome is suppressed deliberately. The dashboard's quick-log button moves inside the card. A test then fails when a value escapes the scale, which is what stops this recurring.

**Tech Stack:** Svelte 5, plain CSS, vitest, Playwright (Pixel 7 / Chromium).

## What the screenshots actually showed

Two of the four complaints were diagnosed wrongly at first; the code corrected them, and the plan follows the code:

- The dashboard card **does** carry the counter, the total and a due chip (`ObjectCard.svelte:22-30`). It looked empty only because the objects had no activities yet. **Do not add fields.** The defect is the layout beside it.
- The quick-log button is **`＋` (U+FF0B, fullwidth plus)** — a glyph standing in for an icon, in a project that has an `Icon.svelte` and a test that exists to stop exactly this. It slipped through because `tests/icons.test.ts`'s pattern does not cover the Halfwidth and Fullwidth Forms block.

## Global Constraints

- No new dependency, runtime or dev. No Tailwind, no component library — the spec records why, with measurements.
- The palette does not change. Teal, off-white, the bee.
- Every user-facing string exists in both `frontend/src/i18n/en.ts` and `de.ts`.
- `npm run check` passes; backend 367, vitest 168, Playwright 24 pass at every task boundary.
- The debug binary serves `frontend/dist` from disk, so `npm run build` is enough for a frontend change; a **Rust** change needs `cargo build`.
- Run verification in the FOREGROUND. Seven agents on this project stalled on background runs that died with their turn.

## File structure

| File | Responsibility |
|---|---|
| `frontend/src/app.css` | Element-level base styles for every control |
| `frontend/src/routes/Search.svelte` | Loses its private input styling and its uppercase label |
| `frontend/src/lib/ObjectCard.svelte` | The row, and the quick-log button inside it |
| `frontend/src/routes/Dashboard.svelte` | The archived filter as a chip |
| `frontend/src/lib/Icon.svelte` | A `plus` variant |
| `frontend/tests/scale.test.ts` (new) | The guard that stops values escaping again |

---

### Task 1: A control is styled because it exists

**Files:**
- Modify: `frontend/src/app.css:72-81`
- Modify: `frontend/src/routes/Search.svelte:41-50, 86-87`
- Test: `frontend/tests-e2e/11-controls.spec.ts` (new)

**Interfaces:** none shared.

- [ ] **Step 1: Write the failing test**

Create `frontend/tests-e2e/11-controls.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

// The search box was the one input in the app nobody wrapped in `.field`, so it fell through to
// Chrome's own styling: square corners, a hard focus rectangle, and a blue clear button in a
// colour that appears nowhere else in LogB. Styling controls by element rather than by class is
// what makes that impossible rather than unlikely.
test('the search box wears the app styling, not the browser default', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Search' }).click();
  const box = page.getByRole('searchbox');
  await expect(box).toBeVisible();

  const shape = await box.evaluate((el) => {
    const s = getComputedStyle(el);
    return { radius: s.borderTopLeftRadius, height: el.getBoundingClientRect().height, appearance: s.appearance };
  });
  expect(shape.radius).not.toBe('0px');
  expect(shape.height).toBeGreaterThanOrEqual(44);
  expect(shape.appearance).toBe('none');
});

// Chrome draws its own ✕ inside a search input. It is blue, it is not ours, and `appearance:
// none` on the control does not remove it -- the pseudo-element has to be addressed directly.
test('the browser draws no clear button of its own', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Search' }).click();
  const box = page.getByRole('searchbox');
  await box.fill('golf');
  const nativeWidth = await box.evaluate((el) =>
    parseFloat(getComputedStyle(el, '::-webkit-search-cancel-button').width || '0'),
  );
  expect(nativeWidth).toBe(0);
});
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd frontend && npm run build && npx playwright test 11-controls`

Expected: the first test fails on `radius` being `0px`. Record what the values actually are — if
`appearance` already reports `none`, say so rather than assuming the whole test failed for one
reason.

- [ ] **Step 3: Move the base styles onto elements**

In `frontend/src/app.css`, the rule at `:80` currently reads
`.field input, .field select, .field textarea { … }`. Styling is opt-in there, and that is the
defect. Replace it with element selectors, and suppress the native chrome explicitly:

```css
/* Controls are styled because they exist, not because someone remembered to wrap them in
   `.field`. The search box was the one input nobody wrapped, and it fell through to the
   browser's own look -- square corners and a blue clear button, in an app with neither. */
input, select, textarea {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  padding: var(--space-3);
  min-height: 44px;
  width: 100%;
  appearance: none;
}
/* `appearance: none` does not reach this: Chrome draws its own ✕ inside a search input, in its
   own blue. Ours is a button beside the field, in the app's colours. */
input[type='search']::-webkit-search-cancel-button { appearance: none; display: none; }
/* A checkbox with `appearance: none` is an invisible box, so these keep the platform control --
   it is the one native widget that already reads correctly in both themes. */
input[type='checkbox'], input[type='radio'] {
  appearance: auto; width: auto; min-height: 0; padding: 0;
}
```

Keep `.field`'s layout rules — the label, the gap, the margin. Only the control styling moves.

- [ ] **Step 4: Take the private styling off the search box**

`Search.svelte:86`'s `input[type='search'] { width: 100% }` is now redundant. Delete it.

`:87`'s `h2` rule sets `text-transform: uppercase; letter-spacing: .04em; font-size: .9rem` —
a treatment used on no other heading in the app. Delete the rule and let the global `h2` apply.

- [ ] **Step 5: Run everything**

Run: `cd frontend && npm run check && npm run build && npx playwright test`

Expected: 0 type errors, 26 Playwright tests passing (24 plus the two new).

Then look: screenshot the search screen with a query typed, in both themes, and confirm the
field is rounded, the focus ring is the app's, and no blue ✕ appears. Report what you see.

Check every other screen for a control that changed shape unexpectedly — this rule now reaches
inputs that previously had none. The activity form, the object form and Settings are where to
look.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/app.css frontend/src/routes/Search.svelte frontend/tests-e2e/11-controls.spec.ts
git commit -m "fix: style controls by element, so the search box stops wearing Chrome's clothes"
```

---

### Task 2: The dashboard row is one object, not two cards

**Files:**
- Modify: `frontend/src/lib/ObjectCard.svelte:13-48`
- Modify: `frontend/src/lib/Icon.svelte`
- Test: `frontend/tests-e2e/11-controls.spec.ts`

**Interfaces:**
- Produces: `Icon` gains a `plus` variant.

- [ ] **Step 1: Write the failing test**

Append to `frontend/tests-e2e/11-controls.spec.ts`:

```ts
// The quick-log button sat in its own full-height box beside the card, with a gap on each side,
// so every row read as two cards -- and it was a fullwidth plus character rather than an icon.
test('the quick-log action belongs to its row', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Row shape probe');
  await page.getByLabel('Type').selectOption('car');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.getByRole('button', { name: 'Back' }).click();

  const row = page.locator('.card-row', { hasText: 'Row shape probe' });
  const card = row.locator('.list-card');
  const quick = row.getByRole('button', { name: /Log|Eintrag/ });

  const [rowBox, cardBox, quickBox] = await Promise.all([
    row.boundingBox(), card.boundingBox(), quick.boundingBox(),
  ]);
  // One card, the width of the row: the action is inside it, not a sibling with a gap.
  expect(cardBox!.width).toBeCloseTo(rowBox!.width, 0);
  expect(quickBox!.x).toBeGreaterThan(cardBox!.x);
  expect(quickBox!.x + quickBox!.width).toBeLessThanOrEqual(cardBox!.x + cardBox!.width + 1);
  // And it is an icon, not a glyph standing in for one.
  expect(await quick.locator('svg').count()).toBe(1);
});
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd frontend && npm run build && npx playwright test 11-controls`

Expected: fails because the card is narrower than the row — the `＋` box is a sibling. Report the
two widths.

- [ ] **Step 3: Add a `plus` icon**

In `frontend/src/lib/Icon.svelte`, add `plus` to the name union and a branch matching the
existing style — 24×24 viewBox, `fill="none"`, `stroke="currentColor"`, `stroke-width="1.75"`,
round caps:

```svelte
  {:else if name === 'plus'}
    <path d="M12 5v14M5 12h14" />
```

- [ ] **Step 4: Move the button inside the card**

In `ObjectCard.svelte`, the quick-log `<button>` moves inside `.list-card`'s row, at its end, and
renders `<Icon name="plus" />` rather than `＋`. Keep its `aria-label` exactly as it is — the
e2e suite finds controls by accessible name.

A button cannot be nested inside a button. `.list-card` is currently a `<button>`; make the card
a `<div>` with the name as the navigating control, or keep the card a button and place the
quick-log action as a sibling positioned inside the card's bounds. Choose, and say which and why
in your report — the constraint is that the row reads as one object and both actions remain
reachable by keyboard, with visible focus.

Replace the stray values in this file's `<style>` block — `8px`, `12px`, `4px`, `6px`, `1.4rem`
— with scale tokens.

- [ ] **Step 5: Run everything, and look**

Run: `cd frontend && npm run check && npm run build && npx playwright test`

Expected: 0 type errors, 27 passing.

Screenshot the dashboard with three objects in both themes. Confirm the rows read as single
cards, the action is obviously an action, and the spacing between rows is even. Report what you
see, and tab through the dashboard to confirm both the card and the quick-log button take focus
visibly.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/ObjectCard.svelte frontend/src/lib/Icon.svelte frontend/tests-e2e/11-controls.spec.ts
git commit -m "fix: the quick-log action belongs to its row"
```

---

### Task 3: Values stop escaping the scale

**Files:**
- Create: `frontend/tests/scale.test.ts`
- Modify: every `.svelte` file whose `<style>` block holds a stray value

**Interfaces:** none shared.

- [ ] **Step 1: Write the failing test**

Create `frontend/tests/scale.test.ts`. Model it on `frontend/tests/icons.test.ts`, which walks
the tree and carries per-value exemptions — read that file first and follow its shape:

```ts
import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';

const SRC = fileURLToPath(new URL('../src', import.meta.url));

/** Every `<style>` block under src, as (file, css) pairs. */
function styleBlocks(dir: string): Array<{ rel: string; css: string }> {
  const out: Array<{ rel: string; css: string }> = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...styleBlocks(full));
    else if (entry.name.endsWith('.svelte')) {
      const src = readFileSync(full, 'utf8');
      const m = src.match(/<style>([\s\S]*?)<\/style>/);
      if (m) out.push({ rel: relative(SRC, full), css: m[1] });
    }
  }
  return out;
}

/**
 * Values a component may hold despite not being on the scale, each with the reason. Listed by
 * the exact string so an exemption that stops being needed fails loudly, the way the icon
 * test's exclusions do.
 */
const ALLOWED: Record<string, string> = {
  '1px': 'a hairline border is not a spacing step',
  '2px': 'the focus ring and badge insets are optical, not spatial',
  '50%': 'a circle',
  '100%': 'fill the parent',
  '0': 'zero is zero',
};

describe('the scale', () => {
  it('no component invents a font size', () => {
    for (const { rel, css } of styleBlocks(SRC)) {
      const hits = [...css.matchAll(/font-size:\s*([^;]+);/g)].map((m) => m[1].trim());
      const raw = hits.filter((v) => !v.startsWith('var(--text-'));
      expect(raw, `${rel} sets a font size outside the scale: ${raw.join(', ')}`).toEqual([]);
    }
  });

  it('no component invents a spacing value', () => {
    for (const { rel, css } of styleBlocks(SRC)) {
      const hits = [...css.matchAll(/(?:gap|margin|padding)(?:-[a-z]+)?:\s*([^;]+);/g)]
        .flatMap((m) => m[1].trim().split(/\s+/))
        .filter((v) => !v.startsWith('var(--space-') && !(v in ALLOWED));
      expect(hits, `${rel} sets spacing outside the scale: ${hits.join(', ')}`).toEqual([]);
    }
  });
});
```

- [ ] **Step 2: Run it and see the full list**

Run: `cd frontend && npx vitest run tests/scale.test.ts`

Expected: failures naming each file and value. **Record that list in your report** — it is the
work of this task, and the next step is resolving every entry.

- [ ] **Step 3: Resolve every value**

Each stray resolves to the nearest scale step. `6px` becomes `--space-1` or `--space-2` by which
side of the gap it serves; `10px` and `18px` become `--space-3` and `--space-4`; `.8rem` and
`.9rem` become `--text-xs` and `--text-sm`.

Where a literal genuinely encodes something the scale cannot express — an optical alignment, a
fixed thumbnail size — add it to `ALLOWED` **with its reason**, rather than bending a token to
fit. A scale that cannot say a thing is not improved by a token that lies.

- [ ] **Step 4: Run everything, and compare**

Run: `cd frontend && npx vitest run && npm run check && npm run build && npx playwright test`

Expected: vitest 170 (168 plus the two new), 0 type errors, 27 Playwright.

Screenshot the dashboard, object detail, the activity form and Settings before and after this
task, and compare them. Values moved by a pixel or two in places; nothing should have moved by
more. Report anything that did.

- [ ] **Step 5: Commit**

```bash
git add frontend/src frontend/tests/scale.test.ts
git commit -m "fix: fold the strays into the scale, and fail when a new one appears"
```

---

### Task 4: A screen with nothing in it still says something

**Files:**
- Modify: `frontend/src/routes/Dashboard.svelte`, `frontend/src/lib/Timeline.svelte`, `frontend/src/lib/Documents.svelte`, `frontend/src/lib/Reminders.svelte`, `frontend/src/routes/Search.svelte`
- Modify: `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests-e2e/11-controls.spec.ts`

**Interfaces:** none shared.

- [ ] **Step 1: Write the failing test**

Append to `frontend/tests-e2e/11-controls.spec.ts`:

```ts
// A screen that stops at a heading looks broken. Each empty state says what belongs there and
// offers the action that puts something there.
test('an empty search says so, and an unrun search does not', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Search' }).click();
  // Nothing typed yet: no result state at all, which is different from "no results".
  await expect(page.getByText(/No matches|Keine Treffer/)).toHaveCount(0);
  await page.getByRole('searchbox').fill('zzzz-nothing-matches-this');
  await expect(page.getByText(/No matches|Keine Treffer/)).toBeVisible();
});
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npm run build && npx playwright test 11-controls`

Expected: the second assertion fails — nothing is rendered for a search with no results.

- [ ] **Step 3: Write the states**

Each empty state is a sentence and, where there is one, the action — in the interface's voice,
never an apology, never a bare "No data":

| Screen | What it says |
|---|---|
| Dashboard | what LogB is for, and the button that starts it |
| Timeline | an invitation to log the first entry for this object |
| Documents | what belongs here — receipts, manuals, photos — and the picker |
| Reminders | what a reminder does, and the button to add one |
| Search | what was searched for, and that nothing matched it |

The dashboard's archived filter also becomes a chip row rather than a raw checkbox, matching the
chips object detail already uses. `Dashboard.svelte:82` is the checkbox to replace.

Every string in both `en.ts` and `de.ts`, and the German reads as German.

- [ ] **Step 4: Run everything, and look**

Run: `cd frontend && npm run check && npm run build && npx playwright test`, and the backend
suite to confirm nothing moved there.

Expected: 0 type errors, 28 Playwright, backend 367.

Screenshot each empty state in both themes and look at them. A screen with nothing in it should
look deliberate. Report what you see.

- [ ] **Step 5: Commit**

```bash
git add frontend/src frontend/tests-e2e/11-controls.spec.ts
git commit -m "feat: screens say what belongs in them when they are empty"
```

---

### Task 5: Close the loop on the glyph that escaped

**Files:**
- Modify: `frontend/tests/icons.test.ts`
- Modify: `docs/superpowers/specs/2026-09-13-consistent-controls-design.md` (status)

- [ ] **Step 1: Widen the guard**

`tests/icons.test.ts` exists to stop a glyph standing in for an icon. The dashboard's `＋` was
exactly that and it passed, because the pattern does not cover the Halfwidth and Fullwidth Forms
block (U+FF00–FFEF).

Add that range. Then check the whole tree for anything else it now catches, and fix whatever it
finds rather than exempting it — an exemption is for a character that is genuinely not an icon,
like the arrow in a source comment.

- [ ] **Step 2: Prove it would have caught the original**

Put `＋` back into a component temporarily, run the test, watch it fail naming that file, then
remove it. Report both outputs. A guard that was not seen catching the thing it was widened for
is not yet a guard.

- [ ] **Step 3: Mark the spec implemented, and commit**

```bash
git add frontend/tests/icons.test.ts docs/superpowers/specs/2026-09-13-consistent-controls-design.md
git commit -m "test: catch a fullwidth plus pretending to be an icon"
```
