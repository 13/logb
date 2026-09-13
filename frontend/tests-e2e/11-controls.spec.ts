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
