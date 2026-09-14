import { test, expect, type Page } from '@playwright/test';
import { ADMIN, signInFresh } from './helpers';

/** The service worker must control the page before an offline reload can be served from it. */
async function underServiceWorker(page: Page) {
  await page.evaluate(async () => { await navigator.serviceWorker.ready; });
  // The built `sw.js` calls `clientsClaim()`, so an open page is taken over once the worker
  // activates -- but that can land a moment after `ready` resolves. A page still uncontrolled is
  // reloaded, because a load that starts under an active worker is controlled for certain.
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
  await object(page, 'Own Of B');
  await page.goto('/');
  // B's list is really on screen -- not a blank or "Loading…" page that would show nothing of A's
  // either.
  await expect(page.getByText('Own Of B')).toBeVisible();
  await expect(page.getByText('Private Of A')).toHaveCount(0);
  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByRole('status').filter({ hasText: /Offline/ })).toBeVisible();
  await expect(page.getByText('Own Of B')).toBeVisible();
  await expect(page.getByText('Private Of A')).toHaveCount(0);
  await context.setOffline(false);
});

/**
 * With no connection the session check cannot answer, and signing out forgot the profile an
 * offline start would open as -- so the app cannot tell who (if anyone) is signed in and stays on
 * its loading screen. It shows no sign-in form either: that needs the server's answer.
 */
test('after signing out, an offline start stays on the loading screen and shows nothing of the old session', async ({ page, context }) => {
  await signInFresh(page, '25-offline-out');
  await object(page, 'Private Of Out');
  await page.goto('/');
  await underServiceWorker(page);
  await expect(page.getByText('Private Of Out')).toBeVisible();
  // The app's own sign-out, not an API call: the point is that IT forgets the profile.
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Account/ }).click();
  // Exact: the account page also offers "Sign out everywhere". Inside `main`: on desktop the
  // sidebar carries its own "Sign out" too.
  await page.locator('main').getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByText(/^(Loading…|Lädt…)$/)).toBeVisible();
  await expect(page.getByText(/My objects|Meine Objekte/)).toHaveCount(0);
  await expect(page.getByText('Private Of Out')).toHaveCount(0);
  await context.setOffline(false);
});

/**
 * The owner check on its own, with no 401 anywhere to clear the caches first: A's session is
 * simply replaced by B's cookie, and the only thing standing between B and A's cached objects is
 * the recorded owner.
 */
test('the cache owner alone keeps the previous user\'s saved objects from the next one', async ({ page, context }) => {
  await signInFresh(page, '25-owner-a');
  const id = await object(page, 'Owner Check A');
  await page.goto('/');
  await underServiceWorker(page);
  await expect(page.getByText('Owner Check A')).toBeVisible();
  await page.goto(`/objects/${id}`);
  await expect(page.getByRole('heading', { name: 'Owner Check A' })).toBeVisible();

  // Off the app while the cookies change hands, so no screen of A's can make a request that 401s.
  await page.goto('about:blank');
  await context.clearCookies();
  const b = `e2e-owner-b-${Date.now().toString(36).slice(-6)}`;
  expect((await page.request.post('/api/auth/login', { data: ADMIN })).ok()).toBe(true);
  expect((await page.request.post('/api/users', { data: { username: b, password: 'password123', is_admin: false } })).ok()).toBe(true);
  expect((await page.request.post('/api/auth/logout')).ok()).toBe(true);
  expect((await page.request.post('/api/auth/login', { data: { username: b, password: 'password123' } })).ok()).toBe(true);
  await object(page, 'Owner Check B');

  await page.goto('/');
  await expect(page.getByText('Owner Check B')).toBeVisible();
  await expect(page.getByText('Owner Check A')).toHaveCount(0);

  await context.setOffline(true);
  await page.reload();
  await expect(page.getByRole('status').filter({ hasText: /Offline/ })).toBeVisible();
  await expect(page.getByText('Owner Check B')).toBeVisible();
  await expect(page.getByText('Owner Check A')).toHaveCount(0);
  await context.setOffline(false);
});

