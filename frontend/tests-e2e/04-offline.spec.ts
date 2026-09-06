import { expect, test } from '@playwright/test';
import { signIn } from './helpers';

test('an activity logged offline appears once after reconnecting', async ({ page, context }) => {
  await signIn(page);

  // create the object
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Trailer');
  await page.getByLabel('Category').fill('trailer');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Trailer' })).toBeVisible();

  await page.getByRole('button', { name: /Log/ }).first().click();

  await context.setOffline(true);
  await page.getByLabel('Title').fill('Fuel');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Fuel', { exact: true })).toBeVisible();

  await context.setOffline(false);
  await page.reload();
  await expect(page.getByText('Fuel', { exact: true })).toHaveCount(1);
});
