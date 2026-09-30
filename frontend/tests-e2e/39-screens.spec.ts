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
