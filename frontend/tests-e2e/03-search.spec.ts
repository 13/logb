import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

// Runs after 02-lifecycle, which leaves a "Golf" object carrying a "Winter tyres" activity.
test('search finds an object and an activity from the dashboard', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Search' }).click();
  await expect(page).toHaveURL(/\/search$/);

  await page.getByLabel(/Search objects and activities/).fill('golf');
  await expect(page.getByRole('heading', { name: 'OBJECTS' })).toBeVisible();
  await expect(page.getByRole('button', { name: /Golf/ })).toBeVisible();

  await page.getByLabel(/Search objects and activities/).fill('tyres');
  await expect(page.getByRole('heading', { name: 'ACTIVITIES' })).toBeVisible();
  const hit = page.getByRole('button', { name: /Winter tyres/ });
  await expect(hit).toBeVisible();

  // The query is in the URL, so the result list survives a reload.
  await expect(page).toHaveURL(/\/search\?q=tyres$/);
  await page.reload();
  await expect(page.getByRole('button', { name: /Winter tyres/ })).toBeVisible();

  // A hit opens the activity it points at.
  await page.getByRole('button', { name: /Winter tyres/ }).click();
  await expect(page.getByLabel('Title')).toHaveValue('Winter tyres');
});

test('a query with no hits says so', async ({ page }) => {
  await signIn(page);
  await page.goto('/search');
  await page.getByLabel(/Search objects and activities/).fill('zzzznothing');
  await expect(page.getByText('Nothing found.')).toBeVisible();
});
