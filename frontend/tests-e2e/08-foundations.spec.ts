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
    return { tag: el.tagName, width: s.outlineWidth, style: s.outlineStyle, offset: s.outlineOffset };
  });

  expect(ring, 'something should be focused after one Tab').not.toBeNull();

  // Chromium's own default focus ring already satisfies "has some outline" (outlineStyle:
  // 'auto', 1px, no offset), so that's not proof this app styles focus. What the app's rule
  // in app.css actually adds -- and the browser default does not -- is a *solid* 2px outline
  // with a 2px offset. Assert on those, not on "not none" / "> 0".
  expect(ring!.style, `${ring!.tag} outline should be 'solid' (the app's rule), not the browser default 'auto'`).toBe(
    'solid'
  );
  expect(ring!.width, `${ring!.tag} outline width should be the app's 2px, not the browser default 1px`).toBe('2px');
  expect(ring!.offset, `${ring!.tag} outline offset should be the app's 2px, not the browser default 0px`).toBe(
    '2px'
  );
});
