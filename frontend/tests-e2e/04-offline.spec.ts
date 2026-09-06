import { expect, test } from '@playwright/test';
import { signIn } from './helpers';

test('an activity logged offline is replayed to the server exactly once', async ({ page, context, browser }) => {
  await signIn(page);

  // create the object
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Trailer');
  await page.getByLabel('Category').fill('trailer');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Trailer' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];
  expect(objectId).toBeTruthy();

  await page.getByRole('button', { name: /Log/ }).first().click();

  await context.setOffline(true);
  await page.getByLabel('Title').fill('Fuel');
  await page.getByRole('button', { name: 'Save' }).click();
  // Visible immediately from the local outbox queue -- before the write has ever reached the
  // server. This alone proves nothing about replay: a build with flushOutbox removed entirely
  // would still show this.
  await expect(page.getByText('Fuel', { exact: true })).toBeVisible();

  await context.setOffline(false);
  // The reconnect fires the outbox's 'online' listener; give the flush a moment to land, then
  // the UI should still show exactly one entry (merging a queued op with its own replayed row
  // would be a duplicate-visible-entry bug).
  await expect(page.getByText('Fuel', { exact: true })).toHaveCount(1);

  // Server truth: ask the backend directly, through a path no local/queued state can satisfy.
  // `page.request` shares this browser context's cookies, so it is genuinely authenticated,
  // but it never touches the page's own IndexedDB or in-memory state -- if flushOutbox were
  // deleted, or if replay duplicated the row, this is what would catch it.
  await expect(async () => {
    const res = await page.request.get(`/api/objects/${objectId}/activities`);
    expect(res.ok()).toBe(true);
    const activities = (await res.json()) as Array<{ title: string }>;
    expect(activities.filter((a) => a.title === 'Fuel')).toHaveLength(1);
  }).toPass();

  // Belt and braces: a second browser context has its own empty IndexedDB and cookie jar, so
  // anything it shows for this object came only from the server, not from replaying local
  // outbox state a second time.
  const otherContext = await browser.newContext();
  const otherPage = await otherContext.newPage();
  try {
    await signIn(otherPage);
    await otherPage.goto(`/objects/${objectId}`);
    await expect(otherPage.getByText('Fuel', { exact: true })).toHaveCount(1);
  } finally {
    await otherContext.close();
  }
});
