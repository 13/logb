import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

/** Erase every local preference while keeping the session cookie: a fresh device, same account. */
async function forgetLocalPreferences(page: import('@playwright/test').Page) {
  await page.evaluate(() => {
    for (const key of ['logb.settings', 'logb.appearance-accounts', 'logb.appearance-devices', 'logb.appearance-overrides']) {
      localStorage.removeItem(key);
    }
  });
  await page.reload();
}

test('appearance applied to the account reaches a device that has never seen it', async ({ page }) => {
  await signInFresh(page, '33-account');
  await page.goto('/settings/appearance');
  await page.getByLabel('First day of the week').selectOption('sunday');
  await page.getByLabel('Date format').selectOption('iso');
  await page.getByRole('button', { name: 'Apply appearance', exact: true }).click();
  await expect(page.getByText('Saved', { exact: true })).toBeVisible();

  await forgetLocalPreferences(page);
  await expect(page.getByLabel('First day of the week')).toHaveValue('sunday');
  await expect(page.getByLabel('Date format')).toHaveValue('iso');
});

test('a device-only choice stays on the device and leaves the account alone', async ({ page }) => {
  await signInFresh(page, '33-device');
  await page.goto('/settings/appearance');
  await page.getByLabel('Date format').selectOption('iso');
  await page.getByRole('button', { name: 'Apply appearance', exact: true }).click();
  await expect(page.getByText('Saved', { exact: true })).toBeVisible();

  await page.getByLabel('Use these preferences only on this device').check();
  await page.getByLabel('Date format').selectOption('dmy-dot');
  await page.getByRole('button', { name: 'Apply appearance', exact: true }).click();
  await expect(page.getByText('Saved', { exact: true })).toBeVisible();

  // The account keeps what was applied before the override.
  const stored = await (await page.request.get('/api/me/appearance')).json();
  expect(stored).toMatchObject({ dateFormat: 'iso' });

  // The device keeps its own choice across a reload.
  await page.reload();
  await expect(page.getByLabel('Date format')).toHaveValue('dmy-dot');

  // A device that never made that choice follows the account again.
  await forgetLocalPreferences(page);
  await expect(page.getByLabel('Date format')).toHaveValue('iso');
});

test('a personal delivery hour and timezone are saved and read back', async ({ page }) => {
  await signInFresh(page, '33-hour');
  await page.goto('/settings/notifications');
  const hour = page.getByLabel('Daily delivery hour');
  await expect(hour).toHaveValue('8');
  await hour.fill('17');
  await page.getByLabel('Timezone for that hour').selectOption('America/New_York');
  await page.getByRole('button', { name: 'Apply delivery time', exact: true }).click();
  await expect(page.getByRole('status')).toHaveText('Saved');

  await page.reload();
  await expect(page.getByLabel('Daily delivery hour')).toHaveValue('17');
  await expect(page.getByLabel('Timezone for that hour')).toHaveValue('America/New_York');
  const settings = await (await page.request.get('/api/me/notifications')).json();
  expect(settings.hour).toBe(17);
  expect(settings.timezone).toBe('America/New_York');
});
