import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('a house shows its rooms, and a room shows its breadcrumb', async ({ page }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Hierarchy House');
  await page.getByLabel('Type').selectOption('home');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.goto('/');
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Hierarchy Garage');
  await page.getByLabel('Type').selectOption('other');
  await page.getByLabel('Inside').selectOption({ label: 'Hierarchy House' });
  await page.getByRole('button', { name: 'Save' }).click();

  // The garage's own page shows the breadcrumb back to the house.
  await expect(page.getByText('Hierarchy House')).toBeVisible();

  // The house's page lists the garage in its contents.
  await page.goto('/');
  await page.getByText('Hierarchy House').click();
  await page.getByRole('button', { name: 'Info' }).click();
  await expect(page.getByText('Hierarchy Garage')).toBeVisible();
});
