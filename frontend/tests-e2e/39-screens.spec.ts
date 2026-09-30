import { test, expect, type Page } from '@playwright/test';
import { signIn, signInFresh } from './helpers';

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

test('the floating buttons clear the tab bar on a phone and stay in the pane on a desktop', async ({ page }, info) => {
  await signInFresh(page, '39-fab');
  const id = await object(page, { name: 'Fab probe', type: 'tool' });
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Fab reminder', due_date: '2099-01-01' } })).ok()).toBe(true);
  for (const [path, name] of [['/', /^\+ Log$/], [`/objects/${id}?tab=reminders`, /^\+ New reminder$/]] as const) {
    await page.goto(path);
    const fab = page.getByRole('button', { name });
    await expect(fab).toBeVisible();
    const box = (await fab.boundingBox())!;
    const viewport = page.viewportSize()!;
    if (info.project.name === 'mobile') {
      const nav = (await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).boundingBox())!;
      expect(box.y + box.height, `${path}: above the tab bar`).toBeLessThanOrEqual(nav.y - 8);
      expect(Math.abs(viewport.width - (box.x + box.width) - 16), `${path}: 16 px from the edge`).toBeLessThan(2);
    } else {
      expect(box.x, `${path}: right of the sidebar`).toBeGreaterThan(240);
      expect(Math.abs(viewport.height - (box.y + box.height) - 16), `${path}: 16 px above the bottom`).toBeLessThan(2);
    }
  }
});

test("the browser's bar takes the page background of the theme chosen", async ({ page }) => {
  await signInFresh(page, '39-theme-color');
  await page.goto('/settings/appearance');
  await page.getByLabel('Theme', { exact: true }).selectOption('dark');
  await expect(page.locator('meta[name="theme-color"]').first()).toHaveAttribute('content', '#09090b');
  await page.getByLabel('Theme', { exact: true }).selectOption('light');
  await expect(page.locator('meta[name="theme-color"]').first()).toHaveAttribute('content', '#fafafa');
});

test('search hits are cards like the dashboard: icon tile, name, facts, tags inside', async ({ page }) => {
  await signInFresh(page, '39-search');
  const id = await object(page, { name: 'Kartenrad', type: 'bike', tags: ['Kartentag'] });
  expect((await page.request.post(`/api/objects/${id}/activities`, {
    data: { date: '2026-03-01', category: 'repair', title: 'Kartenschlauch', notes: '', cost_cents: 1250, tags: ['Kartentag'] },
  })).ok()).toBe(true);

  await page.goto('/search?q=Karten');
  const hits = page.getByTestId('search-hit');
  await expect(hits).toHaveCount(2);
  for (const title of ['Kartenrad', 'Kartenschlauch']) {
    const hit = hits.filter({ has: page.getByRole('button', { name: title, exact: true }) });
    await expect(hit.getByTestId('hit-icon')).toBeVisible();
    await expect(hit.locator('.tag', { hasText: 'Kartentag' })).toBeVisible();
  }
  // The whole card opens the entry, not only its title.
  const box = (await hits.filter({ has: page.getByRole('button', { name: 'Kartenschlauch', exact: true }) }).boundingBox())!;
  await page.mouse.click(box.x + box.width - 12, box.y + 12);
  await expect(page.getByLabel('Title')).toHaveValue('Kartenschlauch');
});

