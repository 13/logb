import { test, expect } from '@playwright/test';
import { pngPayload, signIn } from './helpers';

test('an object records activities, photos and reminders', async ({ page }) => {
  await signIn(page);

  // create the object
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Golf');
  await page.getByLabel('Type').selectOption('car');
  await page.getByLabel('Counter').selectOption('km');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Golf' })).toBeVisible();

  // log an activity with a photo and a cost
  await page.getByRole('button', { name: /Log activity/ }).click();
  await page.getByLabel('Title').fill('Winter tyres');
  await page.getByLabel(/Counter reading/).fill('104500');
  await page.getByLabel('Cost').fill('249.90');
  await page.getByRole('button', { name: /Add photos or files/ }).click();
  await page.setInputFiles('input[type=file]', pngPayload());
  await expect(page.locator('.thumb-strip img')).toHaveCount(1);

  // The wrapper must fit its image. It used to carry the global `.thumb` rule, which is
  // written for grid images (`width: 100%; aspect-ratio: 1`), so it inflated to a 388-wide
  // square around a 64px thumbnail and pushed Save below the fold. Comparing the two widths
  // states the requirement; asserting a literal 64 would pin an unrelated decoration value.
  const item = page.locator('.thumb-strip > *').first();
  const img = page.locator('.thumb-strip img').first();
  const itemBox = await item.boundingBox();
  const imgBox = await img.boundingBox();
  expect(itemBox!.width).toBeCloseTo(imgBox!.width, 0);
  await page.getByRole('button', { name: 'Save' }).click();

  // the timeline and the stats reflect it
  await expect(page.getByText('Winter tyres').first()).toBeVisible();
  await expect(page.getByText('€249.90').first()).toBeVisible();
  await expect(page.getByText('104,500 km').first()).toBeVisible();

  // documents tab shows the photo
  await page.getByRole('button', { name: 'Documents' }).click();
  await expect(page.locator('.grid img.thumb')).toHaveCount(1);

  // a counter reminder is already due
  await page.getByRole('button', { name: /^Reminders/ }).click();
  await page.getByRole('button', { name: /New reminder/ }).click();
  await page.getByLabel('Title').fill('Oil change');
  await page.getByLabel(/Due at counter/).fill('100000');
  await page.getByLabel(/Repeat every \(counter\)/).fill('15000');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Oil change').first()).toBeVisible();
  await expect(page.locator('.chip.due').first()).toBeVisible();

  // completing it links the activity and schedules the next one
  await page.getByRole('button', { name: /Mark done/ }).click();
  await page.getByLabel(/Link to activity/).selectOption({ index: 1 });
  await page.getByRole('button', { name: /^Done$/ }).click();
  await expect(page.getByText(/Next reminder created/)).toBeVisible();
  await expect(page.getByText('119,500 km').first()).toBeVisible();

  // the dashboard is clean again, and shows the object's type icon
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.getByText('Golf').first()).toBeVisible();
  await expect(page.getByText(/reminders? due/)).toHaveCount(0);
  await expect(page.locator('.card-row svg').first()).toBeVisible();
});
