import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('an edit saved without a connection is sent once it is back', async ({ page, context }) => {
  await signInFresh(page, '19-offline-edit');
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Offline edit bike');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Offline edit bike' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)![1];

  const created = await page.request.post(`/api/objects/${objectId}/activities`, {
    data: { date: '2026-09-01', category: 'repair', title: 'Brakes' },
  });
  const activity = await created.json();

  await page.goto(`/objects/${objectId}/activities/${activity.id}`);
  await expect(page.getByLabel('Title')).toHaveValue('Brakes');

  await context.setOffline(true);
  await page.getByLabel('Title').fill('Brakes, rear');
  await page.getByRole('button', { name: 'Save' }).click();
  // Saved into the queue, not refused: the form moves on as it does for an offline create.
  await expect(page).toHaveURL(new RegExp(`/objects/${objectId}(\\?|$)`));

  await context.setOffline(false);
  await page.evaluate(() => window.dispatchEvent(new Event('online')));
  await expect.poll(async () => {
    const res = await page.request.get(`/api/activities/${activity.id}`);
    return (await res.json()).title;
  }, { timeout: 15000 }).toBe('Brakes, rear');
});
