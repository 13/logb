import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('tag an object and an entry, see coloured chips, and filter by tapping one', async ({ page }) => {
  await signInFresh(page, '23-tags');
  // A tag used before, so the form can suggest it.
  const seed = await page.request.post('/api/objects', { data: { name: 'Tag Seed', type: 'other', description: '', tags: ['Winter'] } });
  expect(seed.ok()).toBe(true);

  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Tag Golf');
  await page.getByLabel('Type').selectOption('car');
  // exact: the avatar's "Signed in as e2e-23tags-…" label contains "tags" too.
  const tagInput = page.getByLabel('Tags', { exact: true });
  await tagInput.fill('Lease');
  await tagInput.press('Enter');
  await tagInput.fill('win');
  await expect(page.locator('datalist option[value="Winter"]')).toHaveCount(1);
  await tagInput.fill('Winter');
  await tagInput.press('Enter');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);

  // Info tab chips.
  await page.getByRole('button', { name: 'Info', exact: true }).click();
  await expect(page.locator('.tag', { hasText: 'Lease' }).first()).toBeVisible();

  // An entry with a tag; the timeline shows it and filters by it.
  const objectId = Number(new URL(page.url()).pathname.split('/').pop());
  for (const [title, tags] of [['Tyres', ['Winter']], ['Wash', []]] as const) {
    const res = await page.request.post(`/api/objects/${objectId}/activities`, { data: { date: '2026-03-01', category: 'maintenance', title, notes: '', tags } });
    expect(res.ok()).toBe(true);
  }
  await page.goto(`/objects/${objectId}`);
  await expect(page.getByText('Wash')).toBeVisible();
  // Timeline.svelte has no single root element; an entry and its chips share an `.entry-row`.
  await page.locator('.entry-row .tag', { hasText: 'Winter' }).first().click();
  await expect(page.getByText('Wash')).toHaveCount(0);
  await expect(page.getByText('Tyres')).toBeVisible();

  // Objects list: chips on cards, tapping filters, the filter is removable.
  await page.goto('/');
  const golfCard = page.locator('.card-row', { hasText: 'Tag Golf' });
  await expect(golfCard.locator('.tag', { hasText: 'Lease' })).toBeVisible();
  const chipColour = await golfCard.locator('.tag', { hasText: 'Lease' }).evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(chipColour).not.toBe('rgba(0, 0, 0, 0)');
  await golfCard.locator('.tag', { hasText: 'Lease' }).click();
  await expect(page.getByText('Tag Seed')).toHaveCount(0);
  await expect(page.getByText('Tag Golf')).toBeVisible();
  await page.getByRole('button', { name: 'Clear tag filter' }).click();
  await expect(page.getByText('Tag Seed')).toBeVisible();
});
