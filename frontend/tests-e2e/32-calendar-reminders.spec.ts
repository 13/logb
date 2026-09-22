import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('calendar supports keyboard selection and dismissal with Sunday first', async ({ page }) => {
  await signInFresh(page, '32-keyboard');
  await page.goto('/settings/appearance');
  await page.getByLabel('First day of the week').selectOption('sunday');
  await page.getByLabel('Date format').selectOption('iso');
  const response = await page.request.post('/api/objects', { data: { name: 'Calendar keyboard', type: 'other' } });
  const object = await response.json();
  await page.goto(`/objects/${object.id}/reminders/new`);
  await page.getByLabel('Due date', { exact: true }).fill('2028-02-28');
  const trigger = page.getByRole('button', { name: 'Choose date', exact: true });
  await trigger.click();
  await expect(page.locator('.weekday').first()).toHaveText('Sun');
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('Enter');
  await expect(page.getByLabel('Due date', { exact: true })).toHaveValue('2028-02-29');
  await expect(trigger).toBeFocused();
  await trigger.click();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await trigger.click();
  await page.getByLabel('Title', { exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('counter and weight reminders offer the same fixed calendar options', async ({ page }) => {
  await signInFresh(page, '32-reading');
  for (const body of [{ name: 'Calendar meter', type: 'car', counter_unit: 'km' }, { name: 'Calendar weight', type: 'body' }]) {
    const response = await page.request.post('/api/objects', { data: body });
    expect(response.ok()).toBeTruthy();
    const object = await response.json();
    await page.goto(`/objects/${object.id}/reminders/new?kind=reading`);
    await page.getByLabel('Title', { exact: true }).fill(body.name);
    await page.getByLabel('Repeat', { exact: true }).selectOption('monthly');
    await page.getByLabel('Day of month').selectOption('last');
    await expect(page.getByText(/Next dates:/)).toBeVisible();
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page.locator('.card').filter({ hasText: body.name })).toContainText('Last day of month');
    const list = await (await page.request.get(`/api/objects/${object.id}/reminders`)).json();
    expect(list[0]).toMatchObject({ kind: 'reading', schedule: 'monthly:last', every_n: null, every_unit: null });
  }
});

test('a calendar reading reminder queued offline replays once', async ({ page, context }) => {
  await signInFresh(page, '32-offline');
  const response = await page.request.post('/api/objects', { data: { name: 'Offline calendar meter', type: 'car', counter_unit: 'km' } });
  const object = await response.json();
  await page.goto(`/objects/${object.id}/reminders/new?kind=reading`);
  await expect(page.getByLabel('Title', { exact: true })).not.toHaveValue('');
  await context.setOffline(true);
  await page.getByLabel('Title', { exact: true }).fill('Offline weekly reading');
  await page.getByLabel('Repeat', { exact: true }).selectOption('weekly');
  await page.locator('#weekday').selectOption('1');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${object.id}\\?tab=reminders`));
  await context.setOffline(false);
  await page.evaluate(() => window.dispatchEvent(new Event('online')));
  await expect.poll(async () => {
    const list = await (await page.request.get(`/api/objects/${object.id}/reminders`)).json();
    return list.filter((r: { title: string; schedule: string }) => r.title === 'Offline weekly reading' && r.schedule === 'weekly:1').length;
  }).toBe(1);
});

test('calendar reminders and the localized first weekday setting work together', async ({ page }) => {
  await signInFresh(page, '32-calendar');
  const created = await page.request.post('/api/objects', { data: { name: 'Wohnung', type: 'other' } });
  expect(created.ok()).toBeTruthy();
  const object = await created.json();

  await page.goto('/settings/appearance');
  await page.getByLabel('Language').selectOption('de');
  await page.getByLabel('Erster Wochentag').selectOption('monday');

  await page.goto(`/objects/${object.id}/reminders/new`);
  await page.getByRole('button', { name: 'Datum wählen' }).click();
  await expect(page.locator('.weekday').first()).toHaveText('Mo');
  await expect(page.getByRole('button', { name: 'Vorheriger Monat' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Nächster Monat' })).toBeVisible();
  await page.getByRole('button', { name: 'Datum wählen' }).click();

  await page.getByLabel('Titel').fill('Monatsende');
  await page.getByLabel('Wiederholung').selectOption('monthly');
  await page.getByLabel('Tag im Monat').selectOption('last');
  await page.getByRole('button', { name: 'Speichern' }).click();

  const reminder = page.locator('.card').filter({ hasText: 'Monatsende' });
  await expect(reminder).toContainText('Letzter Tag des Monats');
});
