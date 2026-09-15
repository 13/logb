import type { Page } from '@playwright/test';

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
