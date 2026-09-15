import { test, expect, type Page } from '@playwright/test';
import { pngPayload, signInFresh } from './helpers';

/** A fresh object made through the form, and its id. */
async function newObject(page: Page, name: string): Promise<number> {
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill(name);
  await page.getByLabel('Type').selectOption('other');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);
  return Number(new URL(page.url()).pathname.split('/').pop());
}

/** The entry's chips on the timeline: an entry and its chips share an `.entry-row`. */
function entryChip(page: Page, title: string, tag: string) {
  return page.locator('.entry-row', { hasText: title }).locator('.tag', { hasText: tag });
}

/** The stored row, straight from the server: no local or queued state can answer this. */
async function storedTags(page: Page, objectId: number, title: string): Promise<string[] | undefined> {
  const res = await page.request.get(`/api/objects/${objectId}/activities`);
  const rows = (await res.json()) as Array<{ title: string; tags: string[] }>;
  return rows.find((r) => r.title === title)?.tags;
}

test('an entry created and edited through the form keeps its tags on the timeline', async ({ page }) => {
  await signInFresh(page, '23-tags-form');
  const objectId = await newObject(page, 'Form Tag Bike');

  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Bremsbeläge vorne');
  const tagInput = page.getByLabel('Tags', { exact: true });
  await tagInput.fill('BBV');
  await tagInput.press('Enter');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(new RegExp(`/objects/${objectId}$`));
  await expect(entryChip(page, 'Bremsbeläge vorne', 'BBV')).toBeVisible();
  expect(await storedTags(page, objectId, 'Bremsbeläge vorne')).toEqual(['BBV']);

  // Edit: open the entry, add a second tag.
  await page.locator('.entry-row', { hasText: 'Bremsbeläge vorne' }).locator('button.entry').click();
  await expect(page.getByLabel('Title')).toHaveValue('Bremsbeläge vorne');
  await expect(page.locator('.tag-input .tag', { hasText: 'BBV' })).toBeVisible();
  await tagInput.fill('Bremse');
  await tagInput.press('Enter');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(new RegExp(`/objects/${objectId}$`));
  await expect(entryChip(page, 'Bremsbeläge vorne', 'BBV')).toBeVisible();
  await expect(entryChip(page, 'Bremsbeläge vorne', 'Bremse')).toBeVisible();
  expect(await storedTags(page, objectId, 'Bremsbeläge vorne')).toEqual(['BBV', 'Bremse']);
});

test('a tag typed but not committed with Enter is saved with the entry', async ({ page }) => {
  await signInFresh(page, '23-tags-blur');
  const objectId = await newObject(page, 'Blur Tag Bike');
  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Kette');
  await page.getByLabel('Tags', { exact: true }).fill('Antrieb');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(new RegExp(`/objects/${objectId}$`));
  await expect(entryChip(page, 'Kette', 'Antrieb')).toBeVisible();
  expect(await storedTags(page, objectId, 'Kette')).toEqual(['Antrieb']);
});

test('an entry logged offline keeps its tag while queued and once it syncs', async ({ page, context }) => {
  await signInFresh(page, '23-tags-offline');
  const objectId = await newObject(page, 'Offline Tag Bike');
  await page.getByRole('button', { name: /Log/ }).first().click();

  await context.setOffline(true);
  await page.getByLabel('Title').fill('Bremsbeläge hinten');
  const tagInput = page.getByLabel('Tags', { exact: true });
  await tagInput.fill('BBH');
  await tagInput.press('Enter');
  await page.getByRole('button', { name: 'Save' }).click();
  // The pending entry, rendered from the queued op.
  await expect(entryChip(page, 'Bremsbeläge hinten', 'BBH')).toBeVisible();

  await context.setOffline(false);
  await page.evaluate(() => window.dispatchEvent(new Event('online')));
  await expect.poll(() => storedTags(page, objectId, 'Bremsbeläge hinten'), { timeout: 15000 }).toEqual(['BBH']);
  await page.reload();
  await expect(page.locator('.entry-row', { hasText: 'Bremsbeläge hinten' }).locator('.pending-chip')).toHaveCount(0);
  await expect(entryChip(page, 'Bremsbeläge hinten', 'BBH')).toBeVisible();
});

