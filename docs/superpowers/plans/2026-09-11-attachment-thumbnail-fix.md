# Attachment Thumbnail Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the activity form filling with a 388×388 grey slab when a photo is attached.

**Architecture:** Two CSS changes in one task. The component's attachment wrapper stops using the
class name `thumb`, and the global `.thumb` rule is narrowed to `img.thumb` so no future `<div>`
can inherit a grid image's geometry.

**Tech Stack:** Svelte 5, plain CSS, Playwright (Pixel 7 / Chromium).

## Global Constraints

- The palette does not change. No colour value introduced or altered, in either theme.
- No new dependency, runtime or dev.
- No copy changed, no component restructured beyond the class rename.
- `npm run check` must pass; all 19 Playwright tests must pass.
- Existing `aria-label` values must not change — e2e tests find controls by accessible name.

---

### Task 1: Stop the attachment wrapper inheriting the grid image rule

**Files:**
- Modify: `frontend/src/routes/ActivityForm.svelte` (template line 304; `<style>` lines 339-341)
- Modify: `frontend/src/app.css:123`
- Test: `frontend/tests-e2e/02-lifecycle.spec.ts`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: nothing later tasks rely on.

- [ ] **Step 1: Write the failing assertion**

`02-lifecycle.spec.ts` already creates an object, opens the activity form, and attaches a photo
with `await page.setInputFiles('input[type=file]', pngPayload());` followed by
`await expect(page.locator('.thumb-strip img')).toHaveCount(1);`.

Immediately after that existing `toHaveCount` line, add:

```ts
  // The wrapper must fit its image. It used to carry the global `.thumb` rule, which is
  // written for grid images (`width: 100%; aspect-ratio: 1`), so it inflated to a 388-wide
  // square around a 64px thumbnail and pushed Save below the fold. Comparing the two widths
  // states the requirement; asserting a literal 64 would pin an unrelated decoration value.
  const item = page.locator('.thumb-strip > *').first();
  const img = page.locator('.thumb-strip img').first();
  const itemBox = await item.boundingBox();
  const imgBox = await img.boundingBox();
  expect(itemBox!.width).toBeCloseTo(imgBox!.width, 0);
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd frontend && npx playwright test 02-lifecycle`

Expected: FAIL, with the received value near `388` against an expected near `64`.

Record the actual numbers in the task report. If it passes, stop and report: the defect is not
reproducing and the rest of this task is invalid.

- [ ] **Step 3: Rename the component's wrapper**

In `frontend/src/routes/ActivityForm.svelte`, line 304, change the wrapper's class from `thumb`
to `strip-item`:

```svelte
          <div class="strip-item" class:pending={a.pending}>
```

In the same file's `<style>` block, lines 339-341, rename the three selectors and say why the
element exists:

```css
  /* A positioning context for the pending badge, not a thumbnail. It was called `thumb`, which
     collided with the global grid-image rule in app.css and inflated it to a full-width square
     around a 64px image. */
  .strip-item { position: relative; flex: none; }
  .strip-item.pending { opacity: .55; }
  .strip-item .pending-chip { position: absolute; left: 2px; right: 2px; bottom: 2px; text-align: center; font-size: .6rem; padding: 1px 2px; line-height: 1.2; }
```

- [ ] **Step 4: Narrow the global rule to images**

In `frontend/src/app.css:123`, change the selector to `img.thumb` and record why:

```css
/* `img.thumb`, not `.thumb`: this sizes a grid image, and a non-image that borrowed the name
   inherited `width: 100%` plus a square aspect ratio. Both real users are images -- the object
   card's cover and the documents grid. */
img.thumb { width: 100%; aspect-ratio: 1; object-fit: cover; border-radius: var(--radius-sm); background: var(--surface-2); display: block; }
```

- [ ] **Step 5: Run the suites**

Run: `cd frontend && npm run check && npm run build && npx playwright test`

Expected: `npm run check` reports 0 errors; 19 passed.

- [ ] **Step 6: Confirm by eye, in both themes**

Take a Pixel 7 screenshot of the activity form with a photo attached, in light and dark, and
look at them. Expected: a 64px thumbnail in a strip, no grey slab, and Save and Cancel visible
without scrolling past the photos section. Report the page height before and after attaching —
it was 839 → 1068 before this fix.

The two remaining `.thumb` users must be unchanged: the object card's cover image on the
dashboard, and the documents grid on object detail. Screenshot both and confirm.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/routes/ActivityForm.svelte frontend/src/app.css frontend/tests-e2e/02-lifecycle.spec.ts
git commit -m "fix: stop the attachment wrapper inheriting the grid image rule"
```
