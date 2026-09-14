import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('an own type is offered, drawn and counted everywhere a built-in one is', async ({ page }) => {
  await signInFresh(page, '24-own-types');

  // Settings → Types → add one.
  await page.goto('/settings');
  await page.getByRole('button', { name: /^Types/ }).click();
  await page.waitForURL('**/settings/types');
  await page.getByRole('button', { name: 'Add type' }).click();
  await page.getByLabel('Name', { exact: true }).fill('E-Scooter');
  // The radio itself is visually hidden inside its icon cell; a person taps the cell.
  const eBike = page.getByRole('radio', { name: 'E-bike' });
  await page.locator('.icon-choice').filter({ has: eBike }).click();
  await expect(eBike).toBeChecked();
  // The add form starts with a sensible default set; this type wants exactly Repair and Fuel.
  for (const c of ['Maintenance', 'Inspection', 'Purchase']) await page.getByLabel(c, { exact: true }).uncheck();
  await page.getByLabel('Fuel / charge', { exact: true }).check();
  await expect(page.getByLabel('Repair', { exact: true })).toBeChecked();
  await page.getByLabel('Default counter unit').selectOption('km');
  await page.getByRole('button', { name: 'Save type' }).click();
  const typeRow = page.locator('.type-row', { hasText: 'E-Scooter' });
  await expect(typeRow).toBeVisible();

  // New object: the type sits under "Your types" and brings its counter unit along.
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Scooter One');
  await expect(page.locator('#c optgroup[label="Your types"] option', { hasText: 'E-Scooter' })).toHaveCount(1);
  // exact: the "Your types" optgroup is labelled too.
  await page.getByLabel('Type', { exact: true }).selectOption({ label: 'E-Scooter' });
  await expect(page.getByLabel('Counter', { exact: true })).toHaveValue('km');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);
  const objectId = Number(new URL(page.url()).pathname.split('/').pop());

  // The card names the type, never its key.
  await page.goto('/');
  // Anchored: a card's accessible name also carries its nested labels.
  const card = page.getByRole('button', { name: /^Scooter One/ });
  await expect(card).toContainText('E-Scooter');
  await expect(page.getByText(/custom:/)).toHaveCount(0);

  // A new entry offers exactly the type's categories.
  await page.goto(`/objects/${objectId}/activities/new`);
  await expect(page.getByLabel('Category').locator('option')).toHaveText(['Repair', 'Fuel / charge', 'Other']);
  await page.getByLabel('Category').selectOption({ label: 'Repair' });
  await page.getByLabel('Title').fill('New brake pads');
  await page.getByLabel('Cost').fill('120');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await page.waitForURL(`**/objects/${objectId}`);
  await expect(page.getByText('New brake pads')).toBeVisible();

  // Search hits name the type too.
  await page.goto('/search?q=Scooter%20One');
  await expect(page.getByRole('main')).toContainText('E-Scooter');
  await expect(page.getByText(/custom:/)).toHaveCount(0);

  // Statistics: the "By type" row is the type's name.
  await page.goto('/stats');
  const byType = page.getByTestId('stats-by-type');
  await expect(byType.locator('.bar-row', { hasText: 'E-Scooter' })).toContainText('€120.00');
  await expect(byType).not.toContainText('custom:');

  // Deleting a type still in use is refused, with the count, and the type stays.
  await page.goto('/settings/types');
  page.once('dialog', (d) => d.accept());
  await typeRow.getByRole('button', { name: 'Delete' }).click();
  await expect(typeRow.getByRole('alert')).toHaveText(/Used by 1 object/);
  await page.reload();
  await expect(page.locator('.type-row', { hasText: 'E-Scooter' })).toBeVisible();
});
