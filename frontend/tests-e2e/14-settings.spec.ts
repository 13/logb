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

/**
 * The hub renders its failed-sync banner from `deadOps()`, which reads the outbox's dead
 * entries straight out of IndexedDB -- nothing exercised that path before. `replay` (see
 * `../src/lib/outbox.ts`) parks a queued op dead the instant the server answers with a 4xx,
 * rather than after `MAX_ATTEMPTS` retries, so the quickest way to a dead op from the UI alone
 * is a write whose target the server has permanently rejected by the time it is replayed: an
 * activity queued for an object that gets deleted -- from a second, still-online session --
 * while the first is offline. Same technique 04-offline.spec.ts uses to drive the outbox
 * (queue while offline, act from elsewhere, come back online), aimed at the one outcome that
 * makes an op permanently unsendable instead of eventually resent.
 */
test('the failed-sync banner appears only when the queue holds a dead operation', async ({ page, context, browser }) => {
  await signIn(page);

  // Clean queue, freshly signed in: no banner at all.
  await page.goto('/settings');
  // `exact` matters: TopBar shows its own persistent "N could not be sent" chip whenever any
  // dead op exists anywhere in the queue, and a loose substring match would find that chip too.
  await expect(page.getByText('Could not be sent', { exact: true })).toHaveCount(0);

  await page.goto('/');
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Settings dead-op fixture');
  await page.getByLabel('Type').selectOption('other');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Settings dead-op fixture' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];
  expect(objectId).toBeTruthy();

  await page.getByRole('button', { name: /Log/ }).first().click();
  await context.setOffline(true);
  await page.getByLabel('Title').fill('Doomed entry');
  await page.getByRole('button', { name: 'Save' }).click();
  // Queued locally, before the write has reached (or could reach) the server.
  await expect(page.getByText('Doomed entry', { exact: true })).toBeVisible();

  // Delete the object from a second, still-online session while the first stays offline: the
  // queued create now names an object that will never exist by the time it is replayed.
  const otherContext = await browser.newContext();
  const otherPage = await otherContext.newPage();
  try {
    await signIn(otherPage);
    const deleted = await otherPage.request.delete(`/api/objects/${objectId}`);
    expect(deleted.ok()).toBe(true);
  } finally {
    await otherContext.close();
  }

  await context.setOffline(false);
  // The replay this reconnect triggers races the navigation below, so poll: each `goto` is a
  // fresh page load, which itself calls `flushOutbox()` at module start (see `main.ts`), and
  // the object's create gets a 404 -- a permanent rejection -- the first time it is attempted.
  await expect(async () => {
    await page.goto('/settings');
    await expect(page.getByText('Could not be sent', { exact: true })).toBeVisible({ timeout: 2000 });
  }).toPass();

  const deadRow = page.locator('.card.row', { hasText: 'Doomed entry' });
  await expect(deadRow).toBeVisible();

  page.once('dialog', (d) => d.accept());
  await deadRow.getByRole('button', { name: 'Discard' }).click();
  await expect(page.getByText('Could not be sent', { exact: true })).toHaveCount(0);
});
