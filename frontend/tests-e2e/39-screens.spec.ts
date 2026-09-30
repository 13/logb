import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

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
