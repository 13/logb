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
  await page.getByRole('button', { name: /Database/ }).click();
  await expect(page.getByRole('heading', { name: /Database/ })).toBeVisible();
  // SQLite is the default, and the section says so rather than showing a connection string.
  await expect(page.getByText(/SQLite/)).toBeVisible();

  // "Admin only" is a claim about somebody who is not one, so it is only proved by creating
  // such a user and looking again as them -- through the "Add user" form the Users page
  // offers, rather than reaching for a second way to make a user just for this spec.
  await page.goto('/settings');
  await page.getByRole('button', { name: /Users/ }).click();
  await page.getByLabel('Username', { exact: true }).fill('database-plain');
  await page.getByLabel('Password', { exact: true }).fill('password123');
  await page.getByRole('button', { name: 'Add user' }).click();
  await expect(page.getByText('database-plain')).toBeVisible();

  await page.goto('/settings');
  await page.getByRole('button', { name: /Account/ }).click();
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await page.getByLabel('Username', { exact: true }).fill('database-plain');
  await page.getByLabel('Password', { exact: true }).fill('password123');
  await page.getByRole('button', { name: /^Sign in$/ }).click();
  await page.waitForURL('**/');

  // The instance group -- Users, Database -- does not exist on this hub at all for a
  // non-admin, rather than existing and refusing entry.
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('button', { name: /Database/ })).not.toBeVisible();
});

// The redirect in App.svelte's ADMIN_ONLY guard is only proved by typing the URL directly --
// navigating there through the hub's own nav never reaches it, since the hub omits the row
// entirely for a non-admin (see the test above). Same "create a plain user, sign in as them"
// machinery as above, with its own distinctive username so the two tests never collide.
test('a non-administrator reaching /settings/database directly is sent back to the hub', async ({ page }) => {
  await signIn(page);
  await page.goto('/settings');
  await page.getByRole('button', { name: /Users/ }).click();
  await page.getByLabel('Username', { exact: true }).fill('database-direct');
  await page.getByLabel('Password', { exact: true }).fill('password123');
  await page.getByRole('button', { name: 'Add user' }).click();
  await expect(page.getByText('database-direct')).toBeVisible();

  await page.goto('/settings');
  await page.getByRole('button', { name: /Account/ }).click();
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await page.getByLabel('Username', { exact: true }).fill('database-direct');
  await page.getByLabel('Password', { exact: true }).fill('password123');
  await page.getByRole('button', { name: /^Sign in$/ }).click();
  await page.waitForURL('**/');

  await page.goto('/settings/database');
  await expect(page).toHaveURL(/\/settings$/);
});

// The Playwright suite runs on SQLite with no backup directory configured, so this is the "off"
// case: the screen must say backups are not being taken and name the variable that turns them
// on. The PostgreSQL wording is covered by tests/database_api.rs, which has a server.
test('settings says whether backups are being taken', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Database/ }).click();
  await expect(page.getByRole('heading', { name: /Backup/ })).toBeVisible();
  await expect(page.getByText(/LOGB_BACKUP_DIR/)).toBeVisible();
});
