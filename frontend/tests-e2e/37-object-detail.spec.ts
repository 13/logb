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

/** Four tabs and a two-digit due badge on a 360 px phone: the tab row must not scroll sideways,
 *  or the last tab ("Info") is clipped. German labels are the longer ones. */
async function tabsFitAt360(page: Page, label: string): Promise<void> {
  await page.setViewportSize({ width: 360, height: 740 });
  await signInFresh(page, label);
  const id = await object(page, { name: 'Many due', type: 'car', counter_unit: 'km' });
  for (let i = 0; i < 12; i++) {
    const res = await page.request.post(`/api/objects/${id}/reminders`, { data: { title: `Due ${i}`, due_date: '2000-01-01' } });
    expect(res.ok()).toBe(true);
  }
  await page.goto(`/objects/${id}`);
  await expect(page.getByTestId('tab-due-badge')).toHaveText('12');
  const fits = await page.getByRole('tablist').evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth }));
  expect(fits.scroll).toBeLessThanOrEqual(fits.client);
}

test('four tabs and a two-digit badge fit a 360 px phone (English)', async ({ page }) => {
  await tabsFitAt360(page, '37-fit-en');
});

test.describe('German', () => {
  test.use({ locale: 'de-DE' });
  test('four tabs and a two-digit badge fit a 360 px phone (German)', async ({ page }) => {
    await tabsFitAt360(page, '37-fit-de');
    await expect(page.getByRole('tab', { name: /^Erinnerungen/ })).toBeVisible();
  });
});
