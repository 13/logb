import { expect, test } from '@playwright/test';
import { signInFresh } from './helpers';

/** The dashboard's reminder area: one card per due reminder with a real snooze button, calm rows
 *  for what is coming up, and no object tags on either (they belong to the object, not the
 *  reminder). */
test('due reminders are cards with a snooze button; upcoming ones are plain rows', async ({ page }) => {
  await signInFresh(page, '36-dashboard-reminders');
  const obj = await page.request.post('/api/objects', { data: { name: 'Dash reminder car', type: 'car', counter_unit: 'km', tags: ['family'] } });
  expect(obj.ok()).toBe(true);
  const id = (await obj.json()).id as number;
  const soonDate = new Date(Date.now() + 5 * 86_400_000).toISOString().slice(0, 10);
  for (const data of [{ title: 'Dash overdue', due_date: '2000-01-01' }, { title: 'Dash soon', due_date: soonDate }]) {
    expect((await page.request.post(`/api/objects/${id}/reminders`, { data })).ok()).toBe(true);
  }

  await page.goto('/');
  const due = page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' });
  await expect(due).toContainText('Dash reminder car');
  await expect(due.locator('.tag')).toHaveCount(0);
  await expect(due.locator('li')).toHaveCount(0);
  const snooze = due.getByRole('button', { name: /Snooze/ });
  const box = (await snooze.boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);

  const soon = page.getByTestId('upcoming-reminder').filter({ hasText: 'Dash soon' });
  await expect(soon).toContainText(/in 5 days/);
  await expect(soon.locator('.tag')).toHaveCount(0);

  await due.getByRole('link', { name: /Dash overdue/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=reminders`));

  await page.goto('/');
  await page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' }).getByRole('button', { name: /Snooze/ }).click();
  await expect(page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' })).toHaveCount(0);
});
