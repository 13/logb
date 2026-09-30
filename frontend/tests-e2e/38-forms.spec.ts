import { expect, test, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The forms as round 4 of the UI overhaul left them. Seeds through the API so each test drives
 *  only the form it is about. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id as number;
}

/** Save is on screen: inside the viewport, a full-size target, and above a phone's tab bar. */
async function expectSaveOnScreen(page: Page): Promise<void> {
  const save = page.getByRole('button', { name: 'Save', exact: true });
  await expect(save).toBeVisible();
  const box = (await save.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  expect(box.height).toBeGreaterThanOrEqual(44);
  const nav = (await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).boundingBox())!;
  // A bottom tab bar (phone): Save sits above it. A sidebar (desktop) cannot cover it.
  if (nav.y > viewport.height / 2) expect(box.y + box.height).toBeLessThanOrEqual(nav.y + 0.5);
}

test('the reading form: one field pattern, and Save on screen from the start', async ({ page }) => {
  await signInFresh(page, '38-reading');
  const id = await object(page, { name: 'Forms reading car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reading`);

  const reading = page.getByLabel(/Reading \(km\)/);
  await expect(reading).toBeFocused();
  // The object's name is the page's subtitle now, not a stray paragraph in the form.
  await expect(page.getByText('Forms reading car', { exact: true })).toBeVisible();
  await expectSaveOnScreen(page);

  // The label belongs to its control: clicking it focuses the field.
  await page.getByLabel('Date', { exact: true }).focus();
  await page.getByText('Reading (km)', { exact: true }).click();
  await expect(reading).toBeFocused();
  await expect(reading).toHaveCSS('height', '56px');
});

test('the date field keeps its calendar button inside the box, at a full-size target', async ({ page }) => {
  await signInFresh(page, '38-date');
  const id = await object(page, { name: 'Date box car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reading`);

  const field = (await page.getByLabel('Date', { exact: true }).boundingBox())!;
  const button = page.getByRole('button', { name: 'Choose date', exact: true });
  const b = (await button.boundingBox())!;
  expect(b.x).toBeGreaterThanOrEqual(field.x);
  expect(b.x + b.width).toBeLessThanOrEqual(field.x + field.width);
  expect(b.y).toBeGreaterThanOrEqual(field.y);
  expect(b.y + b.height).toBeLessThanOrEqual(field.y + field.height);
  expect(Math.min(b.width, b.height)).toBeGreaterThanOrEqual(44);

  await button.click();
  await expect(page.getByTestId('calendar-weekday')).toHaveCount(7);
  const day = page.getByRole('dialog', { name: 'Choose date' }).getByRole('button', { pressed: true });
  await expect(day).toHaveCount(1);
  expect((await day.boundingBox())!.height).toBeGreaterThanOrEqual(44);
});
