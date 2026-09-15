import { test, expect } from '@playwright/test';
import { signInFresh } from './helpers';

// `/api/types` is matched by the service worker's `householdData` route (NetworkFirst), and a
// request it handles never reaches `page.route` -- this test delays that request to check the
// entry form's own re-render, not the service worker, so it runs with the worker blocked to get
// the delayed response back under `page.route`'s control.
test.use({ serviceWorkers: 'block' });

test('an own type is offered, drawn and counted everywhere a built-in one is', async ({ page }) => {
  await signInFresh(page, '24-own-types');

  // Settings → Types → add one.
  await page.goto('/settings');
  await page.getByRole('button', { name: /^Types/ }).click();
  await page.waitForURL('**/settings/types');
  await page.getByRole('button', { name: 'Add type' }).click();
  await page.getByLabel('Name', { exact: true }).fill('E-Scooter');
  // The radio itself is visually hidden inside its icon cell; a person taps the cell.
  const eBike = page.getByRole('radio', { name: 'E-bike' });
  await page.locator('.icon-choice').filter({ has: eBike }).click();
  await expect(eBike).toBeChecked();
  // The add form starts with a sensible default set; this type wants exactly Repair and Fuel.
  for (const c of ['Maintenance', 'Inspection', 'Purchase']) await page.getByLabel(c, { exact: true }).uncheck();
  await page.getByLabel('Fuel / charge', { exact: true }).check();
  await expect(page.getByLabel('Repair', { exact: true })).toBeChecked();
  await page.getByLabel('Default counter unit').selectOption('km');
  await page.getByRole('button', { name: 'Save type' }).click();
  const typeRow = page.locator('.type-row', { hasText: 'E-Scooter' });
  await expect(typeRow).toBeVisible();

  // New object: the type sits under "Your types" and brings its counter unit along.
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Scooter One');
  await expect(page.locator('#c optgroup[label="Your types"] option', { hasText: 'E-Scooter' })).toHaveCount(1);
  // exact: the "Your types" optgroup is labelled too.
  await page.getByLabel('Type', { exact: true }).selectOption({ label: 'E-Scooter' });
  await expect(page.getByLabel('Counter', { exact: true })).toHaveValue('km');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);
  const objectId = Number(new URL(page.url()).pathname.split('/').pop());

  // The card names the type, never its key.
  await page.goto('/');
  // Anchored: a card's accessible name also carries its nested labels.
  const card = page.getByRole('button', { name: /^Scooter One/ });
  await expect(card).toContainText('E-Scooter');
  await expect(page.getByText(/custom:/)).toHaveCount(0);

  // A new entry offers exactly the type's categories.
  await page.goto(`/objects/${objectId}/activities/new`);
  await expect(page.getByLabel('Category').locator('option')).toHaveText(['Repair', 'Fuel / charge', 'Other']);
  await page.getByLabel('Category').selectOption({ label: 'Repair' });
  await page.getByLabel('Title').fill('New brake pads');
  await page.getByLabel('Cost').fill('120');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await page.waitForURL(`**/objects/${objectId}`);
  await expect(page.getByText('New brake pads')).toBeVisible();

  // A cold load of the entry form whose own types arrive late: the default category is re-chosen
  // from the type's own list once it lands, rather than sitting on one the type does not offer.
  // The stored list is removed first, or it would answer before the network.
  await page.evaluate(() => { for (const k of Object.keys(localStorage)) if (k.startsWith('logb.types.')) localStorage.removeItem(k); });
  await page.context().route('**/api/types', async (route) => {
    await new Promise((r) => setTimeout(r, 1500));
    await route.continue();
  });
  await page.goto(`/objects/${objectId}/activities/new`);
  await page.reload();
  const category = page.getByLabel('Category');
  await expect(category.locator('option')).toHaveText(['Repair', 'Fuel / charge', 'Other']);
  expect(['repair', 'fuel', 'other']).toContain(await category.inputValue());
  await page.context().unroute('**/api/types');

  // Search hits name the type too.
  await page.goto('/search?q=Scooter%20One');
  await expect(page.getByRole('main')).toContainText('E-Scooter');
  await expect(page.getByText(/custom:/)).toHaveCount(0);

  // Statistics: the "By type" row is the type's name.
  await page.goto('/stats');
  const byType = page.getByTestId('stats-by-type');
  await expect(byType.locator('.bar-row', { hasText: 'E-Scooter' })).toContainText('€120.00');
  await expect(byType).not.toContainText('custom:');

  // Deleting a type still in use is refused, with the count, and the type stays.
  await page.goto('/settings/types');
  page.once('dialog', (d) => d.accept());
  await typeRow.getByRole('button', { name: 'Delete E-Scooter' }).click();
  await expect(typeRow.getByRole('alert')).toHaveText(/Used by 1 object/);
  await page.reload();
  await expect(page.locator('.type-row', { hasText: 'E-Scooter' })).toBeVisible();
});

