import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** A date `days` before today in the browser's calendar, as the API takes it. */
function daysAgo(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() - days);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function entry(page: Page, id: number, data: Record<string, unknown>) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', title: 'Entry', ...data } });
  expect(res.ok()).toBe(true);
}

test('tabs, search at any depth, sorting that survives a reload, and a card that says usage and last activity', async ({ page }) => {
  await signInFresh(page, '22-objects-list');
  const house = await object(page, { name: 'List House', type: 'home' });
  await object(page, { name: 'List Boiler', type: 'appliance', parent_id: house });
  const bike = await object(page, { name: 'List E-Bike', type: 'e_bike', counter_unit: 'km' });
  // 2,700 km over the 90 days between the readings: 30 km a day, about 913 a month.
  await entry(page, bike, { date: daysAgo(100), category: 'reading', counter_value: 10_000 });
  await entry(page, bike, { date: daysAgo(10), category: 'reading', counter_value: 12_700 });
  await entry(page, bike, { date: daysAgo(3), category: 'maintenance' });
  const old = await object(page, { name: 'List Old Bike', type: 'bike' });
  const res = await page.request.patch(`/api/objects/${old}`, { data: { name: 'List Old Bike', type: 'bike', description: '', archived: true } });
  expect(res.ok()).toBe(true);

  await page.goto('/');
  const tabActive = page.getByRole('button', { name: /^Active/ });
  const tabArchived = page.getByRole('button', { name: /^Archived/ });
  await expect(tabActive).toContainText('2');
  await expect(tabArchived).toContainText('1');

  // The card: counter, usage per month, last activity.
  const bikeCard = page.getByRole('button', { name: /List E-Bike/ });
  await expect(bikeCard).toContainText('12,700 km');
  await expect(bikeCard).toContainText('913 km a month');
  await expect(bikeCard).toContainText(/\d+ days ago/);

  // Top-level only until searching; a search finds the boiler inside the house and says so.
  await expect(page.getByRole('button', { name: /List Boiler/ })).toHaveCount(0);
  await page.getByLabel('Search objects').fill('boil');
  const boiler = page.getByRole('button', { name: /List Boiler/ });
  await expect(boiler).toBeVisible();
  await expect(boiler).toContainText('in List House');
  await expect(page.getByRole('button', { name: /^List House/ })).toHaveCount(0);
  await page.getByLabel('Search objects').fill('zzz-nothing');
  await expect(page.getByText('No objects match “zzz-nothing”.')).toBeVisible();
  await page.getByLabel('Search objects').fill('');

  // Last activity puts the used bike first; the house has no entries and goes last.
  await page.getByLabel('Sort').selectOption('last-activity');
  const cards = page.locator('.list .list-card');
  await expect(cards.first()).toContainText('List E-Bike');
  await expect(page).toHaveURL(/[?&]sort=last-activity(&|$)/);
  await page.reload();
  await expect(page.getByLabel('Sort')).toHaveValue('last-activity');
  await expect(page.locator('.list .list-card').first()).toContainText('List E-Bike');

  // Archived tab.
  await page.getByRole('button', { name: /^Archived/ }).click();
  await expect(page.getByRole('button', { name: /List Old Bike/ })).toBeVisible();
  await expect(page).toHaveURL(/[?&]tab=archived(&|$)/);
});
