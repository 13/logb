import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('a house shows its rooms, and a room shows its breadcrumb', async ({ page }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Hierarchy House');
  await page.getByLabel('Type').selectOption('home');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.goto('/');
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Hierarchy Garage');
  await page.getByLabel('Type').selectOption('other');
  await page.getByLabel('Inside').selectOption({ label: 'Hierarchy House' });
  await page.getByRole('button', { name: 'Save' }).click();

  // The garage's own page shows the breadcrumb back to the house.
  await expect(page.getByText('Hierarchy House')).toBeVisible();

  // The house's page lists the garage in its contents.
  await page.goto('/');
  await page.getByText('Hierarchy House').click();
  await page.getByRole('button', { name: 'Info' }).click();
  await expect(page.getByText('Hierarchy Garage')).toBeVisible();
});

test('an archived object deep inside the tree is still reachable from the archived view', async ({ page }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Attic Nest House');
  await page.getByLabel('Type').selectOption('home');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.goto('/');
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Attic Nest Garage');
  await page.getByLabel('Inside').selectOption({ label: 'Attic Nest House' });
  await page.getByRole('button', { name: 'Save' }).click();

  await page.goto('/');
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Attic Nest Bulb');
  // The picker says which object each candidate sits inside, so two garages cannot be
  // confused: the option is the name plus its parent's, the same idiom the search hits use.
  await expect(page.getByLabel('Inside').locator('option', { hasText: 'Attic Nest Garage' }))
    .toHaveText('Attic Nest Garage · in Attic Nest House');
  await page.getByLabel('Inside').selectOption({ label: 'Attic Nest Garage · in Attic Nest House' });
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);

  // Archive the bulb: two levels down, inside a garage, inside a house.
  await page.getByRole('button', { name: 'Edit' }).first().click();
  await page.getByLabel('Archive').check();
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);

  // It is gone from the live dashboard, which lists top-level objects only...
  await page.goto('/');
  await expect(page.getByText('Attic Nest House')).toBeVisible();
  await expect(page.getByText('Attic Nest Bulb')).toHaveCount(0);

  // ...and present in the archived view, which is the recovery route for everything archived
  // at any depth. Before this it asked for archived *roots* only, and a nested archived object
  // appeared in no list in the app at all.
  await page.getByRole('button', { name: 'Show archived' }).click();
  await expect(page.getByText('Attic Nest Bulb')).toBeVisible();
});
