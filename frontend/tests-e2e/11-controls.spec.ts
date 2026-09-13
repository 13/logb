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
//
// This is asserted by clicking rather than by reading a computed style, because
// `getComputedStyle(el, '::-webkit-search-cancel-button')` does not resolve that pseudo-element
// at all: Chrome hands back the input's own style, so the call returns the input's width
// (388px here) whether the ✕ is drawn or suppressed, and an assertion on it can never pass.
// What the ✕ actually does to a user is wipe the query when they meant to put the caret at the
// end of it, so that is what is checked: with the native button present, clicking ~20px in from
// the right edge empties the field; with it gone, the click only moves the caret.
test('the browser draws no clear button of its own', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Search' }).click();
  const box = page.getByRole('searchbox');
  await box.fill('golf');
  const bb = (await box.boundingBox())!;

  for (const inset of [8, 12, 16, 20, 24, 30]) {
    await box.fill('golf');
    await page.mouse.click(bb.x + bb.width - inset, bb.y + bb.height / 2);
    expect(
      await box.inputValue(),
      `clicking ${inset}px in from the right edge cleared the query -- that is the browser's own ✕`,
    ).toBe('golf');
  }
});

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
  const quick = row.getByRole('button', { name: /Log|Erfassen/ });

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

// A button stood 46px tall, a text field 48 and a select and a date field 50 -- three heights
// in one form, from three different internal line boxes rather than from anything anyone chose.
// The numbers are the assertion: "they look consistent" is what the last fix claimed.
test('every control in a form is the same height', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Control height probe');
  await page.getByLabel('Type').selectOption('car');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.getByRole('button', { name: 'Back' }).click();
  await page
    .locator('.card-row', { hasText: 'Control height probe' })
    .getByRole('button', { name: /Log|Erfassen/ })
    .click();

  const title = page.locator('input#ti');
  await expect(title).toBeVisible();
  const heights = await page.evaluate(() =>
    Object.fromEntries(
      Object.entries({
        button: 'button.primary',
        input: 'input#ti',
        select: 'select#c',
        date: 'input#d',
      }).map(([name, sel]) => [
        name,
        Math.round((document.querySelector(sel) as HTMLElement).getBoundingClientRect().height),
      ]),
    ),
  );
  const distinct = [...new Set(Object.values(heights))];
  expect(distinct, `controls stand at different heights: ${JSON.stringify(heights)}`).toHaveLength(1);
  // And the one height they share still clears the tap-target floor.
  expect(distinct[0]).toBeGreaterThanOrEqual(44);
});

// The chips row sat flush against the first card: 8px of air above it and 4px below, and the
// 4px was the focus ring's bleed rather than a gap anyone chose. A row that filters a list
// belongs between the two, not stuck to one of them.
test('the filter row sits in the middle of its own gap', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Chip gap probe');
  await page.getByLabel('Type').selectOption('bike');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.locator('.card-row', { hasText: 'Chip gap probe' })).toBeVisible();

  const gaps = await page.evaluate(() => {
    const box = (el: Element) => el.getBoundingClientRect();
    const chips = document.querySelector('.chips')!;
    const chip = chips.querySelector('button')!;
    // What is actually above the chip is the topbar's last control, not the topbar's box. The
    // dashboard's topbar carries no buttons of its own any more -- both used to live there, and
    // now live in the app nav instead -- so its last control is the title.
    const above = box(chip).top - box(document.querySelector('.topbar h1')!).bottom;
    const below = box(chips.nextElementSibling!).top - box(chip).bottom;
    return { above, below };
  });
  expect(gaps.below, `the chips row is off-centre in its gap: ${JSON.stringify(gaps)}`).toBe(gaps.above);
});

// An empty list is what these tabs are initialised with, not an answer from the server. Drawing
// the empty state from it put a full icon-sentence-button block on screen while the first
// request was still out, and took it away again when the answer arrived -- a bigger flash than
// the one-line "nothing yet" it replaced.
test('an empty state waits for the answer before claiming there is nothing', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Empty flash probe');
  await page.getByLabel('Type').selectOption('bike');
  await page.getByRole('button', { name: 'Save' }).click();

  // Hold both answers back long enough that a premature empty state would be on screen.
  for (const path of ['**/api/objects/*/attachments', '**/api/objects/*/reminders']) {
    await page.route(path, async (route) => {
      await new Promise((r) => setTimeout(r, 1500));
      await route.continue();
    });
  }

  const docsEmpty = page.getByText(/Receipts, manuals|Belege, Handb/);
  await page.getByRole('button', { name: 'Documents' }).click();
  await expect(docsEmpty, 'the documents empty state was drawn before the request answered').toHaveCount(0);
  await expect(docsEmpty).toBeVisible({ timeout: 5000 });

  const remEmpty = page.getByText(/A reminder watches|Eine Erinnerung/);
  await page.getByRole('button', { name: 'Reminders' }).click();
  await expect(remEmpty, 'the reminders empty state was drawn before the request answered').toHaveCount(0);
  await expect(remEmpty).toBeVisible({ timeout: 5000 });
});
