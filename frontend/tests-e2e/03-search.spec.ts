import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

/**
 * Seeds its own object and activity through the API rather than searching for what another
 * spec happened to leave behind. The terms are deliberately unlike anything the other specs
 * create, so a hit can only be this test's own row however many objects the shared database
 * has accumulated by the time this runs.
 */
async function seed(page: import('@playwright/test').Page) {
  const object = await page.request.post('/api/objects', {
    data: { name: 'Saab', type: 'car', counter_unit: 'km' },
  });
  expect(object.ok()).toBe(true);
  const { id } = (await object.json()) as { id: number };
  const activity = await page.request.post(`/api/objects/${id}/activities`, {
    data: { date: '2026-01-01', category: 'maintenance', title: 'Cambelt', notes: '' },
  });
  expect(activity.ok()).toBe(true);
}

test('search finds an object and an activity from the dashboard', async ({ page }, testInfo) => {
  // `seed` names this test's fixtures "Saab" / "Cambelt" so they cannot collide with what other
  // *specs* create -- but it does not make them unique against a second run of *this same
  // test*. The desktop project shares the mobile project's database (see playwright.config.ts),
  // so the mobile pass has already seeded one "Saab", and seeding a second here turns every
  // `getByRole('button', { name: /Saab/ })` into a strict-mode violation -- a run-order
  // limitation of this test, not anything about desktop rendering.
  test.skip(testInfo.project.name === 'desktop', 'seeds fixed-named fixtures that collide with the mobile project\'s own run of this test');
  await signIn(page);
  await seed(page);
  await page.getByRole('button', { name: 'Search' }).click();
  await expect(page).toHaveURL(/\/search$/);

  await page.getByLabel(/Search objects and activities/).fill('saab');
  await expect(page.getByRole('heading', { name: 'OBJECTS' })).toBeVisible();
  await expect(page.getByRole('button', { name: /Saab/ })).toBeVisible();

  await page.getByLabel(/Search objects and activities/).fill('cambelt');
  await expect(page.getByRole('heading', { name: 'ACTIVITIES' })).toBeVisible();
  const hit = page.getByRole('button', { name: /Cambelt/ });
  await expect(hit).toBeVisible();

  // The query is in the URL, so the result list survives a reload.
  await expect(page).toHaveURL(/\/search\?q=cambelt$/);
  await page.reload();
  await expect(page.getByRole('button', { name: /Cambelt/ })).toBeVisible();

  // A hit opens the activity it points at.
  await page.getByRole('button', { name: /Cambelt/ }).click();
  await expect(page.getByLabel('Title')).toHaveValue('Cambelt');
});

test('a query with no hits says so', async ({ page }) => {
  await signIn(page);
  await page.goto('/search');
  await page.getByLabel(/Search objects and activities/).fill('zzzznothing');
  // The line names the query it failed to match, so this asserts the whole fact, not just
  // that some empty state appeared.
  await expect(page.getByText(/No matches for “zzzznothing”/)).toBeVisible();
});
