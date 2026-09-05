import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('first run leads to setup, then the dashboard', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('memto');
  await expect(page.getByRole('heading', { name: /Welcome to memto/ })).toBeVisible();
  await signIn(page);
  await expect(page.getByRole('heading', { name: /My objects/ })).toBeVisible();
});

test('signing out returns to the login screen', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Sign out/ }).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
});

test('the interface switches to German', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByLabel('Language').selectOption('de');
  await expect(page.getByRole('heading', { name: 'Einstellungen' })).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  await page.getByLabel('Sprache').selectOption('en');
});
