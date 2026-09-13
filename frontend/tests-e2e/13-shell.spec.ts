import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

/// The nav is one element with two presentations, so "is it the right one" is a question about
/// layout, not about which markup rendered. A sidebar sits at the left edge; a tab bar sits at
/// the bottom. Asserting on position is what actually distinguishes them.
test('navigation reaches every destination, and marks where you are', async ({ page }) => {
  await signIn(page);

  const nav = page.getByRole('navigation', { name: /Main|Hauptnavigation/ });
  await expect(nav).toBeVisible();

  await nav.getByRole('button', { name: 'Search' }).click();
  await expect(page).toHaveURL(/\/search$/);
  await expect(nav.getByRole('button', { name: 'Search' })).toHaveAttribute('aria-current', 'page');

  await nav.getByRole('button', { name: 'Settings' }).click();
  await expect(page).toHaveURL(/\/settings$/);
  await expect(nav.getByRole('button', { name: 'Settings' })).toHaveAttribute('aria-current', 'page');

  await nav.getByRole('button', { name: 'Objects' }).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(nav.getByRole('button', { name: 'Objects' })).toHaveAttribute('aria-current', 'page');
});

test('a drill-down three screens deep still shows which destination it belongs to', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await expect(page).toHaveURL(/\/objects\/new$/);

  const nav = page.getByRole('navigation', { name: /Main|Hauptnavigation/ });
  await expect(nav.getByRole('button', { name: 'Objects' })).toHaveAttribute('aria-current', 'page');
});

test('the nav sits where the viewport can afford it, and never covers the main action', async ({ page }, testInfo) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).waitFor().catch(() => {});

  const nav = page.getByRole('navigation', { name: /Main|Hauptnavigation/ });
  const navBox = await nav.boundingBox();
  const viewport = page.viewportSize();
  if (!navBox || !viewport) throw new Error('no nav or no viewport');

  if (testInfo.project.name === 'desktop') {
    // A sidebar: against the left edge, tall, and narrow.
    expect(navBox.x).toBeLessThan(2);
    expect(navBox.height).toBeGreaterThan(viewport.height / 2);
    expect(navBox.width).toBeLessThan(viewport.width / 2);
  } else {
    // A tab bar: against the bottom edge, full width, short.
    expect(navBox.y + navBox.height).toBeGreaterThan(viewport.height - 2);
    expect(navBox.width).toBeGreaterThan(viewport.width - 2);
    expect(navBox.height).toBeLessThan(viewport.height / 3);
  }

  // The FAB is this app's primary action. A nav that covers it is a nav that broke the app.
  const fab = page.getByRole('button', { name: /New object/ });
  if (await fab.isVisible()) {
    const fabBox = await fab.boundingBox();
    if (!fabBox) throw new Error('no FAB box');
    const overlaps =
      fabBox.x < navBox.x + navBox.width && fabBox.x + fabBox.width > navBox.x &&
      fabBox.y < navBox.y + navBox.height && fabBox.y + fabBox.height > navBox.y;
    expect(overlaps, 'the FAB must not sit underneath the nav').toBe(false);
  }
});

test('the version is on screen without opening Settings', async ({ page }) => {
  await signIn(page);
  // The footer is rendered twice -- once inside the nav (for the sidebar) and once in the
  // content area (for the tab bar) -- and CSS, not Svelte, decides which one is display:none
  // at the current breakpoint (see the comment on `.appnav` in app.css). The sidebar copy is
  // first in DOM order on both viewports, so plain `.first()` finds it on desktop, where it is
  // the visible one, but keeps finding it -- now hidden -- on mobile, where the *other* copy is
  // the one actually on screen. Intersecting with `:visible` follows whichever copy CSS is
  // actually showing, instead of assuming DOM order tracks visibility.
  await expect(page.getByText(/Version/).and(page.locator(':visible'))).toBeVisible();
});

test('signing in is offered without a nav, since there is nowhere yet to go', async ({ page }) => {
  await page.goto('/login');
  await expect(page.getByRole('navigation', { name: /Main|Hauptnavigation/ })).toHaveCount(0);
});
