import { expect, test } from '@playwright/test';
import { signInFresh } from './helpers';

/** The dashboard's reminder area: one card per due reminder with a real snooze button, calm rows
 *  for what is coming up, and no object tags on either (they belong to the object, not the
 *  reminder). */
test('due reminders are cards with a snooze button; upcoming ones are plain rows', async ({ page }) => {
  await signInFresh(page, '36-dashboard-reminders');
  const obj = await page.request.post('/api/objects', { data: { name: 'Dash reminder car', type: 'car', counter_unit: 'km', tags: ['family'] } });
  expect(obj.ok()).toBe(true);
  const id = (await obj.json()).id as number;
  const soonDate = new Date(Date.now() + 5 * 86_400_000).toISOString().slice(0, 10);
  for (const data of [{ title: 'Dash overdue', due_date: '2000-01-01' }, { title: 'Dash soon', due_date: soonDate }]) {
    expect((await page.request.post(`/api/objects/${id}/reminders`, { data })).ok()).toBe(true);
  }

  await page.goto('/');
  const due = page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' });
  await expect(due).toContainText('Dash reminder car');
  await expect(due.locator('.tag')).toHaveCount(0);
  await expect(due.locator('li')).toHaveCount(0);
  const snooze = due.getByRole('button', { name: /Snooze/ });
  const box = (await snooze.boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);

  const soon = page.getByTestId('upcoming-reminder').filter({ hasText: 'Dash soon' });
  await expect(soon).toContainText(/in 5 days/);
  await expect(soon.locator('.tag')).toHaveCount(0);

  await due.getByRole('link', { name: /Dash overdue/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=reminders`));

  await page.goto('/');
  await page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' }).getByRole('button', { name: /Snooze/ }).click();
  await expect(page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' })).toHaveCount(0);
});

test('an object card holds its tags, a compact due badge and a labelled last entry', async ({ page }) => {
  await signInFresh(page, '36-dashboard-card');
  const obj = await page.request.post('/api/objects', { data: { name: 'Card drill', type: 'tool', tags: ['garage'] } });
  const id = (await obj.json()).id as number;
  expect((await page.request.post(`/api/objects/${id}/activities`, { data: { date: '2025-06-10', category: 'other', title: 'Old entry' } })).ok()).toBe(true);
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Card due', due_date: '2000-01-01' } })).ok()).toBe(true);

  await page.goto('/');
  const card = page.getByTestId('object-card').filter({ hasText: 'Card drill' });
  // Tags inside the card's box, not hanging below it.
  const cardBox = (await card.boundingBox())!;
  const tagBox = (await card.getByRole('button', { name: 'garage' }).boundingBox())!;
  expect(tagBox.y + tagBox.height).toBeLessThanOrEqual(cardBox.y + cardBox.height);
  // The badge is as wide as its text, not the rest of the row.
  const badge = card.getByTestId('due-badge');
  await expect(badge).toHaveText(/1 reminder due/);
  expect((await badge.boundingBox())!.width).toBeLessThan(cardBox.width / 2);
  // A last entry older than a month says what the date is.
  await expect(card).toContainText(/Last entry Jun 2025/);
  // No unlabelled quick-log square any more.
  await expect(card.getByRole('button', { name: /^Log$/ })).toHaveCount(0);
  // The tag filters; the rest of the card opens the object.
  await card.getByRole('button', { name: 'garage' }).click();
  await expect(page.getByTestId('tag-filter')).toContainText('garage');
  await card.getByRole('button', { name: /Card drill/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}$`));
});

test('the dashboard has a subtitle, one toolbar row, and a grid on wide screens', async ({ page }, info) => {
  await signInFresh(page, '36-dashboard-toolbar');
  for (const name of ['Grid one', 'Grid two', 'Grid three']) {
    const r = await page.request.post('/api/objects', { data: { name, type: 'tool' } });
    const id = (await r.json()).id as number;
    await page.request.post(`/api/objects/${id}/activities`, { data: { date: '2026-01-02', category: 'purchase', title: 'Bought', cost_cents: 1000 } });
  }
  await page.goto('/');
  await expect(page.getByRole('main').locator('header')).toContainText(/3 active · .*30\.00 spent/);
  await expect(page.getByRole('button', { name: '+ New object' })).toBeVisible();

  const search = page.getByLabel('Search objects');
  const sort = page.getByLabel('Sort');
  const tabs = page.getByRole('button', { name: /^Active/ });
  if (info.project.name === 'desktop') {
    // One row: search, sort and the Active/Archived switch share a line.
    const ys = await Promise.all([search, sort, tabs].map(async (l) => Math.round((await l.boundingBox())!.y)));
    expect(Math.max(...ys) - Math.min(...ys)).toBeLessThan(12);
    // Two columns at 1280 px.
    const cards = page.getByTestId('object-card');
    const [a, b] = await Promise.all([cards.nth(0).boundingBox(), cards.nth(1).boundingBox()]);
    expect(Math.round(a!.y)).toBe(Math.round(b!.y));
  }
  await expect(tabs).toHaveAttribute('aria-pressed', 'true');
});

test('"+ Log" on the dashboard asks for the object, then opens its quick entry', async ({ page }) => {
  await signInFresh(page, '36-dashboard-log');
  const car = (await (await page.request.post('/api/objects', { data: { name: 'Picker car', type: 'car', counter_unit: 'km' } })).json()).id as number;
  await page.request.post('/api/objects', { data: { name: 'Picker drill', type: 'tool' } });
  await page.goto('/');

  await page.getByRole('button', { name: /^\+ Log$/ }).click();
  const dialog = page.getByRole('dialog', { name: /Log for which object/ });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel('Find an object')).toBeFocused();
  await dialog.getByLabel('Find an object').fill('car');
  await expect(dialog.getByRole('button', { name: /Picker drill/ })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  await page.getByRole('button', { name: /^\+ Log$/ }).click();
  await dialog.getByRole('button', { name: /Picker car/ }).click();
  // A car with a counter goes to its reading, as the old per-card "+" did.
  await expect(page).toHaveURL(new RegExp(`/objects/${car}/reading$`));
});