test('a draft made by adding a file first keeps the tag added before saving', async ({ page }) => {
  await signInFresh(page, '23-tags-draft');
  const objectId = await newObject(page, 'Draft Tag Bike');
  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Rechnung Service');
  // The first click saves the draft; the same-labelled button is FilePicker's afterwards.
  await page.getByRole('button', { name: /Add photos or files/ }).click();
  await page.setInputFiles('input[type=file]', pngPayload());
  await expect(page.locator('.thumb-strip .strip-item')).toHaveCount(1);
  const tagInput = page.getByLabel('Tags', { exact: true });
  await tagInput.fill('Service');
  await tagInput.press('Enter');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(new RegExp(`/objects/${objectId}$`));
  await expect(entryChip(page, 'Rechnung Service', 'Service')).toBeVisible();
  expect(await storedTags(page, objectId, 'Rechnung Service')).toEqual(['Service']);
});

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

test('Enter on an empty tags field still submits the form', async ({ page }) => {
  await signInFresh(page, '23-tags-empty');
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Empty Tags');
  await page.getByLabel('Type').selectOption('other');
  await page.getByLabel('Tags', { exact: true }).press('Enter');
  await page.waitForURL(/\/objects\/\d+$/);
});

test('a tag typed past the character limit shows a described, announced error', async ({ page }) => {
  await signInFresh(page, '23-tags-toolong');
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Long Tag');
  await page.getByLabel('Type').selectOption('other');
  const tagInput = page.getByLabel('Tags', { exact: true });
  await tagInput.fill('x'.repeat(33));
  await tagInput.press('Enter');
  const errorText = page.getByText('A tag can be at most 32 characters.');
  await expect(errorText).toBeVisible();
  const describedBy = await tagInput.getAttribute('aria-describedby');
  expect(describedBy).toBeTruthy();
  const errorEl = page.locator(`#${describedBy}`);
  await expect(errorEl).toHaveAttribute('aria-live', 'polite');
  await expect(errorEl).toHaveText('A tag can be at most 32 characters.');
});

test('a tag still being typed when the form is submitted without leaving the field is saved', async ({ page }) => {
  await signInFresh(page, '23-tags-submit');
  const objectId = await newObject(page, 'Submit Tag Bike');
  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Schlauch');
  const tagInput = page.getByLabel('Tags', { exact: true });
  await tagInput.fill('Reifen');
  // An Android keyboard's Enter often arrives as key "Unidentified": the field's own keydown
  // handler lets it through and the browser submits the form with the focus still in the field,
  // so neither Enter nor blur ever commits the typed tag.
  await tagInput.dispatchEvent('keydown', { key: 'Unidentified' });
  await page.locator('form').evaluate((f: HTMLFormElement) => f.requestSubmit());
  await page.waitForURL(new RegExp(`/objects/${objectId}$`));
  expect(await storedTags(page, objectId, 'Schlauch')).toEqual(['Reifen']);
  await expect(entryChip(page, 'Schlauch', 'Reifen')).toBeVisible();
});

test('a typed tag that cannot be added stops the submit and keeps its error', async ({ page }) => {
  await signInFresh(page, '23-tags-submit-bad');
  const objectId = await newObject(page, 'Bad Tag Bike');
  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Zu lang');
  const tagInput = page.getByLabel('Tags', { exact: true });
  await tagInput.fill('x'.repeat(33));
  await page.locator('form').evaluate((f: HTMLFormElement) => f.requestSubmit());
  await expect(page.getByText('A tag can be at most 32 characters.')).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`/objects/${objectId}/activities/new$`));
  // Once nothing is in flight, a save that slipped through would have landed: check again.
  await page.waitForLoadState('networkidle');
  await expect(page).toHaveURL(new RegExp(`/objects/${objectId}/activities/new$`));
  await expect(page.getByLabel('Tags', { exact: true })).toBeFocused();
  const rows = (await (await page.request.get(`/api/objects/${objectId}/activities`)).json()) as unknown[];
  expect(rows).toHaveLength(0);
});

