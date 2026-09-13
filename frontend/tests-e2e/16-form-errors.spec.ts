import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

/// Adding files saves the entry first, and an entry needs a title. The form used to answer the
/// click with a bare "Title" in red and nothing else -- a label, not a message.
test('adding files before a title says why, and puts the cursor in the title', async ({ page }) => {
  await signIn(page);
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Form errors car');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Form errors car' })).toBeVisible();

  await page.getByRole('button', { name: /Log activity/ }).first().click();
  await expect(page).toHaveURL(/\/activities\/new$/);
  await page.getByRole('button', { name: /Add photos or files/ }).click();

  const alert = page.getByRole('alert');
  await expect(alert).toHaveText('Give the entry a title first — photos and files are saved with it.');
  await expect(page.getByLabel('Title')).toBeFocused();
  // Nothing was saved: the picker only replaces the button once a draft exists.
  await expect(page.getByRole('button', { name: /Take photo/ })).toHaveCount(0);

  // With a title, the same button goes on to the picker.
  await page.getByLabel('Title').fill('Brake pads');
  await page.getByRole('button', { name: /Add photos or files/ }).click();
  await expect(page.getByRole('button', { name: /Take photo/ })).toBeVisible();
  await expect(alert).toHaveCount(0);
});
