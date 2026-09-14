import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function cost(page: Page, id: number, date: string, category: string, cost_cents: number) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { date, category, title: category, notes: '', cost_cents } });
  expect(res.ok()).toBe(true);
}

test('statistics total everything, roll a boiler into its house, and remember the purchase toggle', async ({ page }) => {
  await signInFresh(page, '20-statistics');
  const house = await object(page, { name: 'Stats House', type: 'home', purchase_date: '2024-05-01', purchase_price_cents: 300_000 });
  const boiler = await object(page, { name: 'Stats Boiler', type: 'appliance', parent_id: house });
  const car = await object(page, { name: 'Stats Car', type: 'car', counter_unit: 'km' });
  await cost(page, house, '2025-03-10', 'repair', 100_000);
  await cost(page, boiler, '2026-02-01', 'maintenance', 25_000);
  await cost(page, car, '2026-06-01', 'fuel', 8_000);
  await cost(page, car, '2026-07-01', 'repair', 42_000);

  await page.goto('/');
  // `exact` matters on a phone viewport: without it, "Statistics" also matches the account
  // avatar's "Signed in as e2e-20statistics-..." label, since this spec's own username embeds
  // the same word.
  await page.getByRole('button', { name: 'Statistics', exact: true }).click();
  await page.waitForURL('**/stats');

  const total = page.getByTestId('stats-total');
  await expect(total).toContainText('1,750.00');

  // The boiler is inside the house: hidden until the house is expanded, and counted in it.
  const byObject = page.getByTestId('stats-by-object');
  await expect(byObject).toContainText('Stats House');
  await expect(byObject).toContainText('1,250.00');
  await expect(byObject.getByText('Stats Boiler')).toHaveCount(0);
  await byObject.getByRole('button', { name: 'Show what is inside Stats House' }).click();
  await expect(byObject.getByText('Stats Boiler')).toBeVisible();

  // One year: twelve months, and only that year's money.
  await page.getByLabel('Year').selectOption('2026');
  // "750.00" is also inside the all-years "1,750.00", so wait for that to go first.
  await expect(total).not.toContainText('1,750.00');
  await expect(total).toContainText('750.00');

  // The year is kept in the address, so a reload -- or coming back from an object -- keeps it.
  await expect(page).toHaveURL(/[?&]year=2026(&|$)/);
  await page.reload();
  await expect(page.getByLabel('Year')).toHaveValue('2026');
  await expect(page.getByTestId('stats-total')).not.toContainText('1,750.00');
  await expect(page.getByTestId('stats-total')).toContainText('750.00');

  await expect(page.getByTestId('stats-over-time').locator('.bar-row')).toHaveCount(12);

  // Purchase prices: off by default, on survives a reload.
  await page.getByLabel('Year').selectOption({ label: 'All years' });
  await expect(page.getByLabel('Include purchase prices')).not.toBeChecked();
  await page.getByLabel('Include purchase prices').check();
  await expect(total).toContainText('4,750.00');
  await expect(page.getByTestId('stats-by-category')).toContainText('Purchase price');
  await page.reload();
  await expect(page.getByLabel('Include purchase prices')).toBeChecked();
  await expect(page.getByTestId('stats-total')).toContainText('4,750.00');

  // Tapping an object opens it.
  await page.getByTestId('stats-by-object').getByRole('button', { name: 'Stats Car' }).click();
  await page.waitForURL(`**/objects/${car}`);
});

test('a new user sees the empty state', async ({ page }) => {
  await signInFresh(page, '20-statistics-empty');
  await page.goto('/stats');
  await expect(page.getByText('No costs recorded for this period.')).toBeVisible();
});