test('search shows tags on hits, and a tapped chip opens the object narrowed to that tag', async ({ page }) => {
  await signInFresh(page, '23-tags-search');
  const obj = await page.request.post('/api/objects', { data: { name: 'Suchrad Tagged', type: 'other', description: '', tags: ['Pendeln'] } });
  expect(obj.ok()).toBe(true);
  const objectId = (await obj.json()).id as number;
  for (const [title, tags] of [['Suchkette geölt', ['Antrieb']], ['Suchlicht getauscht', []]] as const) {
    const res = await page.request.post(`/api/objects/${objectId}/activities`, { data: { date: '2026-03-01', category: 'maintenance', title, notes: '', tags } });
    expect(res.ok()).toBe(true);
  }

  await page.goto('/search?q=Such');
  const objectHit = page.locator('.hit-row', { hasText: 'Suchrad Tagged' });
  await expect(objectHit.locator('.tag', { hasText: 'Pendeln' })).toBeVisible();
  const entryHit = page.locator('.hit-row', { hasText: 'Suchkette geölt' });
  await expect(entryHit.locator('.tag', { hasText: 'Antrieb' })).toBeVisible();
  await expect(page.locator('.hit-row', { hasText: 'Suchlicht getauscht' }).locator('.tag')).toHaveCount(0);
  // It leads elsewhere rather than toggling a filter here, so it says where and is no toggle.
  const searchChip = entryHit.getByRole('button', { name: 'Show entries tagged Antrieb' });
  await expect(searchChip).toBeVisible();
  await expect(searchChip).not.toHaveAttribute('aria-pressed');

  await entryHit.locator('.tag', { hasText: 'Antrieb' }).click();
  await page.waitForURL(new RegExp(`/objects/${objectId}$`));
  await expect(page.locator('.tag-filter', { hasText: 'Antrieb' })).toBeVisible();
  await expect(page.getByText('Suchkette geölt')).toBeVisible();
  await expect(page.getByText('Suchlicht getauscht')).toHaveCount(0);

  // An object hit's own tags are plain labels: they are rarely on its entries, so a link would
  // open an empty timeline.
  await page.goto('/search?q=Such');
  const objectChip = page.locator('.hit-row', { hasText: 'Suchrad Tagged' }).locator('.tag', { hasText: 'Pendeln' });
  await expect(objectChip).toBeVisible();
  await expect(page.locator('.hit-row', { hasText: 'Suchrad Tagged' }).getByRole('button', { name: /Pendeln/ })).toHaveCount(0);
});

test('a reminder row shows its object\'s tags on the dashboard and the reminders tab', async ({ page }) => {
  await signInFresh(page, '23-tags-reminders');
  const obj = await page.request.post('/api/objects', { data: { name: 'Erinnerungsrad', type: 'other', description: '', tags: ['E-Bike'] } });
  expect(obj.ok()).toBe(true);
  const objectId = (await obj.json()).id as number;
  const rem = await page.request.post(`/api/objects/${objectId}/reminders`, { data: { title: 'Kette prüfen', due_date: '2000-01-01' } });
  expect(rem.ok()).toBe(true);

  await page.goto('/');
  const row = page.locator('.banner li', { hasText: 'Kette prüfen' });
  await expect(row.locator('.tag', { hasText: 'E-Bike' })).toBeVisible();
  // Plain labels here: nothing on the dashboard to filter.
  await expect(row.locator('button.tag')).toHaveCount(0);

  await page.goto(`/objects/${objectId}?tab=reminders`);
  const card = page.locator('.card', { hasText: 'Kette prüfen' });
  await expect(card.locator('.tag', { hasText: 'E-Bike' })).toBeVisible();
});
