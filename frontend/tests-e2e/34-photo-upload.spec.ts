import { test, expect, type Page } from '@playwright/test';
import { pngPayload, signInFresh } from './helpers';

async function newActivity(page: Page, object: string) {
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill(object);
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: object })).toBeVisible();
  await page.getByRole('button', { name: /Log activity/ }).click();
}

test('Save waits for a photo that is still uploading', async ({ page }) => {
  await signInFresh(page, '34-upload-wait');
  await newActivity(page, 'Upload wait bike');
  await page.getByLabel('Title').fill('Chain');
  await page.getByRole('button', { name: /Add photos or files/ }).click();

  let release!: () => void;
  const held = new Promise<void>((r) => { release = r; });
  await page.route('**/api/objects/*/attachments', async (route) => { await held; await route.continue(); });
  await page.setInputFiles('input[type=file]', pngPayload());

  const save = page.getByRole('button', { name: 'Save' });
  await expect(save).toBeDisabled();
  await expect(save).toHaveAccessibleDescription('Save is available once the upload has finished.');

  release();
  await expect(page.locator('.thumb-strip img')).toHaveCount(1);
  await expect(save).toBeEnabled();
  await save.click();
  await expect(page.getByText('Chain').first()).toBeVisible();
});
