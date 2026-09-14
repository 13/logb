import { expect, test } from '@playwright/test';
import { signInFresh } from './helpers';

/**
 * The timeline pages at 100 entries and "Show N older" appends the next page. An outbox flush
 * runs on every `visibilitychange` -- switching away from the tab and back is enough -- and
 * every completed pass notifies the mounted views, whether or not the queue held anything at
 * all. ObjectDetail reloaded the timeline from page one on each of those, so returning to the
 * tab silently threw away every extra page the user had loaded.
 */
test('returning to the tab keeps the extra pages the user loaded', async ({ page, context }) => {
  await signInFresh(page, '05-pagination');

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Fleet');
  await page.getByLabel('Type').selectOption('other');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Fleet' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];

  // Two pages' worth plus a few, seeded through the API: this test is about paging, not about
  // the form, and 105 trips through the UI would dominate the suite's runtime.
  const seeded = 105;
  for (let i = 0; i < seeded; i++) {
    const res = await page.request.post(`/api/objects/${objectId}/activities`, {
      data: { date: '2026-01-01', category: 'maintenance', title: `Entry ${i}`, notes: '' },
    });
    expect(res.ok()).toBe(true);
  }

  await page.reload();
  const entries = page.locator('button.entry');
  await expect(entries).toHaveCount(100);

  // Appending a page is itself a regression guard: the load used to read `activities` inside
  // the `oid`/`category` effect, so the effect depended on the list it assigned, re-ran on its
  // own result, and reset to page one -- "Show N older" fetched a page and lost it instantly.
  await page.getByRole('button', { name: /Show 5 older/ }).click();
  await expect(entries).toHaveCount(seeded);

  const refetches: string[] = [];
  page.on('request', (r) => { if (r.url().includes('/activities?')) refetches.push(r.url()); });
  // Leave the tab and come back: this is what fires a flush, and with it the notification
  // every mounted view listens for. The queue is empty, so nothing about the timeline changed.
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
    document.dispatchEvent(new Event('visibilitychange', { bubbles: true }));
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    document.dispatchEvent(new Event('visibilitychange', { bubbles: true }));
  });

  // Given a moment for a reload to have happened if one were still triggered.
  await page.waitForTimeout(500);
  await expect(entries).toHaveCount(seeded);
  // And it did not merely re-fetch its way back to the same list: a pass that changed nothing
  // this view renders is not a reason to talk to the server at all.
  expect(refetches).toEqual([]);
});


/**
 * The chunk loop that lets a refresh cover more than one page must stop on a SHORT page, not
 * only an empty one. An ordinary object has fewer activities than a page, so stopping only at
 * zero costs every single load a second, guaranteed-empty round trip -- and its COUNT(*) with
 * it. Asserted on a small object, since a full first page cannot tell the two conditions apart.
 */
test('an ordinary object loads its timeline in one request', async ({ page }) => {
  await signInFresh(page, '05-pagination');

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Kettle');
  await page.getByLabel('Type').selectOption('other');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Kettle' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];

  for (let i = 0; i < 3; i++) {
    await page.request.post(`/api/objects/${objectId}/activities`, {
      data: { date: '2026-01-01', category: 'maintenance', title: `Descale ${i}`, notes: '' },
    });
  }

  const loads: string[] = [];
  page.on('request', (r) => { if (r.url().includes('/activities?')) loads.push(r.url()); });
  await page.reload();
  await expect(page.locator('button.entry')).toHaveCount(3);
  await page.waitForLoadState('networkidle');

  expect(loads).toHaveLength(1);
});
