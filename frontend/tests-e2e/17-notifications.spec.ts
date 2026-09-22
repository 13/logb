import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('a person sets their own digest webhook, and the hub row says so', async ({ page }) => {
  await signInFresh(page, '17-notifications');
  await page.goto('/settings');
  await page.getByRole('button', { name: /Notifications/ }).click();
  await expect(page).toHaveURL(/\/settings\/notifications$/);

  // Push is offered, refused, or explained -- which one depends on the browser, not on LogB.
  await expect(page.getByRole('heading', { name: 'On this device' })).toBeVisible();

  // Nowhere to send yet: the test button says so rather than claiming success.
  await page.getByRole('button', { name: 'Send a test notification' }).click();
  await expect(page.getByRole('status')).toContainText('nowhere to send one yet');

  await page.getByLabel('URL').fill('not a url');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('http or https');

  await page.getByLabel('URL').fill('https://ntfy.example/logb-e2e');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('status')).toHaveText('Saved');

  await page.reload();
  await expect(page.getByLabel('URL')).toHaveValue('https://ntfy.example/logb-e2e');
  await page.goto('/settings');
  await expect(page.getByRole('button', { name: /Notifications/ })).toContainText('Webhook');
});
