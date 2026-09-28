import { test, expect, type Page } from '@playwright/test';
import { jpegWithExifPayload, pngPayload, signInFresh } from './helpers';

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

test('a large photo goes up smaller and keeps its capture date', async ({ page }) => {
  await signInFresh(page, '34-shrink');
  await newActivity(page, 'Shrink camera');
  await page.getByLabel('Title').fill('Big photo');
  await page.getByRole('button', { name: /Add photos or files/ }).click();

  // Over the 1.5 MB threshold, but only because of what trails the image: a decoder stops at the
  // end-of-image marker, so the browser re-encodes a 4x4 picture and has to carry the Exif over
  // for the server to find the capture date in it.
  const photo = jpegWithExifPayload('IMG_big.jpg');
  photo.buffer = Buffer.concat([photo.buffer, Buffer.alloc(2 * 1024 * 1024)]);
  const answered = page.waitForResponse((r) => r.request().method() === 'POST' && /\/api\/objects\/\d+\/attachments$/.test(r.url()));
  await page.setInputFiles('input[type=file]', photo);

  const stored = await (await answered).json();
  expect(stored.size).toBeLessThan(100_000);
  expect(stored.taken_at).toBe('2025-12-25T10:00:00');
  await expect(page.getByRole('button', { name: /Use photo date/ })).toBeVisible();
});
