import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('first run leads to setup, then the dashboard', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('logby');
  await expect(page.getByRole('heading', { name: /Welcome to logby/ })).toBeVisible();
  await signIn(page);
  await expect(page.getByRole('heading', { name: /My objects/ })).toBeVisible();
});

test('signing out returns to the login screen', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  // Exact: the account section also offers "Sign out everywhere".
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
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

test('signing out everywhere returns to the login screen', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  page.once('dialog', (d) => d.accept());
  await page.getByRole('button', { name: 'Sign out everywhere' }).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
});
