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