/**
 * `logout` fails on the network error before touching any session state (see `signOutErrorMessage`
 * in ../src/stores/session.ts): the request to `/api/auth/logout` never reaches the server, so
 * nothing about the signed-in session changes, and the caller shows a message that says so rather
 * than whatever the browser's fetch error happens to be.
 */
test('signing out with no connection says so, and leaves the user signed in', async ({ page, context }) => {
  await signInFresh(page, '25-offline-signout');
  await page.goto('/');
  await underServiceWorker(page);

  await context.setOffline(true);
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: /Account/ }).click();
  // Exact: the account page also offers "Sign out everywhere". Inside `main`: on desktop the
  // sidebar carries its own "Sign out" too.
  await page.locator('main').getByRole('button', { name: 'Sign out', exact: true }).click();

  await expect(page.getByText('Signing out needs a connection.')).toBeVisible();
  // Still on a signed-in page: no navigation to /login, and the account page still names the
  // user and offers to sign out again (nothing about the session actually changed).
  await expect(page).not.toHaveURL(/\/login$/);
  await expect(page.getByRole('heading', { name: 'Account' })).toBeVisible();
  await expect(page.locator('main').getByRole('button', { name: 'Sign out', exact: true })).toBeVisible();

  await context.setOffline(false);
});

/**
 * A1: `NetworkFirst` (see docs/superpowers/specs/2026-09-14-offline-api-cache-design.md, "A1")
 * falls back to `logb-api` once the network takes longer than 4s -- signed in, online, no
 * `context.setOffline`. `context.route` can intercept the requests the service worker itself
 * makes (verified against this Playwright/Chromium build), so this seeds the cache with a
 * response whose `Date` header is already 70s old -- unambiguously "stale" regardless of how
 * little real time separates the two requests in a fast-running test -- then makes the next
 * request to that SAME path hang past the 4s timeout so `NetworkFirst` falls back to it.
 *
 * Routes only the one request the dashboard actually renders from (`?all=true&archived=false`),
 * not the parallel `archived=true` request it also fires: `servingSaved` now tracks staleness
 * per path (see ../src/lib/api.ts), so an unrelated sibling request answering fresh no longer
 * races this one back to false before the assertion below runs -- which is exactly what made an
 * earlier version of this test unreliable (see the report's "A1 e2e decision").
 */
test('shows saved data while online when the network is slower than the cache timeout', async ({ page, context }) => {
  await signInFresh(page, '25-slow-network');
  await object(page, 'Slow Network Bike');
  await page.goto('/');
  await underServiceWorker(page);
  await expect(page.getByText('Slow Network Bike')).toBeVisible();

  let calls = 0;
  await context.route('**/api/objects?all=true&archived=false', async (route) => {
    calls++;
    try {
      if (calls === 1) {
        // Seeds `logb-api` with a response that is already stale on arrival, so the FALLBACK
        // below (not this direct answer) is what the page ends up seeing.
        const response = await route.fetch();
        await route.fulfill({
          response,
          headers: { ...response.headers(), date: new Date(Date.now() - 70_000).toUTCString(), 'cache-control': 'no-store' },
          body: await response.body(),
        });
        return;
      }
      // NetworkFirst's own timeout is 4s; outlasting it is what makes it fall back to the cache
      // entry seeded above instead of waiting for this (otherwise perfectly fine) response.
      await new Promise((r) => setTimeout(r, 4_500));
      await route.continue();
    } catch {
      // A reload can cancel a still-in-flight request out from under this handler (or, on a
      // slower viewport, a second genuine request to the same path can race this one) -- either
      // way Playwright then refuses a further continue/fulfill on that same route. Harmless for
      // this test: its assertions are about what the PAGE ends up showing, not about every
      // individual route dispatch completing cleanly.
    }
  });

  // Populates `logb-api` with the backdated response above.
  await page.reload();
  await expect(page.getByText('Slow Network Bike')).toBeVisible();

  // This request is deliberately slow: `NetworkFirst` falls back to the cache entry instead.
  await page.reload();
  await expect(page.getByRole('status').filter({ hasText: /Offline/ })).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText('Slow Network Bike')).toBeVisible();

  await context.unroute('**/api/objects?all=true&archived=false');
});
