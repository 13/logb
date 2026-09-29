import { expect, test, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The object page as round 3 of the UI overhaul left it: tabs, summary, panes, timeline, charts.
 *  Seeds through the API so each test drives only the screen it is about. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id as number;
}

test('the tabs stay on one row and the due count sits inside the Reminders tab', async ({ page }) => {
  await signInFresh(page, '37-tabs');
  const id = await object(page, { name: 'Tabs car', type: 'car', counter_unit: 'km' });
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Tabs overdue', due_date: '2000-01-01' } })).ok()).toBe(true);
  await page.goto(`/objects/${id}`);

  const tabs = page.getByRole('tab');
  await expect(tabs.first()).toBeVisible();
  const boxes = await Promise.all((await tabs.all()).map((tab) => tab.boundingBox()));
  const ys = boxes.map((b) => Math.round(b!.y));
  // One row: the audit's red "1" dropped onto a second line under "Reminders".
  expect(Math.max(...ys) - Math.min(...ys)).toBeLessThanOrEqual(1);
  for (const b of boxes) expect(b!.height).toBeGreaterThanOrEqual(44);

  const reminders = page.getByRole('tab', { name: /^Reminders/ });
  await expect(reminders).toHaveAccessibleName('Reminders (1 due)');
  const tab = (await reminders.boundingBox())!;
  const badge = (await reminders.getByTestId('tab-due-badge').boundingBox())!;
  expect(badge.y).toBeGreaterThanOrEqual(tab.y);
  expect(badge.y + badge.height).toBeLessThanOrEqual(tab.y + tab.height);
  expect(tab.height).toBeLessThan(56);

  // A tab list moves with the arrow keys, and the address follows the tab.
  await page.getByRole('tab', { name: 'Timeline', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: 'Documents', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=documents$`));
});
