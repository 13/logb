import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** Pinned so every entry logged through the form (its date defaults to "today") lands on the
 *  same day, and the maths in this spec's own comments below don't depend on when it runs --
 *  mirrors `FIXED_NOW` in `28-trips.spec.ts`. */
const FIXED_NOW = new Date('2026-09-16T12:00:00');

/** Mirrors the same helper in `28-trips.spec.ts`: seed the object through the API so the test
 *  itself only drives the charge/trip forms and the timeline and Info tab they produce. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

test('logging charges and trips, and the Energy section they produce', async ({ page }) => {
  await page.clock.setFixedTime(FIXED_NOW);
  await signInFresh(page, '29-charging');

  // e-bike, kWh, 0.30 EUR/kWh (30 cents x1000).
  const bike = await object(page, { name: 'Charging E-Bike', type: 'e_bike', counter_unit: 'km', fuel_unit: 'kwh', energy_price_milli: 30_000 });

  await page.goto(`/objects/${bike}`);

  // A brand-new object's timeline is empty, so this is the empty-state ghost button, not the
  // FAB row (which only appears once there is at least one entry) -- exercising both is exactly
  // why `onchargelog` exists on Timeline.svelte, not only the FAB in ObjectDetail.
  await page.getByRole('button', { name: /Log charge/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}/activities/new\\?category=fuel`));

  // A full charge at 1000 km, no amount and no cost -- it still marks distance, which is all
  // the first window needs. "Charged full" is ticked by default, so it is left alone.
  await expect(page.getByLabel('Charged full')).toBeChecked();
  await page.getByLabel('Title').fill('Charge');
  await page.getByLabel(/^Counter/).fill('1000');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}$`));

  const firstCharge = page.locator('.card.entry', { hasText: 'Charge' });
  await expect(firstCharge).toContainText('1,000 km');
  await expect(firstCharge).toContainText('full');

  // A trip 1000 -> 1200 using 40 % of the battery. Start prefills from the object's own current
  // counter, exactly as in 28-trips.spec.ts.
  await page.getByRole('button', { name: /Log trip/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}/activities/new\\?category=trip`));
  await expect(page.getByLabel(/^Start/)).toHaveValue('1000');
  await page.getByLabel(/^Distance/).fill('200');
  await page.getByLabel(/Battery used/).fill('40');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}$`));

  // Only one full charge exists yet, so the distance/cost figures below stay hidden (fewer than
  // two charges in total -- no window can form at all) -- but the battery line already shows,
  // since it needs only a full charge and a trip carrying a battery percentage, neither of which
  // depends on a second charge. used = 40, remaining = 60; km_per_pct = 200/40 = 5, so
  // range_left = 60 * 5 = 300.
  await page.goto(`/objects/${bike}?tab=info`);
  await expect(page.getByRole('heading', { name: 'Energy', exact: true })).toBeVisible();
  const battery = page.getByTestId('energy-battery');
  await expect(battery).toContainText('Charge due');
  await expect(battery).toContainText('60 %');
  await expect(battery).toContainText('300 km');
  await expect(battery).not.toContainText('charge soon');
  await expect(page.getByText(/Distance per charge/)).toHaveCount(0);

  // A second full charge at 1400 km, 8 kWh, €2.40 -- now two full charges exist, so the window
  // 1000 -> 1400 (400 km) forms, and every rate is drawn from this charge's own amount/cost.
  await page.goto(`/objects/${bike}`);
  await page.getByRole('button', { name: /Log charge/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}/activities/new\\?category=fuel`));
  await expect(page.getByLabel('Charged full')).toBeChecked();
  await page.getByLabel('Title').fill('Charge');
  // The counter prefills from the object's current counter (1200, after the trip); overwritten
  // to 1400 for the second charge.
  await expect(page.getByLabel(/^Counter/)).toHaveValue('1200');
  await page.getByLabel(/^Counter/).fill('1400');
  await page.getByLabel(/^Amount/).fill('8');
  await page.getByLabel('Cost').fill('2.40');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}$`));

  // Newest first: the just-saved second charge is now the first match, and the earlier one has
  // shifted to the second.
  const secondCharge = page.locator('.card.entry', { hasText: 'Charge' }).first();
  await expect(secondCharge).toContainText('1,400 km');
  await expect(secondCharge).toContainText('full');
  // "kWh", not the stored lowercase "kwh" -- fuelUnitLabel.
  await expect(secondCharge).toContainText('8 kWh');
  await expect(secondCharge).toContainText('€2.40');

  // The trip's row now shows an estimated cost: cost_per_counter_milli comes from the closing
  // (1400 km) charge's own cost -- 240 cents x1000 / 400 km = 600 (cents x1000 per km) -- so the
  // 200 km trip costs 200 * 600 = 120,000 (cents x1000) -> €1.20, always prefixed "≈" since it is
  // never stored.
  const trip = page.locator('.card.entry', { hasText: /1,000 km → 1,200 km/ });
  await expect(trip).toContainText('≈ €1.20');

  // The Info tab's Energy section: distance per charge (400 km, the one surviving window),
  // distance per unit (400 km / 8 kWh = 50 km/kWh, via `distance_per_unit_milli` = 50_000
  // milli -> formatPerUnit divides back down and appends the reader-facing "kWh"), and energy
  // cost per distance (the same 600 milli-cents/km as above, rendered as money -- €0.01).
  await page.goto(`/objects/${bike}?tab=info`);
  await expect(page.getByText('Distance per charge: 400 km')).toBeVisible();
  await expect(page.getByText('Distance per unit: 50 km/kWh')).toBeVisible();
  await expect(page.getByText('Energy cost per distance: €0.01')).toBeVisible();

  // A further trip after the second (now latest) full charge, using 85 % of the battery: past
  // the "charge soon" threshold (remaining <= 20). remaining = 100 - 85 = 15; km_per_pct is
  // measured over every trip carrying a battery figure, not only this one: (200 + 50) / (40 +
  // 85) = 250 / 125 = 2, so range_left = 15 * 2 = 30.
  await page.goto(`/objects/${bike}`);
  await page.getByRole('button', { name: /Log trip/ }).click();
  await expect(page.getByLabel(/^Start/)).toHaveValue('1400');
  await page.getByLabel(/^Distance/).fill('50');
  await page.getByLabel(/Battery used/).fill('85');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${bike}$`));

  await page.goto(`/objects/${bike}?tab=info`);
  const batteryAfter = page.getByTestId('energy-battery');
  await expect(batteryAfter).toContainText('Charge due');
  await expect(batteryAfter).toContainText('15 %');
  await expect(batteryAfter).toContainText('30 km');
  await expect(batteryAfter).toContainText('charge soon');
});
