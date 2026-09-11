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
