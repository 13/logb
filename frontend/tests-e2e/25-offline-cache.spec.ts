import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The service worker must control the page before an offline reload can be served from it. */
async function underServiceWorker(page: Page) {
  await page.evaluate(async () => { await navigator.serviceWorker.ready; });
  // A first visit is not controlled even once the worker is active (no `clients.claim` on the
  // very first page): only a load that starts under the worker is.
  if (!(await page.evaluate(() => !!navigator.serviceWorker.controller))) await page.reload();
  await expect.poll(() => page.evaluate(() => !!navigator.serviceWorker.controller)).toBe(true);
}

async function object(page: Page, name: string): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { name, type: 'bike', description: '' } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

test('an app started offline opens as the last user and shows saved objects', async ({ page, context }) => {
  await signInFresh(page, '25-offline-open');
  const id = await object(page, 'Offline Bike');
  await page.goto('/');
  await underServiceWorker(page);
  // Both reads happen online first, under the worker, so `logb-api` holds them.
  await expect(page.getByText('Offline Bike')).toBeVisible();
  await page.goto(`/objects/${id}`);
  await expect(page.getByRole('heading', { name: 'Offline Bike' })).toBeVisible();

  // A full load with no connection: the shell from the precache, `/api/auth/*` failing, and the
  // remembered profile opening the app on what the caches hold.
  await context.setOffline(true);
  await page.goto('/');
  // The top bar's note (`role="status"`), not merely any text containing "Offline": the object
  // card "Offline Bike" would match that too, and prove nothing about offline mode.
  await expect(page.getByRole('status').filter({ hasText: /Offline/ })).toBeVisible();
  await expect(page.getByText('Offline Bike')).toBeVisible();
  await page.goto(`/objects/${id}`);
  await expect(page.getByRole('heading', { name: 'Offline Bike' })).toBeVisible();
  await context.setOffline(false);
});

test('another user never sees the previous user\'s saved objects', async ({ page, context }) => {
  await signInFresh(page, '25-offline-a');
  await object(page, 'Private Of A');
  await page.goto('/');
  await underServiceWorker(page);
  await expect(page.getByText('Private Of A')).toBeVisible();

  // A never signs out: their session just lapses (cookies gone), which runs no logout -- the
  // case the cache owner exists for. Without it `signInFresh` would find A still signed in and
  // no login form. B then signs in on the same device (signInFresh signs the admin in and out
  // first, then B through the login form).
  await context.clearCookies();
  await signInFresh(page, '25-offline-b');
  await page.goto('/');
  await expect(page.getByText('Private Of A')).toHaveCount(0);
  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByText('Private Of A')).toHaveCount(0);
  await context.setOffline(false);
});

test('after signing out, an offline start shows the sign-in screen, not the old session', async ({ page, context }) => {
  await signInFresh(page, '25-offline-out');
  await page.goto('/');
  await underServiceWorker(page);
  // The app's own sign-out, not an API call: the point is that IT forgets the profile.
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Account/ }).click();
  // Exact: the account page also offers "Sign out everywhere". Inside `main`: on desktop the
  // sidebar carries its own "Sign out" too.
  await page.locator('main').getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByText(/My objects|Meine Objekte/)).toHaveCount(0);
  await context.setOffline(false);
});
