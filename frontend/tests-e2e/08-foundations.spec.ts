import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('keyboard focus is visible', async ({ page }) => {
  await signInFresh(page, '08-foundations');
  // A navigation moves focus to the page heading (`focusPageHeading`, a tick after the route
  // renders). Wait for it, or the Tab below can land first and have its focus taken back.
  await expect(page.getByRole('heading', { level: 1 })).toBeFocused();
  // A fresh user's dashboard settles into its empty state; a Tab before that can land on a
  // control that is about to go.
  await expect(page.getByText(/LogB keeps the history/)).toBeVisible();

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
  // (app.tw.css's base layer, and every component's `focus-visible:` utilities) adds -- and the
  // browser default does not -- is a *solid* 2px outline with a 2px offset. Assert on those, not
  // on "not none" / "> 0".
  expect(ring!.style, `${ring!.tag} outline should be 'solid' (the app's rule), not the browser default 'auto'`).toBe(
    'solid'
  );
  expect(ring!.width, `${ring!.tag} outline width should be the app's 2px, not the browser default 1px`).toBe('2px');
  expect(ring!.offset, `${ring!.tag} outline offset should be the app's 2px, not the browser default 0px`).toBe(
    '2px'
  );

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
