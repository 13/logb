import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('first run leads to setup, then the dashboard', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('LogB');
  await expect(page.getByRole('img', { name: 'LogB' })).toBeVisible();
  // The in-app logo is inlined at build time, so it no longer proves /icon.svg is served. The
  // favicon, the manifest and the export archive all still need that file.
  const icon = await page.request.get('/icon.svg');
  expect(icon.ok()).toBe(true);
  expect(await icon.text()).toContain('<!--mark-->');
  await expect(page.getByRole('heading', { name: /Welcome to LogB/ })).toBeVisible();
  await signIn(page);
  await expect(page.getByRole('heading', { name: /My objects/ })).toBeVisible();
});

test('signing out returns to the login screen', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Account/ }).click();
  // Exact: the account page also offers "Sign out everywhere". Inside `main`: on desktop the
  // sidebar carries its own "Sign out" too.
  await page.locator('main').getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
});

test('the interface switches to German', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Appearance/ }).click();
  await page.getByLabel('Language').selectOption('de');
  await expect(page.getByRole('heading', { name: 'Darstellung' })).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  await page.getByLabel('Sprache').selectOption('en');
});

test('signing out everywhere returns to the login screen', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Account/ }).click();
  page.once('dialog', (d) => d.accept());
  await page.getByRole('button', { name: 'Sign out everywhere' }).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
});
