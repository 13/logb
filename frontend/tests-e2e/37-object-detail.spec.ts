import { expect, test, type Page } from '@playwright/test';
import { openInfo, pngPayload, signInFresh } from './helpers';

/** The object page as round 3 of the UI overhaul left it: tabs, summary, panes, timeline, charts.
 *  Seeds through the API so each test drives only the screen it is about. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id as number;
}

async function entry(page: Page, id: number, data: Record<string, unknown>): Promise<void> {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', title: 'Entry', category: 'other', ...data } });
  expect(res.ok()).toBe(true);
}
/** A date `n` days back, as the API takes it. */
const daysAgo = (n: number) => new Date(Date.now() - n * 86_400_000).toISOString().slice(0, 10);

test('the tabs stay on one row and the due count sits inside the Reminders tab', async ({ page }) => {
  await signInFresh(page, '37-tabs');
  const id = await object(page, { name: 'Tabs car', type: 'car', counter_unit: 'km' });
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Tabs overdue', due_date: '2000-01-01' } })).ok()).toBe(true);
  await page.goto(`/objects/${id}`);

  const tabs = page.getByRole('tab');
  await expect(tabs.first()).toBeVisible();
  const boxes = await Promise.all((await tabs.all()).map((tab) => tab.boundingBox()));
  const ys = boxes.map((b) => Math.round(b!.y));
  // One row: the audit's red "1" dropped onto a second line under "Reminders".
  expect(Math.max(...ys) - Math.min(...ys)).toBeLessThanOrEqual(1);
  for (const b of boxes) expect(b!.height).toBeGreaterThanOrEqual(44);

  const reminders = page.getByRole('tab', { name: /^Reminders/ });
  await expect(reminders).toHaveAccessibleName('Reminders (1 due)');
  const tab = (await reminders.boundingBox())!;
  const badge = (await reminders.getByTestId('tab-due-badge').boundingBox())!;
  expect(badge.y).toBeGreaterThanOrEqual(tab.y);
  expect(badge.y + badge.height).toBeLessThanOrEqual(tab.y + tab.height);
  expect(tab.height).toBeLessThan(56);

  // A tab list moves with the arrow keys, and the address follows the tab.
  await page.getByRole('tab', { name: 'Timeline', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: 'Documents', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=documents$`));
});

/** Four tabs and a two-digit due badge on a 360 px phone: the tab row must not scroll sideways,
 *  or the last tab ("Info") is clipped. German labels are the longer ones. */
async function tabsFitAt360(page: Page, label: string): Promise<void> {
  await page.setViewportSize({ width: 360, height: 740 });
  await signInFresh(page, label);
  const id = await object(page, { name: 'Many due', type: 'car', counter_unit: 'km' });
  for (let i = 0; i < 12; i++) {
    const res = await page.request.post(`/api/objects/${id}/reminders`, { data: { title: `Due ${i}`, due_date: '2000-01-01' } });
    expect(res.ok()).toBe(true);
  }
  await page.goto(`/objects/${id}`);
  await expect(page.getByTestId('tab-due-badge')).toHaveText('12');
  const fits = await page.getByRole('tablist').evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth }));
  expect(fits.scroll).toBeLessThanOrEqual(fits.client);
}

test('four tabs and a two-digit badge fit a 360 px phone (English)', async ({ page }) => {
  await tabsFitAt360(page, '37-fit-en');
});

test.describe('German', () => {
  test.use({ locale: 'de-DE' });
  test('four tabs and a two-digit badge fit a 360 px phone (German)', async ({ page }) => {
    await tabsFitAt360(page, '37-fit-de');
    await expect(page.getByRole('tab', { name: /^Erinnerungen/ })).toBeVisible();
  });
});

test('a chart leaves out the months with nothing in them', async ({ page }) => {
  await signInFresh(page, '37-chart');
  const car = await object(page, { name: 'Chart car', type: 'car', counter_unit: 'km' });
  // Two spends, 67 days apart: two different months inside the last twelve, ten empty ones.
  await entry(page, car, { date: daysAgo(3), category: 'repair', title: 'Recent', cost_cents: 10_000 });
  await entry(page, car, { date: daysAgo(70), category: 'repair', title: 'Earlier', cost_cents: 5_000 });
  await page.goto(`/objects/${car}`);
  await openInfo(page);

  await expect(page.getByTestId('insights-spend').getByTestId('chart-bar')).toHaveCount(2);
  // What a screen reader gets: one row per drawn month, named by the chart's own heading.
  await expect(page.getByRole('table', { name: 'Spend per month' }).getByRole('row')).toHaveCount(2);
});

test('the summary shows the figures that apply, and nothing about charging on a diesel car', async ({ page }) => {
  await signInFresh(page, '37-figures');
  const car = await object(page, { name: 'Figures diesel', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  for (const [date, counter_value, quantity_milli, cost_cents] of [
    ['2026-01-01', 10_000, 40_000, 6_000], ['2026-02-01', 10_500, 30_000, 4_500], ['2026-03-01', 11_000, 25_000, 3_800],
  ] as const) {
    await entry(page, car, { date, category: 'fuel', counter_value, quantity_milli, cost_cents, charged_full: 1 });
  }
  const drill = await object(page, { name: 'Figures drill', type: 'tool' });

  await page.goto(`/objects/${car}`);
  await expect(page.getByTestId('figure-cost')).toContainText('€143.00');
  await expect(page.getByTestId('figure-counter')).toContainText('11,000 km');
  await expect(page.getByTestId('figure-consumption')).toContainText('l/100 km');
  // Full fills make a "distance per charge" computable; it is still not a thing a diesel car has.
  await openInfo(page);
  await expect(page.getByRole('heading', { name: 'Consumption per fill' })).toBeVisible();
  await expect(page.getByText(/Distance per charge/)).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Energy', exact: true })).toHaveCount(0);

  await page.goto(`/objects/${drill}`);
  await expect(page.getByTestId('figure-cost')).toBeVisible();
  await expect(page.getByTestId('figure-activities')).toBeVisible();
  await expect(page.getByTestId('figure-counter')).toHaveCount(0);
  await expect(page.getByTestId('figure-consumption')).toHaveCount(0);
});

test('a due reminder in the summary opens the Reminders tab; the cover keeps 16:10', async ({ page }) => {
  await signInFresh(page, '37-summary');
  const car = await object(page, { name: 'Summary car', type: 'car', counter_unit: 'km' });
  expect((await page.request.post(`/api/objects/${car}/reminders`, { data: { title: 'Summary overdue', due_date: '2000-01-01' } })).ok()).toBe(true);
  await page.goto(`/objects/${car}`);

  const due = page.getByTestId('summary-due-reminder').filter({ hasText: 'Summary overdue' });
  await expect(due).toContainText(/days overdue/);
  await due.click();
  await expect(page.getByRole('tab', { name: /^Reminders/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(/\?tab=reminders$/);

  await page.getByRole('tab', { name: 'Documents', exact: true }).click();
  await page.getByRole('button', { name: /Add photos or files/ }).click();
  await page.setInputFiles('input[type=file]', pngPayload());
  await page.getByRole('button', { name: 'Use as cover' }).click();
  const cover = page.getByTestId('object-cover');
  await expect(cover).toBeVisible();
  const box = (await cover.boundingBox())!;
  expect(box.width / box.height).toBeCloseTo(1.6, 1);
});

test('from 1024 px the summary is a sticky pane beside the tabs, with no Info tab', async ({ page }, info) => {
  test.skip(info.project.name !== 'desktop', 'the two panes start at 1024 px');
  await signInFresh(page, '37-panes');
  const car = await object(page, { name: 'Panes car', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  for (let i = 0; i < 30; i++) {
    await entry(page, car, { date: `2026-0${1 + (i % 8)}-1${i % 10}`, category: 'maintenance', title: `Pane entry ${i}`, cost_cents: 1000 });
  }
  await page.goto(`/objects/${car}`);

  const summary = page.getByRole('region', { name: 'Summary' });
  const tablist = page.getByRole('tablist');
  await expect(tablist).toBeVisible();
  await expect(tablist.getByRole('tab', { name: 'Info' })).toHaveCount(0);
  const s = (await summary.boundingBox())!;
  const tl = (await tablist.boundingBox())!;
  expect(s.x + s.width).toBeLessThanOrEqual(tl.x);
  expect(s.width).toBeGreaterThan(290);
  expect(s.width).toBeLessThan(320);
  // What the Info tab held is in the pane.
  await expect(summary.getByRole('heading', { name: 'Contents' })).toBeVisible();
  await expect(summary.getByRole('button', { name: /New object inside/ })).toBeVisible();

  // "+ Log" (a menu for a car with fuel) and Edit are in the page header, not floating.
  const header = page.getByRole('main').locator('header').first();
  await expect(header.getByTestId('log-menu')).toBeVisible();
  await expect(header.getByRole('button', { name: 'Edit' })).toBeVisible();

  // The pane stays in view while the timeline scrolls.
  await page.evaluate(() => window.scrollTo(0, 1500));
  expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(1000);
  const y = (await summary.boundingBox())!.y;
  expect(y).toBeGreaterThanOrEqual(60);
  expect(y).toBeLessThanOrEqual(100);

  // An old ?tab=info link lands on the timeline; the details are already beside it.
  await page.goto(`/objects/${car}?tab=info`);
  await expect(page.getByRole('tab', { name: 'Timeline', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(new RegExp(`/objects/${car}$`));
});

test('below 1024 px the summary sits above the tabs and the rest is under Info', async ({ page }, info) => {
  test.skip(info.project.name !== 'mobile', 'the one-column layout');
  await signInFresh(page, '37-one-column');
  const car = await object(page, { name: 'Column car', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  await entry(page, car, { date: '2026-02-01', category: 'maintenance', title: 'Column entry' });
  await page.goto(`/objects/${car}`);

  const summary = page.getByRole('region', { name: 'Summary' });
  await expect(page.getByRole('tablist')).toBeVisible();
  expect((await summary.boundingBox())!.y).toBeLessThan((await page.getByRole('tablist').boundingBox())!.y);
  await expect(page.getByRole('heading', { name: 'Contents' })).toHaveCount(0);
  // "+ Log" floats; the header holds only Edit.
  await expect(page.getByRole('main').locator('header').first().getByTestId('log-menu')).toHaveCount(0);
  await expect(page.getByTestId('log-menu')).toBeVisible();
  await page.getByRole('tab', { name: 'Info', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Contents' })).toBeVisible();
});
