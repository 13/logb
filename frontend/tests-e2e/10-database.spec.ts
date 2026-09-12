import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

// Seeds its own object with a distinctive name: one server and one database are shared across
// the whole run.
//
// Only the rendering is driven from here, and deliberately so. The switch itself needs a
// PostgreSQL server to copy into, which the Playwright run does not have -- it runs against the
// one SQLite instance `playwright.config.ts` starts. Copying, verifying, the pointer file and
// the refusals around them are covered end-to-end in `tests/database_api.rs`, which does have a
// PostgreSQL server when one is configured. This is not an omission.
test('the database section shows where the data is, and is admin only', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: /Database/ })).toBeVisible();
  // SQLite is the default, and the section says so rather than showing a connection string.
  await expect(page.getByText(/SQLite/)).toBeVisible();

  // "Admin only" is a claim about somebody who is not one, so it is only proved by creating
  // such a user and looking again as them -- through the "Add user" form Settings already
  // offers, rather than reaching for a second way to make a user just for this spec.
  await page.getByLabel('Username', { exact: true }).fill('database-plain');
  await page.getByLabel('Password', { exact: true }).fill('password123');
  await page.getByRole('button', { name: 'Add user' }).click();
  await expect(page.getByText('database-plain')).toBeVisible();

  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await page.getByLabel('Username', { exact: true }).fill('database-plain');
  await page.getByLabel('Password', { exact: true }).fill('password123');
  await page.getByRole('button', { name: /^Sign in$/ }).click();
  await page.waitForURL('**/');

  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: /Database/ })).not.toBeVisible();
});
