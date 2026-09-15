import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The persistent shell nav (see AppNav.svelte): every destination is a client-side route
 *  change. Mirrors the same helper in `26-date-format.spec.ts`. */
async function nav(page: Page, name: string) {
  await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).getByRole('button', { name }).click();
}

/** `label` is the fixed 2026-09-15 example each format renders to (see Appearance.svelte's
 *  `EXAMPLE`), not tied to whatever "today" is when this runs. */
async function chooseDateFormat(page: Page, label: string) {
  await nav(page, 'Settings');
  await page.getByRole('button', { name: /Appearance/ }).click();
  await page.getByLabel('Date format').selectOption({ label });
}

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function entry(page: Page, id: number, data: Record<string, unknown>) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', category: 'maintenance', ...data } });
  expect(res.ok()).toBe(true);
}

test('the Info tab shows what was last done, and tapping it filters the timeline by title', async ({ page }) => {
  await signInFresh(page, '27-last-done');
  await chooseDateFormat(page, '15.09.2026');

  const bike = await object(page, { name: 'Last Done E-Bike', type: 'e_bike', counter_unit: 'km' });
  await entry(page, bike, { date: '2026-01-10', title: 'Bremsbeläge vorne', counter_value: 1000 });
  await entry(page, bike, { date: '2026-05-12', title: 'Bremsbeläge vorne', counter_value: 3420 });
  await entry(page, bike, { date: '2026-02-01', title: 'Kette' });
  // The object's current counter is the highest logged, whatever its date -- so this reading
  // alone fixes "since" at 4650 - 3420 = 1230, without needing to be the newest-dated entry.
  await entry(page, bike, { date: '2026-06-01', category: 'reading', title: 'Stand', counter_value: 4650 });

  await page.goto(`/objects/${bike}`);
  await page.getByRole('button', { name: 'Info', exact: true }).click();

  await expect(page.getByRole('heading', { name: 'Last done', exact: true })).toBeVisible();
  const row = page.locator('.card.entry', { hasText: 'Bremsbeläge vorne' });
  await expect(row).toBeVisible();
  await expect(row).toContainText('12.05.2026');
  await expect(row).toContainText('3,420 km');
  await expect(row).toContainText('1,230 km ago');
  // "Kette" was logged only once and has no reminder, so it stays below the last-done bar.
  await expect(page.locator('.card.entry', { hasText: 'Kette' })).toHaveCount(0);

  // Tapping the row switches to the timeline, narrowed to the tapped title.
  await row.click();
  await expect(page.getByRole('button', { name: 'Timeline', exact: true })).toHaveClass(/active/);
  await expect(page.locator('.tag-filter', { hasText: 'Title: Bremsbeläge vorne' })).toBeVisible();
  const entries = page.locator('.entry-row');
  await expect(entries).toHaveCount(2);
  await expect(page.getByText('Kette')).toHaveCount(0);
  await expect(page.locator('.entry-row', { hasText: 'Bremsbeläge vorne' })).toHaveCount(2);

  // Clearing the chip shows every entry again.
  await page.getByRole('button', { name: 'Clear title filter' }).click();
  await expect(page.locator('.tag-filter')).toHaveCount(0);
  await expect(page.getByText('Kette')).toBeVisible();
});
