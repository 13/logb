import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The persistent shell nav (see AppNav.svelte): every destination is a client-side route
 *  change, never a full page load, which is what lets the later assertions prove a setting
 *  change reaches an already-open view "without reload". */
async function nav(page: Page, name: string) {
  await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).getByRole('button', { name }).click();
}

async function chooseDateFormat(page: Page, label: string) {
  await nav(page, 'Settings');
  await page.getByRole('button', { name: /Appearance/ }).click();
  await page.getByLabel('Date format').selectOption({ label });
}

test('the date format setting is used everywhere, and the typed date field validates', async ({ page }) => {
  await signInFresh(page, '26-date-format');

  // 1. Choose dmy-dot in Appearance, then create an object and an entry through the form,
  // typing the date in that pattern.
  await chooseDateFormat(page, '15.09.2026');

  await nav(page, 'Objects');
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Date Format Golf');
  await page.getByLabel('Type').selectOption('car');
  // A counter unit, so the max-date coverage below (item 2 of the review) has a reading form to
  // exercise -- its date field is the one field in the app bound with a `max`.
  await page.getByLabel('Counter').selectOption('km');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Date Format Golf' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)![1];

  await page.getByRole('button', { name: /Log activity/ }).click();
  await page.getByLabel('Title').fill('Repaint');
  await page.getByLabel('Date', { exact: true }).fill('3.4.2026');
  await page.getByRole('button', { name: 'Save' }).click();

  await expect(page.getByText('Repaint').first()).toBeVisible();
  await expect(page.getByText('03.04.2026').first()).toBeVisible();

  // iOS's numeric keypad -- what `inputmode="numeric"` offers there -- has no `.`/`-`/`/` key at
  // all, so a date typed on it has no separators. Typing one in must still work, and once
  // committed the field must show the formatted rendering, not the raw digits typed.
  await page.getByText('Repaint').first().click();
  await expect(page).toHaveURL(/\/activities\/\d+$/);
  const digitsField = page.getByLabel('Date', { exact: true });
  await digitsField.fill('01052026');
  await digitsField.blur();
  await expect(digitsField).toHaveValue('01.05.2026');
  // Not saved -- this step only exercises typing and reformatting, not a real edit.
  await page.getByRole('button', { name: 'Cancel' }).click();

  // 2. Switching the format re-renders the same entry with no reload: every step from here to
  // the assertion below is a client-side route change (see `nav` and ObjectCard.svelte's own
  // navigation), never a `page.goto()`/`page.reload()`.
  await chooseDateFormat(page, '2026-09-15');
  await nav(page, 'Objects');
  await page.getByRole('button', { name: /Date Format Golf/ }).click();
  await expect(page.getByText('2026-04-03').first()).toBeVisible();

  // 3. An impossible date shows the error, described by aria-describedby, and refuses to save --
  // switch back to dmy-dot so the placeholder/example below reads "15.09.2026", as the brief.
  await chooseDateFormat(page, '15.09.2026');
  await nav(page, 'Objects');
  await page.getByRole('button', { name: /Date Format Golf/ }).click();
  await page.getByText('Repaint').first().click();
  await expect(page).toHaveURL(/\/activities\/\d+$/);

  const dateField = page.getByLabel('Date', { exact: true });
  await dateField.fill('31.02.2026');
  await dateField.blur();
  const errorText = page.getByText('Enter a date like 15.09.2026');
  await expect(errorText).toBeVisible();
  const describedBy = await dateField.getAttribute('aria-describedby');
  expect(describedBy).toBeTruthy();
  await expect(page.locator(`#${describedBy}`)).toHaveText('Enter a date like 15.09.2026');
  // Not just our own rendered error text: the field's native `validationMessage` must be
  // non-empty too, since that -- not our own markup -- is what actually withholds the submit.
  expect(await dateField.evaluate((el) => (el as HTMLInputElement).validationMessage)).not.toBe('');

  const urlBeforeSave = page.url();
  await page.getByRole('button', { name: 'Save' }).click();
  // The browser's own constraint validation refuses to fire the form's submit while the field
  // still carries an unresolved custom validity message (see DateInput.svelte's `markError`), so
  // clicking Save here neither loses the entry's real, unchanged date nor navigates away as
  // though a correction had been made.
  await expect(page).toHaveURL(urlBeforeSave);
  await expect(page.getByRole('heading', { name: 'Edit activity' })).toBeVisible();

  // Leaving the form without having corrected the date, and returning to the timeline, shows the
  // entry's real, unchanged, saved date -- proof that the blocked Save above truly saved nothing
  // (no late/silent save reached the server with the invalid or a reverted date).
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('03.04.2026').first()).toBeVisible();

  // 4. A max-bounded field (the reading form's date, the only field with a `max` in the app)
  // reports being out of range in words, not just silently refusing a value.
  await page.goto(`/objects/${objectId}/reading`);
  const readingDate = page.getByLabel('Date', { exact: true });
  await readingDate.fill('16.09.2026'); // one day after the fixed "today" (2026-09-15)
  await readingDate.blur();
  await expect(page.getByText('Choose a date on or before 15.09.2026')).toBeVisible();
  expect(await readingDate.evaluate((el) => (el as HTMLInputElement).validationMessage)).not.toBe('');
});
