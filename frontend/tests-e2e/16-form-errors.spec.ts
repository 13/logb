import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

/// Snapping the receipt comes before naming the entry. Adding files saves the draft, which needs
/// a title -- so the category stands in as one, visibly, instead of the click being refused.
test('adding files before a title starts the draft under the category name', async ({ page }) => {
  await signInFresh(page, 'form-errors');
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Form errors car');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Form errors car' })).toBeVisible();

  await page.getByRole('button', { name: /Log activity/ }).first().click();
  await expect(page).toHaveURL(/\/activities\/new$/);
  await page.getByLabel('Category').selectOption('repair');
  await page.getByRole('button', { name: /Add photos or files/ }).click();

  await expect(page.getByLabel('Title')).toHaveValue('Repair');
  await expect(page.getByRole('button', { name: /Take photo/ })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
});

/// A validation message used to be the field's label on its own -- "Cost" in red.
test('a field that does not validate is named in a sentence', async ({ page }) => {
  await signInFresh(page, 'form-errors');
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Form errors bike');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Form errors bike' })).toBeVisible();

  await page.getByRole('button', { name: /Log activity/ }).first().click();
  await page.getByLabel('Title').fill('Brake pads');
  await page.getByLabel('Cost').fill('twelve');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('alert')).toHaveText('Check “Cost”: it is missing or not valid.');
});
