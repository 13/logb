import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('a new car starts with the reminders ticked for it, measured from its current reading', async ({ page }) => {
  await signInFresh(page, '18-templates');
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Template Golf');
  await page.getByLabel('Type').selectOption('car');
  await page.getByLabel('Counter').selectOption('km');

  // Offered, never ticked for you.
  const oil = page.getByLabel(/Oil change/);
  await expect(oil).not.toBeChecked();
  await expect(page.getByLabel(/Current reading/)).toHaveCount(0);

  await oil.check();
  await page.getByLabel(/Inspection/).check();
  // A reminder by distance needs to know where the counter is now.
  await page.getByLabel(/Current reading \(km\)/).fill('80000');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Template Golf' })).toBeVisible();

  // The reading was saved as the first entry...
  await expect(page.locator('.entry.reading')).toContainText('80,000 km');

  // ...and the oil change is due 15,000 km on from it.
  await page.getByRole('button', { name: /^Reminders/ }).click();
  const oilCard = page.locator('.card').filter({ hasText: 'Oil change' });
  await expect(oilCard).toContainText('at 95,000 km');
  await expect(page.locator('.card').filter({ hasText: 'Inspection' })).toBeVisible();
  await expect(page.locator('.card').filter({ hasText: 'Tyre change' })).toHaveCount(0);
});
