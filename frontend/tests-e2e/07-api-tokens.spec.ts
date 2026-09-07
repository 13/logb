import { expect, test } from '@playwright/test';
import { signIn } from './helpers';

/**
 * An API token is what lets something that is not a browser -- a phone app, a script -- talk to
 * this backend. The plaintext is returned exactly once and stored only as a hash, so the one
 * moment it can be read is the moment it is created; if that does not reach the screen, the
 * token is unusable and the only remedy is to revoke it and start again.
 */
test('a token can be created, used, and revoked from Settings', async ({ page, playwright }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();

  await page.getByLabel('What is this token for?').fill('phone');
  await page.getByRole('button', { name: 'Create token' }).click();

  const shown = page.locator('.fresh-token code');
  await expect(shown).toBeVisible();
  const token = (await shown.innerText()).trim();
  expect(token).toMatch(/^memto_pat_[0-9a-f]{64}$/);

  // It is listed afterwards by name and prefix, and never in full again.
  await expect(page.getByText('phone')).toBeVisible();
  await expect(page.getByText('never used')).toBeVisible();
  await expect(page.locator('.list').getByText(token)).toHaveCount(0);

  // A request context of its own, so it carries no cookie and the header is the only credential
  // in play -- which is the whole point of the token.
  const client = await playwright.request.newContext({ baseURL: 'http://127.0.0.1:8099' });
  try {
    expect((await client.get('/api/objects')).status()).toBe(401);
    const authed = await client.get('/api/objects', { headers: { Authorization: `Bearer ${token}` } });
    expect(authed.ok()).toBe(true);

    page.once('dialog', (d) => d.accept());
    await page.getByRole('button', { name: 'Revoke' }).click();
    await expect(page.getByText('No tokens yet.')).toBeVisible();

    const after = await client.get('/api/objects', { headers: { Authorization: `Bearer ${token}` } });
    expect(after.status()).toBe(401);
  } finally {
    await client.dispose();
  }
});