test('statistics lead with the year: spent, change against the same months last year, top object', async ({ page }) => {
  await page.clock.setFixedTime(new Date('2026-09-15T12:00:00'));
  await signInFresh(page, '39-stats');
  const car = await object(page, { name: 'Summary Car', type: 'car' });
  const house = await object(page, { name: 'Summary House', type: 'home' });
  const cost = async (id: number, date: string, cost_cents: number) => {
    expect((await page.request.post(`/api/objects/${id}/activities`, { data: { date, category: 'repair', title: 'Work', notes: '', cost_cents } })).ok()).toBe(true);
  };
  await cost(car, '2025-03-10', 100_000);
  await cost(house, '2025-11-10', 900_000); // after September: outside the comparison
  await cost(car, '2026-02-01', 50_000);
  await cost(house, '2026-07-01', 25_000);

  await page.goto('/stats');
  const summary = page.getByTestId('stats-summary');
  await expect(summary.getByTestId('stats-spent')).toContainText('Spent in 2026');
  await expect(summary.getByTestId('stats-spent')).toContainText('750.00');
  await expect(summary.getByTestId('stats-change')).toContainText('-25%');
  await expect(summary.getByTestId('stats-change')).toContainText('vs Jan–Sep 2025');
  await expect(summary.getByTestId('stats-top')).toContainText('Summary Car');

  // Summary first, then the money.
  const order = await page.locator('main [data-testid]').evaluateAll((els) => els.map((e) => e.getAttribute('data-testid')));
  expect(order.indexOf('stats-summary')).toBeLessThan(order.indexOf('stats-over-time'));

  // A past year compares with the whole year before, and the top card opens its object.
  await page.getByLabel('Year').selectOption('2025');
  await expect(summary.getByTestId('stats-spent')).toContainText('Spent in 2025');
  await expect(summary.getByTestId('stats-change')).toContainText('Nothing spent in 2024');
  await summary.getByRole('button', { name: 'Summary House' }).click();
  await page.waitForURL(`**/objects/${house}`);
});

test('the settings hub groups its rows, each with its own icon, and says who is signed in', async ({ page }) => {
  await signInFresh(page, '39-hub');
  await page.goto('/settings');
  const rows = page.getByRole('list', { name: 'You' }).getByRole('button');
  await expect(rows).toHaveCount(6);
  const icons = await rows.evaluateAll((els) => els.map((e) => e.querySelector('svg')?.innerHTML ?? ''));
  expect(new Set(icons).size, 'every row draws a different icon').toBe(6);
  for (const h of await rows.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height))) expect(h).toBeGreaterThanOrEqual(44);
  await expect(page.getByRole('main').getByTestId('signed-in')).toContainText('Signed in as');
  await expect(page.getByTestId('about')).toContainText('Version');
});

