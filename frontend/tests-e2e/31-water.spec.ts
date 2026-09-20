import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('water meter readings produce monthly household consumption', async ({ page }) => {
  await signInFresh(page, '31-water');
  await page.goto('/objects/new');
  await page.getByRole('button', { name: 'Water meter', exact: true }).click();
  await expect(page.getByLabel('Resource')).toHaveValue('water');
  await expect(page.getByLabel('Usage unit')).toHaveValue('m3');
  await page.getByLabel(/Monthly target/).fill('15');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await page.getByRole('button', { name: /Record water/ }).click();
  await page.getByLabel(/Meter reading/).fill('100');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await page.getByRole('button', { name: /Record water/ }).click();
  await page.getByLabel(/Meter reading/).fill('102.5');
  await page.getByRole('button', { name: 'Save', exact: true }).click();

  await page.goto('/stats');
  const water = page.getByTestId('stats-water');
  await expect(water).toContainText('2.5 m³');
});
