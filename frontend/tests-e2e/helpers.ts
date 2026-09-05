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
