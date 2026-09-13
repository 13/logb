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
