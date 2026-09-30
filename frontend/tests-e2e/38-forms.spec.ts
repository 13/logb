import { expect, test, type Page } from '@playwright/test';
import { chooseType, openMoreDetails, signInFresh, typeTile } from './helpers';

/** The forms as round 4 of the UI overhaul left them. Seeds through the API so each test drives
 *  only the form it is about. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id as number;
}

/** Save is on screen: inside the viewport, a full-size target, and above a phone's tab bar. */
async function expectSaveOnScreen(page: Page): Promise<void> {
  const save = page.getByRole('button', { name: 'Save', exact: true });
  await expect(save).toBeVisible();
  const box = (await save.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  expect(box.height).toBeGreaterThanOrEqual(44);
  const nav = (await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).boundingBox())!;
  // A bottom tab bar (phone): Save sits above it. A sidebar (desktop) cannot cover it.
  if (nav.y > viewport.height / 2) expect(box.y + box.height).toBeLessThanOrEqual(nav.y + 0.5);
}

test('the reading form: one field pattern, and Save on screen from the start', async ({ page }) => {
  await signInFresh(page, '38-reading');
  const id = await object(page, { name: 'Forms reading car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reading`);

  const reading = page.getByLabel(/Reading \(km\)/);
  await expect(reading).toBeFocused();
  // The object's name is the page's subtitle now, not a stray paragraph in the form.
  await expect(page.getByText('Forms reading car', { exact: true })).toBeVisible();
  await expectSaveOnScreen(page);

  // The label belongs to its control: clicking it focuses the field.
  await page.getByLabel('Date', { exact: true }).focus();
  await page.getByText('Reading (km)', { exact: true }).click();
  await expect(reading).toBeFocused();
  await expect(reading).toHaveCSS('height', '56px');
});

test('the date field keeps its calendar button inside the box, at a full-size target', async ({ page }) => {
  await signInFresh(page, '38-date');
  const id = await object(page, { name: 'Date box car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reading`);

  const field = (await page.getByLabel('Date', { exact: true }).boundingBox())!;
  const button = page.getByRole('button', { name: 'Choose date', exact: true });
  const b = (await button.boundingBox())!;
  expect(b.x).toBeGreaterThanOrEqual(field.x);
  expect(b.x + b.width).toBeLessThanOrEqual(field.x + field.width);
  expect(b.y).toBeGreaterThanOrEqual(field.y);
  expect(b.y + b.height).toBeLessThanOrEqual(field.y + field.height);
  expect(Math.min(b.width, b.height)).toBeGreaterThanOrEqual(44);

  await button.click();
  await expect(page.getByTestId('calendar-weekday')).toHaveCount(7);
  const day = page.getByRole('dialog', { name: 'Choose date' }).getByRole('button', { pressed: true });
  await expect(day).toHaveCount(1);
  expect((await day.boundingBox())!.height).toBeGreaterThanOrEqual(44);
});

test('a reminder is due by date, by counter or by both, chosen up front', async ({ page }) => {
  await signInFresh(page, '38-due-by');
  const id = await object(page, { name: 'Due-by car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reminders/new`);

  const dueBy = page.getByRole('group', { name: 'Due by' });
  await expect(dueBy.getByRole('radio', { name: 'Date', exact: true })).toBeChecked();
  await expect(page.getByLabel('Due date', { exact: true })).toBeVisible();
  await expect(page.getByLabel(/^Due at/)).toHaveCount(0);
  await expectSaveOnScreen(page);

  await dueBy.getByRole('radio', { name: 'Counter', exact: true }).check();
  await expect(page.getByLabel('Due date', { exact: true })).toHaveCount(0);
  await expect(page.getByLabel('Repeat', { exact: true })).toHaveCount(0);
  // Words in the label, the unit in the box: no "(counter) (km)".
  await expect(page.getByText(/\(counter\)/)).toHaveCount(0);
  await page.getByLabel('Title').fill('Timing belt');
  await page.getByLabel(/^Due at/).fill('100000');
  await page.getByLabel(/^Then every/).fill('15000');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=reminders$`));

  const rows = (await (await page.request.get(`/api/objects/${id}/reminders`)).json()) as Array<Record<string, unknown>>;
  expect(rows.find((r) => r.title === 'Timing belt')).toMatchObject({ due_date: null, schedule: null, every_n: null, due_counter: 100000, repeat_counter: 15000 });

  // "Both" says what it means, and a saved reminder opens on its own side.
  await page.goto(`/objects/${id}/reminders/new`);
  await dueBy.getByRole('radio', { name: 'Both', exact: true }).check();
  await expect(page.getByText('Due at whichever comes first.')).toBeVisible();
  const belt = rows.find((r) => r.title === 'Timing belt')!;
  await page.goto(`/objects/${id}/reminders/${belt.id}`);
  await expect(dueBy.getByRole('radio', { name: 'Counter', exact: true })).toBeChecked();
});

test('a counter field typed and then hidden is not saved', async ({ page }) => {
  await signInFresh(page, '38-due-switch');
  const id = await object(page, { name: 'Switch car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reminders/new`);
  const dueBy = page.getByRole('group', { name: 'Due by' });
  await page.getByLabel('Title').fill('Brake fluid');
  await dueBy.getByRole('radio', { name: 'Counter', exact: true }).check();
  await page.getByLabel(/^Due at/).fill('90000');
  await dueBy.getByRole('radio', { name: 'Date', exact: true }).check();
  await page.getByLabel('Due date', { exact: true }).fill('01/01/2031');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=reminders$`));
  const rows = (await (await page.request.get(`/api/objects/${id}/reminders`)).json()) as Array<Record<string, unknown>>;
  expect(rows.find((r) => r.title === 'Brake fluid')).toMatchObject({ due_date: '2031-01-01', due_counter: null });
});

test('a failed save leaves no stale error behind when the next save is refused by a field', async ({ page }) => {
  await signInFresh(page, '38-stale-error');
  const id = await object(page, { name: 'Stale error car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reminders/new`);
  let failed = false;
  await page.route(`**/api/objects/${id}/reminders`, (route) => {
    if (route.request().method() === 'POST' && !failed) { failed = true; return route.fulfill({ status: 422, contentType: 'application/json', body: '{"error":"boom"}' }); }
    return route.continue();
  });
  await page.getByLabel('Title').fill('Stale check');
  await page.getByLabel('Due date', { exact: true }).fill('01/01/2031');
  const bar = page.getByTestId('form-actions');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(bar.getByRole('alert')).toBeVisible();

  // The date is now refused: only the field's own message shows, and the Save bar is clean.
  await page.getByLabel('Due date', { exact: true }).fill('');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.locator('#dd-error')).toBeVisible();
  await expect(bar.getByRole('alert')).toHaveCount(0);
  await expect(page.getByRole('alert')).toHaveCount(1);
});

test('notes wait under "More details", which opens by itself for a reminder that has them', async ({ page }) => {
  await signInFresh(page, '38-reminder-notes');
  const id = await object(page, { name: 'Notes car', type: 'other' });
  const plain = await (await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Plain', due_date: '2031-01-01' } })).json();
  const noted = await (await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Noted', due_date: '2031-01-01', notes: 'Use DOT 4' } })).json();

  const toggle = page.getByRole('button', { name: 'More details' });
  await page.goto(`/objects/${id}/reminders/${plain.id}`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByLabel('Notes')).toBeHidden();
  await openMoreDetails(page);
  await expect(page.getByLabel('Notes')).toBeVisible();
  await page.goto(`/objects/${id}/reminders/${noted.id}`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel('Notes')).toHaveValue('Use DOT 4');

  // A refused save names the field under it and moves the focus there.
  await page.getByLabel('Title').fill('');
  await page.getByLabel('Title').evaluate((el: HTMLInputElement) => el.removeAttribute('required'));
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveText('Check “Title”: it is missing or not valid.');
  await expect(page.getByLabel('Title')).toBeFocused();
  await expect(page.getByLabel('Title')).toHaveAttribute('aria-invalid', 'true');
});

test('a new object starts with the type as tiles, nothing chosen, then templates, then the name', async ({ page }) => {
  await signInFresh(page, '38-type-tiles');
  await page.goto('/objects/new');

  const types = page.getByRole('group', { name: 'Type', exact: true });
  await expect(types.getByRole('radio')).toHaveCount(9);
  await expect(types.getByRole('radio', { checked: true })).toHaveCount(0);
  for (const radio of await types.getByRole('radio').all()) expect((await radio.boundingBox())!.height).toBeGreaterThanOrEqual(44);

  const templates = page.getByRole('region', { name: 'Or start from a template' });
  const typesBox = (await types.boundingBox())!;
  const templatesBox = (await templates.boundingBox())!;
  const nameBox = (await page.getByLabel('Name').boundingBox())!;
  expect(typesBox.y).toBeLessThan(templatesBox.y);
  expect(templatesBox.y).toBeLessThan(nameBox.y);
  await expectSaveOnScreen(page);

  // No type, no save: the message is under the tiles.
  await page.getByLabel('Name').fill('Unfiled thing');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveText('Check “Type”: it is missing or not valid.');
  await expect(page).toHaveURL(/\/objects\/new$/);
  await chooseType(page, 'tool');
  await expect(typeTile(page, 'tool')).toBeChecked();
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Unfiled thing' })).toBeVisible();
});

test('object details wait under "More details", which opens for an object that uses them', async ({ page }) => {
  await signInFresh(page, '38-object-details');
  const toggle = page.getByRole('button', { name: 'More details' });
  await page.goto('/objects/new');
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByLabel('Description')).toBeHidden();

  const plain = await object(page, { name: 'Plain drill', type: 'tool' });
  const described = await object(page, { name: 'Grey car', type: 'car', description: 'grey' });
  await page.goto(`/objects/${plain}/edit`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(typeTile(page, 'tool')).toBeChecked();
  await page.goto(`/objects/${described}/edit`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel('Description')).toHaveValue('grey');
  // A real checkbox, full-size target.
  const archive = page.getByRole('checkbox', { name: 'Archive' });
  await expect(archive).not.toBeChecked();
});

test('the Save bar stays on screen while the form scrolls, and never covers the last field', async ({ page }) => {
  await signInFresh(page, '38-sticky');
  await page.goto('/objects/new');
  await openMoreDetails(page);
  await page.evaluate(() => window.scrollTo(0, 400));
  await expectSaveOnScreen(page);
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await expectSaveOnScreen(page);
  const bar = (await page.getByTestId('form-actions').boundingBox())!;
  const last = (await page.getByLabel('Private object').boundingBox())!;
  expect(last.y + last.height).toBeLessThanOrEqual(bar.y);
});

test('the activity form: one Notes label, headings under the title, details on request', async ({ page }) => {
  await signInFresh(page, '38-activity');
  const id = await object(page, { name: 'Activity form car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/activities/new`);

  const size = async (loc: ReturnType<Page['locator']>) => loc.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  const title = await size(page.getByRole('heading', { level: 1 }));
  expect(await size(page.getByRole('heading', { name: 'Photos & documents' }))).toBeLessThan(title);
  await expect(page.getByRole('heading', { name: 'Notes' })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Details' })).toHaveCount(0);
  await expectSaveOnScreen(page);

  // The unit sits in the field; the label is words.
  await expect(page.getByLabel(/^Counter reading \(km\)$/)).toBeVisible();

  await expect(page.getByLabel('Notes')).toBeHidden();
  await openMoreDetails(page);
  await expect(page.getByText('Notes', { exact: true })).toHaveCount(1);
  await page.getByLabel('Title').fill('Wheel bolts');
  await page.getByLabel('Notes').fill('Torque 120 Nm');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}$`));

  // Opening it again: its notes are there, so the section is open.
  await page.getByTestId('timeline-entry').filter({ hasText: 'Wheel bolts' }).getByRole('button', { name: 'Wheel bolts' }).click();
  await expect(page.getByRole('button', { name: 'More details' })).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel('Notes')).toHaveValue('Torque 120 Nm');
});

test('a refused field inside a closed "More details" is shown and focused', async ({ page }) => {
  await signInFresh(page, '38-activity-reveal');
  const id = await object(page, { name: 'Reveal bike', type: 'e_bike', counter_unit: 'km' });
  await page.goto(`/objects/${id}/activities/new?category=trip`);
  await page.getByLabel(/^Start/).fill('100');
  await page.getByLabel(/^End/).fill('150');
  await openMoreDetails(page);
  await page.getByLabel(/Battery used/).fill('101');
  await page.getByRole('button', { name: 'More details' }).click();
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('button', { name: 'More details' })).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel(/Battery used/)).toBeFocused();
  await expect(page.getByRole('alert')).toHaveText('Battery used must be 0–100 %');
});

test('the Reminders tab asks the server for the reminders once', async ({ page }) => {
  await signInFresh(page, '38-reminders-once');
  const id = await object(page, { name: 'Once car', type: 'car', counter_unit: 'km' });
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Once overdue', due_date: '2000-01-01' } })).ok()).toBe(true);

  const gets: string[] = [];
  page.on('request', (r) => { if (r.method() === 'GET' && new URL(r.url()).pathname === `/api/objects/${id}/reminders`) gets.push(r.url()); });
  await page.goto(`/objects/${id}?tab=reminders`);
  await expect(page.getByTestId('reminder-card')).toHaveCount(1);
  // The summary's due reminder comes from the same answer.
  await expect(page.getByTestId('summary-due-reminder')).toContainText('Once overdue');
  await page.waitForLoadState('networkidle');
  expect(gets).toHaveLength(1);
});
