import { expect, test } from '@playwright/test';
import { signInFresh } from './helpers';

/** One floating action on the object page: a plain button when only activities can be logged,
 *  a keyboard-usable menu when trips or fills can be too. */
test('"+ Log" is a button for a tool and a menu for a car', async ({ page }) => {
  await signInFresh(page, '35-log-action');
  const make = async (data: object) => {
    const r = await page.request.post('/api/objects', { data });
    expect(r.ok()).toBeTruthy();
    return (await r.json()).id as number;
  };
  const drill = await make({ name: 'LogAction drill', type: 'tool' });
  const car = await make({ name: 'LogAction car', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  // One entry each, so the timeline is not empty (an empty one shows its own buttons instead).
  for (const [id, extra] of [[drill, {}], [car, { counter_value: 100 }]] as const) {
    const r = await page.request.post(`/api/objects/${id}/activities`, { data: { date: '2026-01-10', category: 'other', title: 'First', ...extra } });
    expect(r.ok()).toBeTruthy();
  }

  await page.goto(`/objects/${drill}`);
  await expect(page.getByTestId('log-menu')).toHaveCount(0);
  await page.getByRole('button', { name: /Log activity/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${drill}/activities/new$`));

  await page.goto(`/objects/${car}`);
  const menu = page.getByTestId('log-menu');
  await menu.focus();
  await page.keyboard.press('Enter');
  const items = page.getByRole('menuitem');
  await expect(items).toHaveText([/Log activity/, /Log fill/, /Log trip/]);
  await page.keyboard.press('Escape');
  await expect(items).toHaveCount(0);
  await expect(menu).toBeFocused();
  await menu.click();
  await page.getByRole('menuitem', { name: /Log trip/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${car}/activities/new\\?category=trip`));
});
