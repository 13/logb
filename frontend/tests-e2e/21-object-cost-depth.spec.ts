import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function entry(page: Page, id: number, data: Record<string, unknown>) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', title: 'Entry', ...data } });
  expect(res.ok()).toBe(true);
}

async function openInfo(page: Page, id: number) {
  await page.goto(`/objects/${id}`);
  await page.getByRole('button', { name: 'Info', exact: true }).click();
}

test('a car shows consumption per fill and no contents switch', async ({ page }) => {
  await signInFresh(page, '21-cost-car');
  const car = await object(page, { name: 'Depth Car', type: 'car', counter_unit: 'km' });
  for (const [date, counter_value, quantity_milli] of [['2026-01-01', 10_000, 40_000], ['2026-02-01', 10_500, 30_000], ['2026-03-01', 11_000, 25_000]]) {
    await entry(page, car as number, { date, category: 'fuel', counter_value, quantity_milli });
  }

  await openInfo(page, car);
  await expect(page.getByTestId('insights-by-fill').locator('.bar-row')).toHaveCount(2);
  await expect(page.getByLabel('Include contents')).toHaveCount(0);
});

test('a house includes its boiler on request, and remembers the choice', async ({ page }) => {
  await signInFresh(page, '21-cost-house');
  const house = await object(page, { name: 'Depth House', type: 'home', purchase_date: '2024-05-01', purchase_price_cents: 300_000 });
  const boiler = await object(page, { name: 'Depth Boiler', type: 'appliance', parent_id: house });
  await entry(page, house, { date: '2025-03-10', category: 'repair', cost_cents: 100_000 });
  await entry(page, boiler, { date: '2026-02-01', category: 'maintenance', cost_cents: 25_000 });

  await openInfo(page, house);
  const ownership = page.getByTestId('insights-ownership');
  await expect(ownership).toContainText('4,000.00');
  const toggle = page.getByLabel('Include contents');
  await expect(toggle).not.toBeChecked();
  await toggle.check();
  await expect(ownership).toContainText('4,250.00');

  await page.reload();
  await page.getByRole('button', { name: 'Info', exact: true }).click();
  await expect(page.getByLabel('Include contents')).toBeChecked();
  await expect(page.getByTestId('insights-ownership')).toContainText('4,250.00');
});
