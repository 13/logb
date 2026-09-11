import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

// Seeds its own object with a name no other spec uses: one server and one database are shared
// across the run.
test('a body object logs a symptom, and is not offered fuel', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Left shoulder');
  await page.getByLabel('Type').selectOption('body');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByRole('button', { name: /Log activity/ }).click();
  const select = page.getByLabel('Category');
  await expect(select.locator('option')).toHaveText(['Symptom', 'Treatment', 'Appointment', 'Medication', 'Other']);

  await select.selectOption('symptom');
  await page.getByLabel('Title').fill('Pain lifting overhead');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Pain lifting overhead').first()).toBeVisible();
});

// The rule that stops a re-type silently re-filing history.
test('an entry keeps its own category after its object is re-typed', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Retyped van');
  await page.getByLabel('Type').selectOption('car');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByRole('button', { name: /Log activity/ }).click();
  await page.getByLabel('Category').selectOption('fuel');
  await page.getByLabel('Title').fill('Diesel fill');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByRole('button', { name: 'Edit' }).click();
  await page.getByLabel('Type').selectOption('body');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByText('Diesel fill').first().click();
  await expect(page.getByLabel('Category')).toHaveValue('fuel');
  await expect(page.getByLabel('Category').locator('option')).toContainText(['Fuel / charge']);
});

test('a car is not offered health filters, and keeps a chip for what it actually has', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Chip test wagon');
  await page.getByLabel('Type').selectOption('car');
  await page.getByRole('button', { name: 'Save' }).click();

  const chips = page.locator('.chips button');
  // Order matters here, and both lines are load-bearing.
  //
  // The presence assertion comes FIRST because it is the one that waits: a bare
  // `toHaveCount(0)` is satisfied while the row simply has not rendered yet, so it passes
  // during navigation and never retries -- it cannot fail. Asserting a chip that must exist
  // forces the row to be there before absence means anything.
  //
  // And absence is counted, not negated: `expect(locator).not.toContainText([...])` passes even
  // when the text IS present, because the array form negates a list-wide comparison rather than
  // membership.
  await expect(chips).toContainText(['Fuel / charge']);
  await expect(chips.filter({ hasText: 'Symptom' })).toHaveCount(0);

  // An entry whose category the type no longer offers still has a chip, or its rows become
  // unreachable by filtering.
  await page.getByRole('button', { name: /Log activity/ }).click();
  await page.getByLabel('Category').selectOption('fuel');
  await page.getByLabel('Title').fill('Filter probe');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.getByRole('button', { name: 'Edit' }).click();
  await page.getByLabel('Type').selectOption('body');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('.chips button')).toContainText(['Fuel / charge']);
});