test('"+ New type…" on the object form makes a type without losing what was typed', async ({ page }) => {
  await signInFresh(page, '24-own-types-shortcut');

  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Mein Pedelec');
  await page.getByLabel('Type', { exact: true }).selectOption({ label: '+ New type…' });

  // Lands on Types with the add form already open, and the object form's own path plus a
  // one-time draft token carried as `return`/`draft` -- not chosen, just opened for the
  // roundtrip.
  await page.waitForURL(/\/settings\/types\?new=1&return=%2Fobjects%2Fnew&draft=/);
  await expect(page.getByRole('button', { name: 'Save type' })).toBeVisible();
  await page.getByLabel('Name', { exact: true }).fill('Pedelec');
  await page.getByLabel('Default counter unit').selectOption('km');
  await page.getByRole('button', { name: 'Save type' }).click();

  // Back on the object form, its address clean again (the `?type=`/`draft=` round trip is
  // consumed on mount, not left sitting in the URL for a reload to re-apply): the name typed
  // before the detour survived, and the new type is selected, unit and all.
  await expect(page).toHaveURL(/\/objects\/new$/);
  await expect(page.getByLabel('Name')).toHaveValue('Mein Pedelec');
  await expect(page.getByLabel('Type', { exact: true })).toHaveValue(/^custom:/);
  await expect(page.locator('#c option:checked')).toHaveText('Pedelec');
  await expect(page.getByLabel('Counter', { exact: true })).toHaveValue('km');

  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);
  // The object's own name also contains "Pedelec", so the type itself is checked on the Info
  // tab's own type line rather than by a page-wide text search that a coincidence like that
  // could pass on its own.
  await page.getByRole('button', { name: 'Info' }).click();
  await expect(page.locator('main .muted').first()).toHaveText('Pedelec');
});

test('Cancel on Types, reached from the shortcut, returns without changing the type', async ({ page }) => {
  await signInFresh(page, '24-own-types-shortcut-cancel');

  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Mein Auto');
  // A type picked before the detour must still be there after Cancel -- otherwise nothing tells
  // apart "the shortcut changed nothing" from "the shortcut happened to leave the default in
  // place".
  await page.getByLabel('Type', { exact: true }).selectOption({ label: 'Car' });
  await page.getByLabel('Type', { exact: true }).selectOption({ label: '+ New type…' });

  await page.waitForURL(/\/settings\/types\?new=1&return=%2Fobjects%2Fnew&draft=/);
  await expect(page.getByRole('button', { name: 'Save type' })).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();

  await expect(page).toHaveURL(/\/objects\/new$/);
  await expect(page.getByLabel('Name')).toHaveValue('Mein Auto');
  await expect(page.getByLabel('Type', { exact: true })).toHaveValue('car');
});

test('the shortcut also works from the edit form, and clears type/draft from the URL', async ({ page }) => {
  await signInFresh(page, '24-own-types-edit-shortcut');

  // An existing object to edit.
  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Old Name');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(/\/objects\/\d+$/);
  const objectId = Number(new URL(page.url()).pathname.split('/').pop());

  await page.goto(`/objects/${objectId}/edit`);
  await page.getByLabel('Name').fill('New Name');
  await page.getByLabel('Type', { exact: true }).selectOption({ label: '+ New type…' });
  await page.waitForURL(new RegExp(`/settings/types\\?new=1&return=%2Fobjects%2F${objectId}%2Fedit&draft=`));
  await page.getByLabel('Name', { exact: true }).fill('Widget');
  await page.getByLabel('Default counter unit').selectOption('mi');
  await page.getByRole('button', { name: 'Save type' }).click();

  // Back on the edit form at its own clean URL: the renamed name survived the detour, the new
  // type is selected, and nothing from the roundtrip (`type=`, `draft=`) is left in the address.
  await expect(page).toHaveURL(new RegExp(`/objects/${objectId}/edit$`));
  expect(page.url()).not.toMatch(/[?&](type|draft)=/);
  await expect(page.getByLabel('Name')).toHaveValue('New Name');
  await expect(page.locator('#c option:checked')).toHaveText('Widget');
  await expect(page.getByLabel('Counter', { exact: true })).toHaveValue('mi');

  await page.getByRole('button', { name: 'Save' }).click();
  await page.waitForURL(`**/objects/${objectId}`);
  await expect(page.getByRole('main')).toContainText('New Name');
  // The type line lives under the Info tab, not the default Timeline one.
  await page.getByRole('button', { name: 'Info' }).click();
  await expect(page.locator('main .muted').first()).toHaveText('Widget');
});

test('abandoning the shortcut leaves a later, plain visit to the object form empty', async ({ page }) => {
  await signInFresh(page, '24-own-types-abandon');

  await page.goto('/objects/new');
  await page.getByLabel('Name').fill('Abandoned Draft');
  // The detour is started (a draft is kept and a token minted) but never finished -- no create,
  // no Cancel click, just leaving Types the way someone closing the tab or typing a new address
  // would.
  await page.getByLabel('Type', { exact: true }).selectOption({ label: '+ New type…' });
  await page.waitForURL(/\/settings\/types\?new=1&return=%2Fobjects%2Fnew&draft=/);

  // A plain, direct visit -- no `draft=` token in its address -- must not resurrect that
  // abandoned input: the form comes up exactly as empty as a fresh one.
  await page.goto('/objects/new');
  await expect(page.getByLabel('Name')).toHaveValue('');
  await expect(page.getByLabel('Type', { exact: true })).toHaveValue('other');
});
