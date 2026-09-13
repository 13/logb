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

test('on a wide desktop viewport, the FAB stays anchored to the content pane, not the bare viewport edge', async ({ page }) => {
  await signIn(page);
  // Wide enough that `main`'s 1100px cap opens a gutter between it and the viewport edge (see
  // the comment on `.fab` in app.css). `setViewportSize` overrides the project's own viewport
  // for this one test.
  await page.setViewportSize({ width: 1600, height: 900 });

  // Force the FAB itself into existence regardless of run order: run alone, this spec's
  // database has no objects yet, and the dashboard swaps the FAB for a plain inline button in
  // that case. Toggling "archived" satisfies the same `objects.length > 0 || archived`
  // condition the FAB is gated on without depending on data another spec left behind.
  await page.getByRole('button', { name: /Show archived|Archivierte anzeigen/ }).click();

  const main = page.locator('main');
  const fab = page.getByRole('button', { name: /New object/ });
  await expect(fab).toBeVisible();

  const mainBox = await main.boundingBox();
  const fabBox = await fab.boundingBox();
  if (!mainBox || !fabBox) throw new Error('no main or no FAB box');
  // The FAB's usual clearance from whatever edge it hugs -- `--space-5` on desktop, the same
  // value the un-gutted case already uses between the FAB and the bare viewport edge.
  const clearance = await page.evaluate(() =>
    parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--space-5')),
  );

  // The design spec: the FAB sits at the bottom-right of the content pane, clear of the
  // sidebar -- a constant `--space-5` from `main`'s right edge, not drifting further out in the
  // gutter that opens once `main` hits its 1100px cap. A `position: fixed` FAB measures `right`
  // from the viewport, so on a wide screen that constant gap has to be computed deliberately
  // rather than falling out of a plain `right: var(--space-5)`.
  const gap = mainBox.x + mainBox.width - clearance - (fabBox.x + fabBox.width);
  expect(
    Math.abs(gap),
    "the FAB's right edge should sit one clearance inside main's right edge, not drift out into the gutter beyond it",
  ).toBeLessThan(4);
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
