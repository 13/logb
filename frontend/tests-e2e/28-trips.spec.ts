import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The app's own example date, pinned so nothing here depends on when it actually runs (mirrors
 *  `FIXED_NOW` in `26-date-format.spec.ts`). Nothing in this spec asserts on a date directly, but
 *  a pinned clock keeps the object's default "today" entries stable regardless of the real date. */
const FIXED_NOW = new Date('2026-09-15T12:00:00');

/** Mirrors the same helper in `26-date-format.spec.ts`/`27-last-done.spec.ts`: seed data through
 *  the API so the test itself only drives the trip form and the timeline it produces. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function entry(page: Page, id: number, data: Record<string, unknown>) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', category: 'maintenance', ...data } });
  expect(res.ok()).toBe(true);
}

test('logging a trip, editing one, filtering by it, and the end-below-start error', async ({ page }) => {
  await page.clock.setFixedTime(FIXED_NOW);
  await signInFresh(page, '28-trips');

  const bike = await object(page, { name: 'Trip E-Bike', type: 'e_bike', counter_unit: 'km' });
  await entry(page, bike, { category: 'reading', title: 'Odometer', counter_value: 400, date: '2026-01-01' });

  await page.goto(`/objects/${bike}`);
  await page.getByRole('button', { name: /Log trip/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}/activities/new\\?category=trip`));

  // Start defaults to the object's current counter (the last reading, 400).
  await expect(page.getByLabel(/^Start/)).toHaveValue('400');
  // Typing Distance fills End = start + distance.
  await page.getByLabel(/^Distance/).fill('200');
  await expect(page.getByLabel(/^End/)).toHaveValue('600');

  await page.getByLabel(/^From/).fill('Home');
  await page.getByLabel(/^To/).fill('Office');
  await page.getByLabel(/^Duration/).fill('1:15');
  await page.getByLabel(/Battery used/).fill('32');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}$`));

  // The counter-value pattern allows for a thousands separator the real numbers here don't need,
  // so this stays correct however large a real trip's counters get.
  const firstTrip = page.locator('.card.entry', { hasText: 'Home → Office' });
  await expect(firstTrip).toBeVisible();
  await expect(firstTrip).toContainText(/400 km → 600 km/);
  await expect(firstTrip).toContainText('200 km');
  await expect(firstTrip).toContainText('Home → Office');
  await expect(firstTrip).toContainText('1:15 h');
  await expect(firstTrip).toContainText('32 %');
  // The trip moved the object's own current counter, the same as a plain reading would.
  await expect(page.locator('.stat', { hasText: 'Current' })).toContainText('600 km');

  // A second trip: start now prefills from the first trip's end, and From already offers "Home".
  await page.getByRole('button', { name: /Log trip/ }).click();
  await expect(page.getByLabel(/^Start/)).toHaveValue('600');
  await expect(page.locator('#trip-from option[value="Home"]')).toHaveCount(1);
  await page.getByLabel(/^Distance/).fill('50');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}$`));

  // Editing the first trip: changing End recomputes Distance the same way the new-trip form does.
  await firstTrip.click();
  await expect(page).toHaveURL(/\/activities\/\d+$/);
  await expect(page.getByLabel(/^Start/)).toHaveValue('400');
  await page.getByLabel(/^End/).fill('650');
  await expect(page.getByLabel(/^Distance/)).toHaveValue('250');
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}$`));

  // The "Trip" category chip narrows the timeline to exactly the two trips just logged.
  await page.getByRole('button', { name: 'Trip', exact: true }).click();
  await expect(page.locator('.entry-row')).toHaveCount(2);

  // An end below start is refused, and the form is left open with the error on screen.
  await page.getByRole('button', { name: /Log trip/ }).click();
  await page.getByLabel(/^Start/).fill('500');
  await page.getByLabel(/^End/).fill('400');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('End must not be below start')).toBeVisible();
  await expect(page).toHaveURL(/\/activities\/new/);

  // Battery used has no native `min`/`max` (see ActivityForm.svelte) precisely so a value past
  // 100 reaches the app's own message instead of being silently refused by the browser.
  await page.getByLabel(/^End/).fill('600');
  await page.getByLabel(/Battery used/).fill('101');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Battery used must be 0–100 %')).toBeVisible();
  await expect(page).toHaveURL(/\/activities\/new/);
});
