import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

test('calendar reminders and the localized first weekday setting work together', async ({ page }) => {
  await signInFresh(page, '31-calendar');
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
