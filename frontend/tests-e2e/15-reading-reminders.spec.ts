import { test, expect, type Page } from '@playwright/test';
import { signIn } from './helpers';

async function newCar(page: Page, name: string) {
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill(name);
  await page.getByLabel('Type').selectOption('car');
  await page.getByLabel('Counter').selectOption('km');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name })).toBeVisible();
}

test('a reading reminder is satisfied by logging a reading, not by marking it done', async ({ page }) => {
  await signIn(page);
  await newCar(page, 'Reading Golf');

  await page.getByRole('button', { name: /^Reminders/ }).click();
  await page.getByRole('button', { name: 'Remind me to log the reading' }).click();
  await expect(page.getByLabel('Every')).toHaveValue('1');
  // Started long ago, so with no reading on record it is due straight away.
  await page.getByLabel('Starting').fill('2020-01-01');
  await page.getByRole('button', { name: 'Save' }).click();

  const card = page.locator('.card').filter({ hasText: 'Log the counter reading' });
  await expect(card.locator('.chip.due')).toBeVisible();
  await expect(card.getByRole('button', { name: 'Mark done' })).toHaveCount(0);
  await expect(card.getByText('No reading yet')).toBeVisible();

  await card.getByRole('button', { name: 'Record reading' }).click();
  await expect(page).toHaveURL(/\/reading$/);
  await page.getByLabel(/Reading \(km\)/).fill('12345');
  await page.getByRole('button', { name: 'Save' }).click();

  // Back on the object: the reading is in the timeline, folded to one line.
  await expect(page.locator('.entry.reading')).toContainText('12,345 km');

  await page.getByRole('button', { name: /^Reminders/ }).click();
  await expect(card.getByText(/Last reading 12,345 km/)).toBeVisible();
  await expect(card.locator('.chip.due')).toHaveCount(0);
});

test('a reading lower than the last one asks for a second look before it is saved', async ({ page }) => {
  await signIn(page);
  await newCar(page, 'Reading Polo');
  const id = page.url().match(/\/objects\/(\d+)/)![1];

  await page.goto(`/objects/${id}/reading`);
  await page.getByLabel(/Reading \(km\)/).fill('50000');
  await page.getByRole('button', { name: 'Save' }).click();
  // The object page writes its open tab into the URL, so a query string may follow.
  await expect(page).toHaveURL(new RegExp(`/objects/${id}(\\?|$)`));

  await page.goto(`/objects/${id}/reading`);
  await expect(page.getByLabel(/Reading \(km\)/)).toHaveValue('50000');
  await page.getByLabel(/Reading \(km\)/).fill('5000');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('alert')).toContainText('Lower than the last reading');
  await expect(page).toHaveURL(/\/reading$/);

  // Saving the same value again is the confirmation.
  await page.getByRole('button', { name: 'Save' }).click();
  // The object page writes its open tab into the URL, so a query string may follow.
  await expect(page).toHaveURL(new RegExp(`/objects/${id}(\\?|$)`));
});

test('a new object with a counter can ask for a monthly reading reminder on the way in', async ({ page }) => {
  await signIn(page);
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Reading Bike');
  await page.getByLabel('Type').selectOption('e_bike');
  const optIn = page.getByLabel('Remind me to log the reading every month');
  await expect(optIn).toHaveCount(0);
  await page.getByLabel('Counter').selectOption('km');
  await optIn.check();
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Reading Bike' })).toBeVisible();

  await page.getByRole('button', { name: /^Reminders/ }).click();
  const card = page.locator('.card').filter({ hasText: 'Log the counter reading' });
  await expect(card.getByText('every month')).toBeVisible();
  // It starts a month out, so it is not due yet.
  await expect(card.locator('.chip.due')).toHaveCount(0);
});
