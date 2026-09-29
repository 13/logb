// Captures every main screen at phone and desktop size, light and dark, against a LogB server
// seeded here with a small, realistic household. Used before and after each UI round so a
// change is judged on pictures of the real app, not on a reading of the diff.
//
//   rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 \
//     target/debug/logb &
//   node frontend/scripts/shots.mjs shots/before
//
// Full-page captures draw `position: fixed` elements (the bottom nav, floating buttons) where
// they were in the first viewport, part-way down the image: that is how the capture works, not
// an overlap in the app.
import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';

const BASE = process.env.SHOTS_BASE ?? 'http://127.0.0.1:8111';
const OUT = process.argv[2] ?? 'shots';
const ONLY = process.argv[3];
const USER = { username: 'ben', password: 'correct horse' };
mkdirSync(OUT, { recursive: true });

const day = (n) => { const d = new Date(); d.setDate(d.getDate() - n); return d.toISOString().slice(0, 10); };

async function seed(request) {
  const call = async (method, path, data) => {
    const r = await request.fetch(`${BASE}/api${path}`, { method, data });
    if (!r.ok()) throw new Error(`${method} ${path}: ${r.status()} ${await r.text()}`);
    const text = await r.text();
    return text ? JSON.parse(text) : null;
  };
  await call('POST', '/auth/setup', { ...USER, timezone: 'Europe/Berlin' });
  await call('POST', '/auth/login', USER);
  const car = await call('POST', '/objects', { name: 'VW Golf VIII', type: 'car', counter_unit: 'km', fuel_unit: 'l', purchase_date: '2022-04-12', purchase_price_cents: 2459000, tags: ['family', 'diesel'] });
  const bike = await call('POST', '/objects', { name: 'Cube Kathmandu Hybrid', type: 'e_bike', counter_unit: 'km', purchase_date: '2023-03-01', tags: ['commute'] });
  const house = await call('POST', '/objects', { name: 'House Lindenstraße 12', type: 'home', purchase_date: '2019-07-01', tags: ['family'] });
  const boiler = await call('POST', '/objects', { name: 'Gas boiler Vaillant ecoTEC', type: 'appliance', parent_id: house.id, counter_unit: 'h' });
  const drill = await call('POST', '/objects', { name: 'Bosch GSR 18V drill', type: 'tool', purchase_date: '2021-11-20', purchase_price_cents: 15999 });
  const entries = [
    [car, { date: day(3), category: 'fuel', title: 'Refuel Aral', counter_value: 48210, cost_cents: 8743, quantity_milli: 46500 }],
    [car, { date: day(24), category: 'fuel', title: 'Refuel Shell', counter_value: 47480, cost_cents: 8120, quantity_milli: 44100 }],
    [car, { date: day(40), category: 'maintenance', title: 'Oil change + filter', counter_value: 47100, cost_cents: 18950, notes: '5W-30, 4.7 l. Cabin filter also replaced.', tags: ['service'] }],
    [car, { date: day(75), category: 'repair', title: 'Front brake pads', counter_value: 45800, cost_cents: 31200, tags: ['service'] }],
    [car, { date: day(160), category: 'inspection', title: 'TÜV / HU', counter_value: 41020, cost_cents: 13900, notes: 'Passed without defects.' }],
    [bike, { date: day(10), category: 'maintenance', title: 'Chain + cassette replaced', counter_value: 6120, cost_cents: 8900 }],
    [bike, { date: day(95), category: 'repair', title: 'Flat tyre rear', counter_value: 5480, cost_cents: 1850 }],
    [house, { date: day(30), category: 'maintenance', title: 'Gutter cleaning', cost_cents: 24000 }],
    [boiler, { date: day(55), category: 'inspection', title: 'Annual boiler service', counter_value: 21350, cost_cents: 16900 }],
    [drill, { date: day(120), category: 'purchase', title: 'Spare 4Ah battery', cost_cents: 6999 }],
  ];
  for (const [o, e] of entries) await call('POST', `/objects/${o.id}/activities`, { ...e, client_op_id: crypto.randomUUID() });
  await call('POST', `/objects/${car.id}/reminders`, { title: 'Winter tyres on', due_date: day(6) });
  await call('POST', `/objects/${car.id}/reminders`, { title: 'Oil change', due_date: day(-150), due_counter: 62100, repeat_months: 12, repeat_counter: 15000 });
  await call('POST', `/objects/${boiler.id}/reminders`, { title: 'Chimney sweep', due_date: day(-12), repeat_months: 12 });
  return car.id;
}

const VARIANTS = [
  { name: 'mobile', viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  { name: 'desktop', viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
];

const browser = await chromium.launch();
const seeder = await browser.newContext();
const car = await seed(seeder.request);
await seeder.close();

const SCREENS = [
  ['02-login', '/login', null, false],
  ['03-dashboard', '/'],
  ['05-object-timeline', `/objects/${car}`],
  ['06-object-reminders', `/objects/${car}?tab=reminders`],
  ['07-object-info', `/objects/${car}?tab=info`],
  ['08-activity-new', `/objects/${car}/activities/new`],
  ['10-reminder-new', `/objects/${car}/reminders/new`],
  ['11-object-new', '/objects/new'],
  ['12-search', '/search', async (p) => { await p.getByRole('searchbox').first().fill('oil'); }],
  ['13-stats', '/stats'],
  ['14-settings', '/settings'],
  ['15-settings-appearance', '/settings/appearance'],
  ['16-settings-notifications', '/settings/notifications'],
];

for (const { name, ...opts } of VARIANTS) {
  for (const colorScheme of ['light', 'dark']) {
    const context = { ...opts, colorScheme, locale: 'en-US', timezoneId: 'Europe/Berlin' };
    const anon = await browser.newContext(context);
    const signed = await browser.newContext(context);
    const login = await signed.request.post(`${BASE}/api/auth/login`, { data: USER });
    if (!login.ok()) throw new Error(`login: ${login.status()}`);
    for (const [id, path, act, authed = true] of SCREENS) {
      if (ONLY && !id.includes(ONLY)) continue;
      const page = await (authed ? signed : anon).newPage();
      await page.goto(BASE + path);
      await page.waitForLoadState('networkidle');
      if (act) { await act(page); await page.waitForLoadState('networkidle'); }
      await page.waitForTimeout(500);
      await page.screenshot({ path: `${OUT}/${id}-${name}-${colorScheme}.png`, fullPage: true });
      await page.close();
    }
    await signed.close();
    await anon.close();
  }
}
await browser.close();
