import { expect, type Page } from '@playwright/test';

export const ADMIN = { username: 'ben', password: 'correct horse' };

/** Completes first-run setup, or signs in when the instance already has users. */
export async function signIn(page: Page): Promise<void> {
  await page.goto('/');
  if (await page.getByRole('button', { name: /Create admin|Admin anlegen/ }).isVisible().catch(() => false)) {
    await page.getByLabel(/Username|Benutzername/).fill(ADMIN.username);
    await page.getByLabel(/Password|Passwort/).fill(ADMIN.password);
    await page.getByRole('button', { name: /Create admin|Admin anlegen/ }).click();
  } else {
    await page.getByLabel(/Username|Benutzername/).fill(ADMIN.username);
    await page.getByLabel(/Password|Passwort/).fill(ADMIN.password);
    await page.getByRole('button', { name: /^Sign in$|^Anmelden$/ }).click();
  }
  await page.waitForURL('**/');
}

const FRESH_PASSWORD = 'password123';
let fresh = 0;

/**
 * Signs in as a brand-new, non-admin user of this spec's own, and answers the username.
 *
 * Every project shares one database, so a spec signed in as the admin sees every object any
 * earlier spec left behind: a count, a first card, an empty state -- anything it reads can
 * depend on run order. A user nobody else has ever signed in as starts with nothing, whichever
 * specs ran first and however many times. Specs that need the administrator (users, the
 * database, instance settings) keep `signIn`.
 */
export async function signInFresh(page: Page, label: string): Promise<string> {
  await signIn(page);
  // Usernames are 3-32 characters of letters, digits, `_ . -`: the label is trimmed to fit, and
  // the tail of the clock plus a counter keeps two calls in one millisecond apart.
  const tag = label.replace(/[^A-Za-z0-9]/g, '').slice(0, 14);
  const username = `e2e-${tag}-${Date.now().toString(36).slice(-6)}${(fresh++).toString(36)}`;
  const created = await page.request.post('/api/users', { data: { username, password: FRESH_PASSWORD, is_admin: false } });
  if (!created.ok()) throw new Error(`could not create ${username}: ${created.status()} ${await created.text()}`);
  await page.request.post('/api/auth/logout');
  await page.goto('/login');
  await page.getByLabel(/Username|Benutzername/).fill(username);
  await page.getByLabel(/Password|Passwort/).fill(FRESH_PASSWORD);
  await page.getByRole('button', { name: /^Sign in$|^Anmelden$/ }).click();
  await page.waitForURL('**/');
  return username;
}

/** The service worker must control the page before an offline reload can be served from it. */
export async function underServiceWorker(page: Page) {
  // Asked BEFORE waiting: a page the worker claims only later has already made its reads past
  // the worker, so `logb-api` does not hold them. With the start-up requests running in
  // parallel and every page chunk in the precache, the first screen's reads regularly finish
  // before the worker has installed and claimed the page.
  // Whether the page's own load went through the worker, not whether a worker controls it now:
  // `clientsClaim()` can take the page over part-way through its first reads, which sets
  // `controller` while some of those reads have already gone past the worker. `workerStart` is
  // only non-zero for a navigation the worker itself answered.
  const controlledFromTheStart = await page.evaluate(() => {
    const nav = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
    return (nav?.workerStart ?? 0) > 0;
  });
  await page.evaluate(async () => { await navigator.serviceWorker.ready; });
  // The built `sw.js` calls `clientsClaim()`, so an open page is taken over once the worker
  // activates -- but its reads so far went past it. A page that was not controlled from the
  // start is reloaded, because a load that starts under an active worker is controlled for
  // certain, and so are its reads.
  if (!controlledFromTheStart) await page.reload();
  await expect.poll(() => page.evaluate(() => !!navigator.serviceWorker.controller)).toBe(true);
}

/** A 1×1 PNG as a Playwright file payload. */
export function pngPayload(name = 'photo.png') {
  return {
    name,
    mimeType: 'image/png',
    buffer: Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
      'base64',
    ),
  };
}

/**
 * A tiny (4×4) JPEG carrying a real EXIF `DateTimeOriginal` of 2025-12-25 10:00 -- built with
 * Pillow and round-tripped through the server's own upload endpoint to confirm
 * `process_image()` (src/files.rs) reads it back as `taken_at: "2025-12-25T10:00:00"` before
 * this was hardcoded here. Exists so a test can drive ActivityForm's "Use photo date" hint --
 * the one place in the app that changes a `DateInput`'s bound value from outside the component
 * itself, rather than through its own text field or calendar picker.
 */
