import { test, expect, type Page } from '@playwright/test';
import { signInFresh, underServiceWorker } from './helpers';

/** Every language, English included, is its own chunk: none is in the entry. */

async function chooseGerman(page: Page) {
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('link', { name: /Appearance/ }).click();
  await page.getByLabel('Language').selectOption('de');
  await expect(page.getByRole('heading', { name: 'Darstellung' })).toBeVisible();
}

test('an app started offline in German opens in German', async ({ page, context }) => {
  await signInFresh(page, '40-offline-de');
  await chooseGerman(page);
  await page.goto('/');
  await underServiceWorker(page);
  await expect(page.getByRole('heading', { name: 'Meine Objekte' })).toBeVisible();

  // The German chunk comes from the precache, like the shell.
  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByRole('status').filter({ hasText: /Offline/ })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Meine Objekte' })).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  await context.setOffline(false);
});

test.describe('a language chunk that cannot be fetched', () => {
  // No service worker: its precache would serve the chunk, and this is about one it cannot.
  test.use({ serviceWorkers: 'block' });

  test('the app starts in English instead, and in German on the next start', async ({ page, context }) => {
    await signInFresh(page, '40-chunk-de');
    await chooseGerman(page);

    let blocked = true;
    await page.route(/\/assets\/de-[^/]*\.js$/, (route) => (blocked ? route.abort() : route.continue()));
    await page.goto('/');
    // Not bare keys and not a blank page: the fallback language.
    await expect(page.getByRole('heading', { name: 'My objects' })).toBeVisible();

    // Chromium remembers a failed dynamic import for the page's lifetime, so the language comes
    // with the next start rather than when the connection returns.
    blocked = false;
    await page.reload();
    await expect(page.getByRole('heading', { name: 'Meine Objekte' })).toBeVisible();
  });
});
