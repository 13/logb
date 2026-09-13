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

  // The Users row must read "1 user", not "1 users" -- earlier specs sharing this database
  // (10-database creates a second user and never removes it) may have left more than the
  // signed-in admin behind, so clear them first: this assertion is about the singular form,
  // not about whatever count happens to be left over from another spec.
  await page.goto('/settings/people');
  for (;;) {
    const removeButtons = page.getByRole('button', { name: /Remove|Entfernen/ });
    const remaining = await removeButtons.count();
    if (remaining === 0) break;
    page.once('dialog', (d) => d.accept());
    await removeButtons.first().click();
    await expect(removeButtons).toHaveCount(remaining - 1);
  }
  await page.goto('/settings');
  // A regex without a word boundary would let "1 users" match too -- \b after "user" only
  // holds when nothing more follows, so this fails against the un-pluralised bug on purpose.
  await expect(page.getByRole('button', { name: /Users/ })).toContainText(/\b1 user\b/);
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
