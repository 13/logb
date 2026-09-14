import { expect, test } from '@playwright/test';
import { signInFresh } from './helpers';

/** Reads the queue straight out of IndexedDB, which is where the ordering actually lives. */
async function queue(page: import('@playwright/test').Page) {
  return page.evaluate(() => new Promise<Array<{ id: string; seq?: number; path: string }>>((resolve, reject) => {
    const req = indexedDB.open('logb-outbox');
    req.onsuccess = () => {
      const tx = req.result.transaction('ops', 'readonly');
      const all = tx.objectStore('ops').getAll();
      all.onsuccess = () => resolve(all.result.map((o) => ({ id: o.id, seq: o.seq, path: o.path })));
      all.onerror = () => reject(all.error);
    };
    req.onerror = () => reject(req.error);
  }));
}

/**
 * `seq` is what gives the queue a deterministic total order. It used to be handed out by a
 * per-tab counter seeded once from the store in a transaction of its own, so two tabs opening
 * the same queue at the same time handed out the SAME value -- and the order then fell back to
 * `getAll()`'s UUID key order, the exact arbitrariness `seq` exists to remove. It is now read
 * and written inside one readwrite transaction, which IndexedDB serialises across tabs.
 */
test('two tabs queueing at once never hand out the same seq', async ({ context }) => {
  const a = await context.newPage();
  await signInFresh(a, '06-outbox-order');
  await a.getByRole('button', { name: /New object/ }).click();
  await a.getByLabel('Name').fill('Workshop');
  await a.getByLabel('Type').selectOption('other');
  await a.getByRole('button', { name: 'Save' }).click();
  await expect(a.getByRole('heading', { name: 'Workshop' })).toBeVisible();
  const objectId = a.url().match(/\/objects\/(\d+)/)?.[1];

  // A second tab on the same origin, so the same IndexedDB.
  const b = await context.newPage();
  await b.goto(`/objects/${objectId}`);
  await expect(b.getByRole('heading', { name: 'Workshop' })).toBeVisible();

  await context.setOffline(true);

  // Both tabs queue at the same time, which is what used to collide.
  const log = async (page: import('@playwright/test').Page, title: string) => {
    await page.getByRole('button', { name: /Log/ }).first().click();
    await page.getByLabel('Title').fill(title);
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText(title, { exact: true })).toBeVisible();
  };
  await Promise.all([log(a, 'From tab A'), log(b, 'From tab B')]);

  const rows = await queue(a);
  expect(rows).toHaveLength(2);
  const seqs = rows.map((r) => r.seq);
  expect(seqs.every((s) => typeof s === 'number')).toBe(true);
  expect(new Set(seqs).size).toBe(2);
  // Both tabs read the same queue, so both must agree on the order it is in.
  expect(await queue(b)).toEqual(await queue(a));

  await context.setOffline(false);
  await a.close();
  await b.close();
});

/**
 * A record written before `seq` existed and never given one by the v2 backfill is not in the
 * index at all, so the maximum read from that index says nothing about it. `Date.now()` is
 * therefore a floor on every newly assigned `seq`, keeping it strictly after such a record --
 * whose order comes from `queued_at`, epoch milliseconds on the same scale.
 */
test('a newly queued op sorts after a record that predates seq', async ({ page, context }) => {
  await signInFresh(page, '06-outbox-order');
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Shed');
  await page.getByLabel('Type').selectOption('other');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Shed' })).toBeVisible();

  // A legacy row: no `seq`, only the `queued_at` the old code ordered by.
  await page.evaluate(() => new Promise<void>((resolve, reject) => {
    const req = indexedDB.open('logb-outbox');
    req.onsuccess = () => {
      const tx = req.result.transaction('ops', 'readwrite');
      tx.objectStore('ops').put({
        id: 'legacy-op', kind: 'activity.create', path: '/objects/1/activities',
        body: { title: 'Older' }, attempts: 0, queued_at: Date.now(),
      });
      tx.oncomplete = () => resolve();
      tx.onabort = () => reject(tx.error);
    };
    req.onerror = () => reject(req.error);
  }));

  await context.setOffline(true);
  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Newer');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Newer', { exact: true })).toBeVisible();

  const rows = await queue(page);
  const legacy = rows.find((r) => r.id === 'legacy-op')!;
  const fresh = rows.find((r) => r.id !== 'legacy-op')!;
  expect(legacy.seq).toBeUndefined();
  expect(fresh.seq!).toBeGreaterThan(Date.now() - 60_000);

  await context.setOffline(false);
});