export function jpegWithExifPayload(name = 'photo.jpg') {
  return {
    name,
    mimeType: 'image/jpeg',
    buffer: Buffer.from(
      '/9j/4AAQSkZJRgABAQAAAQABAAD/4QBIRXhpZgAATU0AKgAAAAgAAYdpAAQAAAABAAAAGgAAAAAAAZADAAIAAAAUAAAALAAAAAAyMDI1' +
      'OjEyOjI1IDEwOjAwOjAwAP/bAEMACAYGBwYFCAcHBwkJCAoMFA0MCwsMGRITDxQdGh8eHRocHCAkLicgIiwjHBwoNyksMDE0NDQfJzk9' +
      'ODI8LjM0Mv/bAEMBCQkJDAsMGA0NGDIhHCEyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMv/A' +
      'ABEIAAQABAMBIgACEQEDEQH/xAAfAAABBQEBAQEBAQAAAAAAAAAAAQIDBAUGBwgJCgv/xAC1EAACAQMDAgQDBQUEBAAAAX0BAgMABBE' +
      'FEiExQQYTUWEHInEUMoGRoQgjQrHBFVLR8CQzYnKCCQoWFxgZGiUmJygpKjQ1Njc4OTpDREVGR0hJSlNUVVZXWFlaY2RlZmdoaWpzdH' +
      'V2d3h5eoOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4eLj5OXm5+jp6vHy8/T19vf4' +
      '+fr/xAAfAQADAQEBAQEBAQEBAAAAAAAAAQIDBAUGBwgJCgv/xAC1EQACAQIEBAMEBwUEBAABAncAAQIDEQQFITEGEkFRB2FxEyIygQ' +
      'gUQpGhscEJIzNS8BVictEKFiQ04SXxFxgZGiYnKCkqNTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqCg4SFhoeI' +
      'iYqSk5SVlpeYmZqio6Slpqeoqaqys7S1tre4ubrCw8TFxsfIycrS09TV1tfY2dri4+Tl5ufo6ery8/T19vf4+fr/2gAMAwEAAhEDE' +
      'QA/AOeoooryT9DP/9k=',
      'base64',
    ),
  };
}

/**
 * Starts a new entry on the open object page. "+ Log" is a menu when the object offers more
 * than activities (a trip, a fill or charge) and the action itself when it does not; an empty
 * timeline shows its own buttons instead. This finds whichever the page has.
 */
export async function logEntry(page: Page, name: RegExp): Promise<void> {
  const menu = page.getByTestId('log-menu');
  const direct = page.getByRole('button', { name });
  // The empty timeline renders (with its own buttons) before the activities load; let the page
  // settle first, so a button that is about to vanish is not the one picked.
  await page.waitForLoadState('networkidle');
  await expect(menu.or(direct).first()).toBeVisible();
  const viaMenu = async () => {
    await menu.click();
    await page.getByRole('menuitem', { name }).click();
  };
  if (await menu.isVisible()) await viaMenu();
  else {
    try { await direct.first().click({ timeout: 2000 }); } catch { await viaMenu(); }
  }
}

/**
 * Opens the object page's Info tab where there is one. From 1024 px the summary pane on the left
 * holds the same content and there is no Info tab, so there is nothing to open. Waits for the
 * tab list first, so "no Info tab" is an answer about the page, not about a page still loading.
 */
export async function openInfo(page: Page): Promise<void> {
  const tabs = page.getByRole('tablist');
  await expect(tabs).toBeVisible();
  const info = tabs.getByRole('tab', { name: 'Info', exact: true });
  if ((await info.count()) > 0) await info.click();
}

/** Opens a form's "More details" section if it is closed. Safe to call when it is open already. */
export async function openMoreDetails(page: Page): Promise<void> {
  // A form that loads its data marks itself aria-busy until it has applied it (and opened "More
  // details" itself if the data uses it): reading the toggle before then is a race.
  await expect(page.locator('form[aria-busy="true"]')).toHaveCount(0);
  const toggle = page.getByRole('button', { name: /^(More details|Weitere Angaben)$/ });
  await expect(async () => {
    if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'true', { timeout: 1000 });
  }).toPass();
}

const TYPE_LABELS: Record<string, string> = {
  car: 'Car', e_bike: 'E-bike', bike: 'Bicycle', motorcycle: 'Motorcycle', home: 'Home',
  appliance: 'Appliance', tool: 'Tool', body: 'Body', other: 'Other',
};

/** The object form's tile for `type`: a built-in type by key ('car'), an own type by its name. */
export function typeTile(page: Page, type: string) {
  return page.getByRole('group', { name: 'Type', exact: true }).getByRole('radio', { name: TYPE_LABELS[type] ?? type, exact: true });
}

/** Picks the object's type on the object form. A new object has none until one is picked. */
export async function chooseType(page: Page, type: string): Promise<void> {
  await typeTile(page, type).check();
}
