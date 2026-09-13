import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('the hub lists what there is, and each row opens its own page', async ({ page }) => {
  await signIn(page);
  await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).getByRole('button', { name: 'Settings' }).click();
  await expect(page).toHaveURL(/\/settings$/);

  for (const [name, url] of [
    ['Appearance', /\/settings\/appearance$/],
    ['Account', /\/settings\/account$/],
    ['API access', /\/settings\/api$/],
  ] as const) {
    await page.getByRole('button', { name: new RegExp(name) }).click();
    await expect(page).toHaveURL(url);
    await page.goBack();
    await expect(page).toHaveURL(/\/settings$/);
  }
});

/// The value on a row is the reason the hub is a hub rather than a menu: it answers the
/// question without being opened.
test('a row carries its current value', async ({ page }) => {
  await signIn(page);
  await page.goto('/settings');
  // `signIn` uses the admin account, so the account row shows its username.
  await expect(page.getByRole('button', { name: /Account/ })).toContainText('ben');
});

test('an administrator sees the instance group; the rows are real links', async ({ page }) => {
  await signIn(page);
  await page.goto('/settings');
  await expect(page.getByRole('button', { name: /Database/ })).toBeVisible();
  await page.getByRole('button', { name: /Database/ }).click();
  await expect(page).toHaveURL(/\/settings\/database$/);
  // The database page carries the backup section too -- they are one page deliberately.
  await expect(page.getByText(/Backup|Sicherung/).first()).toBeVisible();
});