test('appearance saves itself: a change applies at once and says so', async ({ page }) => {
  await signInFresh(page, '39-appearance');
  await page.goto('/settings/appearance');
  await expect(page.getByRole('main').getByRole('button', { name: /Apply|Save/ })).toHaveCount(0);

  await page.getByLabel('Theme').selectOption('dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.getByRole('status')).toHaveText('Saved');
  await expect.poll(async () => (await (await page.request.get('/api/me/appearance')).json()).theme).toBe('dark');

  // The device-only box sits level with its label (the audit found it misaligned).
  const box = (await page.getByRole('checkbox', { name: 'Use these preferences only on this device' }).boundingBox())!;
  const label = (await page.getByText('Use these preferences only on this device', { exact: true }).boundingBox())!;
  expect(Math.abs(box.y + box.height / 2 - (label.y + label.height / 2))).toBeLessThan(4);
});

test('a change made just before leaving the page still reaches the server', async ({ page }) => {
  await signInFresh(page, '39-flush');
  await page.goto('/settings/appearance');
  // The worker must control the page, or this would not cover the path a real visit takes.
  if (!(await page.evaluate(() => !!navigator.serviceWorker.controller))) await page.reload();
  await page.waitForFunction(() => !!navigator.serviceWorker.controller);
  await page.getByLabel('Date format').selectOption('iso');
  // A real document navigation inside the 300 ms debounce: the timer dies with the page, so only
  // the pagehide flush (with keepalive) can deliver the save.
  await page.goto('/settings');
  await expect.poll(async () => (await (await page.request.get('/api/me/appearance')).json())?.dateFormat).toBe('iso');
  await page.goto('/settings/appearance');
  await expect(page.getByLabel('Date format')).toHaveValue('iso');
});

test('leaving by the in-app Back button sends a waiting change at once, not after the debounce', async ({ page }) => {
  await signInFresh(page, '39-flush-destroy');
  // A frozen clock: the 300 ms debounce can never fire, so only the destroy flush can send.
  await page.clock.install({ time: new Date('2026-09-30T10:00:00Z') });
  await page.goto('/settings/appearance');
  await page.clock.pauseAt(new Date('2026-09-30T10:00:05Z'));
  const sent = page.waitForRequest((r) => r.method() === 'PUT' && r.url().endsWith('/api/me/appearance'));
  await page.getByLabel('Date format').selectOption('iso');
  await page.getByRole('main').getByRole('button', { name: 'Back' }).click();
  await sent;
  await expect.poll(async () => (await (await page.request.get('/api/me/appearance')).json())?.dateFormat).toBe('iso');
});

test('changing the password is one explicit action, and a password field can show what was typed', async ({ page }) => {
  const username = await signInFresh(page, '39-account');
  await page.goto('/settings/account');
  await page.getByLabel('Current password').fill('password123');
  const fresh = page.getByLabel('New password');
  await fresh.fill('password456');
  await expect(fresh).toHaveAttribute('type', 'password');
  const show = page.locator('div[data-slot="password"]', { has: fresh }).getByRole('button', { name: 'Show password' });
  await expect(show).toHaveAttribute('aria-pressed', 'false');
  await show.click();
  await expect(fresh).toHaveAttribute('type', 'text');
  await expect(show).toHaveAttribute('aria-pressed', 'true');
  await expect(fresh).toHaveValue('password456');

  await page.getByRole('button', { name: 'Change password' }).click();
  await expect(page.getByRole('status')).toHaveText('Password changed');
  await expect(page.getByLabel('Current password')).toHaveValue('');
  expect((await page.request.post('/api/auth/login', { data: { username, password: 'password456' } })).ok()).toBe(true);
});

test('signing in is a centred card, and the password can be shown while typing', async ({ page }) => {
  // A fresh instance lands on setup instead; both screens share the card and the field.
  await page.goto('/login');
  await expect(page.getByTestId('auth-form')).toBeVisible();
  const password = page.getByLabel(/^(Password|Passwort)$/);
  await password.fill('secret words');
  await expect(password).toHaveAttribute('type', 'password');
  const toggle = page.getByRole('button', { name: 'Show password' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await expect(password).toHaveAttribute('type', 'text');
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(password).toHaveValue('secret words');
  const box = (await toggle.boundingBox())!;
  expect(box.width).toBeGreaterThanOrEqual(44);
  expect(box.height).toBeGreaterThanOrEqual(44);
});

test.describe('the sign-in chunk not yet fetched', () => {
  // No service worker: its precache would serve the chunk, and this is about the start it cannot.
  test.use({ serviceWorkers: 'block' });

  test('offline, sign-in waits for the connection instead of stranding the person', async ({ page, context }) => {
    await signIn(page); // an instance with an administrator, so a signed-out start lands on /login
    await page.request.post('/api/auth/logout');
    await page.evaluate(() => sessionStorage.removeItem('logb.chunk-reload'));
    // The connection drops just as the sign-in chunk is asked for.
    let first = true;
    await page.route(/\/assets\/Login-[^/]*\.js$/, async (route) => {
      if (!first) return route.continue();
      first = false;
      await context.setOffline(true);
      return route.abort();
    });
    await page.goto('/');
    await expect.poll(() => first).toBe(false);
    await expect(page.getByText(/^(Loading…|Lädt…)$/)).toBeVisible();
    // For a second the page stays the app: a reload gone wrong ends on the browser's error page.
    const since = Date.now();
    await expect.poll(() => (page.url().startsWith('chrome-error:') ? -1 : Date.now() - since), { intervals: [100], timeout: 5_000 }).toBeGreaterThan(1_000);
    await expect(page.getByText(/^(Loading…|Lädt…)$/)).toBeVisible();
    expect(await page.evaluate(() => sessionStorage.getItem('logb.chunk-reload'))).toBeNull();

    await context.setOffline(false);
    await expect(page.getByRole('heading', { name: /^(Sign in|Anmelden)$/ })).toBeVisible();
    expect(page.url()).toContain('/login');
    await expect(page.getByTestId('auth-form')).toBeVisible();
  });
});
