import { expect, test } from '@playwright/test';
import { pngPayload, signIn } from './helpers';

test('an activity logged offline is replayed to the server exactly once', async ({ page, context, browser }) => {
  await signIn(page);

  // create the object
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Trailer');
  await page.getByLabel('Category').fill('trailer');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Trailer' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];
  expect(objectId).toBeTruthy();

  await page.getByRole('button', { name: /Log/ }).first().click();

  await context.setOffline(true);
  await page.getByLabel('Title').fill('Fuel');
  await page.getByRole('button', { name: 'Save' }).click();
  // Visible immediately from the local outbox queue -- before the write has ever reached the
  // server. This alone proves nothing about replay: a build with flushOutbox removed entirely
  // would still show this.
  await expect(page.getByText('Fuel', { exact: true })).toBeVisible();

  await context.setOffline(false);
  // The reconnect fires the outbox's 'online' listener; give the flush a moment to land, then
  // the UI should still show exactly one entry (merging a queued op with its own replayed row
  // would be a duplicate-visible-entry bug).
  await expect(page.getByText('Fuel', { exact: true })).toHaveCount(1);

  // Server truth: ask the backend directly, through a path no local/queued state can satisfy.
  // `page.request` shares this browser context's cookies, so it is genuinely authenticated,
  // but it never touches the page's own IndexedDB or in-memory state -- if flushOutbox were
  // deleted, or if replay duplicated the row, this is what would catch it.
  await expect(async () => {
    const res = await page.request.get(`/api/objects/${objectId}/activities`);
    expect(res.ok()).toBe(true);
    const activities = (await res.json()) as Array<{ title: string }>;
    expect(activities.filter((a) => a.title === 'Fuel')).toHaveLength(1);
  }).toPass();

  // Belt and braces: a second browser context has its own empty IndexedDB and cookie jar, so
  // anything it shows for this object came only from the server, not from replaying local
  // outbox state a second time.
  const otherContext = await browser.newContext();
  const otherPage = await otherContext.newPage();
  try {
    await signIn(otherPage);
    await otherPage.goto(`/objects/${objectId}`);
    await expect(otherPage.getByText('Fuel', { exact: true })).toHaveCount(1);
  } finally {
    await otherContext.close();
  }
});

test('an activity logged offline with an attachment replays both exactly once', async ({ page, context, browser }) => {
  await signIn(page);

  // create the object
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Generator');
  await page.getByLabel('Category').fill('generator');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Generator' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];
  expect(objectId).toBeTruthy();

  await page.getByRole('button', { name: /Log/ }).first().click();

  await context.setOffline(true);
  await page.getByLabel('Title').fill('Fuel');

  // A file needs a parent activity row to hang on. Offline, `ensureSaved()` cannot get one
  // from the server -- there is no server to ask -- so it mints a temp id and queues the
  // create itself (see ActivityForm.svelte). This first click drives that: the button visible
  // before that resolves is the form's own placeholder ("+ Add photos or files"); once `saved`
  // is set, the same-labelled button belongs to FilePicker instead, which is what actually
  // opens the file input `setInputFiles` below drives.
  await page.getByRole('button', { name: /Add photos or files/ }).click();
  await page.setInputFiles('input[type=file]', pngPayload());
  // Visible immediately even though the upload never reached the server: a photo that vanishes
  // because there was no signal at the fuel pump is the exact failure this feature exists to
  // prevent. Dimmed-plus-a-chip is the same "still queued" treatment Timeline gives a pending
  // activity.
  await expect(page.locator('.thumb-strip .strip-item.pending')).toHaveCount(1);

  await page.getByRole('button', { name: 'Save' }).click();
  // Visible immediately from the local outbox queue -- before either write has ever reached
  // the server. This alone proves nothing about replay: a build with flushOutbox removed
  // entirely would still show this.
  await expect(page.getByText('Fuel', { exact: true })).toBeVisible();

  await context.setOffline(false);
  // The reconnect fires the outbox's 'online' listener; give the flush a moment to land, then
  // the UI should still show exactly one entry (merging a queued op with its own replayed row
  // would be a duplicate-visible-entry bug).
  await expect(page.getByText('Fuel', { exact: true })).toHaveCount(1);

  // Server truth: ask the backend directly, through a path no local/queued state can satisfy.
  // `page.request` shares this browser context's cookies, so it is genuinely authenticated,
  // but it never touches the page's own IndexedDB or in-memory state -- if the multipart
  // replay path were broken, or the temp id were never rewritten to the real one, this is what
  // would catch it: either no attachment at all, or one still pointing at a temp activity id
  // the server never issued and so 404'd (and was parked dead) instead of landing.
  await expect(async () => {
    const res = await page.request.get(`/api/objects/${objectId}/activities`);
    expect(res.ok()).toBe(true);
    const activities = (await res.json()) as Array<{ title: string; attachments: unknown[] }>;
    const fuel = activities.filter((a) => a.title === 'Fuel');
    expect(fuel).toHaveLength(1);
    expect(fuel[0].attachments).toHaveLength(1);
  }).toPass();

  // Belt and braces: a second browser context has its own empty IndexedDB and cookie jar, so
  // anything it shows for this object came only from the server, not from replaying local
  // outbox state a second time.
  const otherContext = await browser.newContext();
  const otherPage = await otherContext.newPage();
  try {
    await signIn(otherPage);
    await otherPage.goto(`/objects/${objectId}`);
    await expect(otherPage.getByText('Fuel', { exact: true })).toHaveCount(1);
    await expect(otherPage.locator('.thumb-strip img')).toHaveCount(1);
  } finally {
    await otherContext.close();
  }
});

/**
 * Editing an existing activity while offline. ActivityForm's onMount loads the row being
 * edited; if that GET fails, `saved` stays null and the form renders with empty defaults even
 * though it is on an edit URL. Save then took the "no row yet" branch and queued a CREATE, so
 * an offline edit silently became a second activity -- with no error shown, and the change the
 * user actually made nowhere.
 */
test('an activity edited offline never turns into a second activity', async ({ page, context }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Van');
  await page.getByLabel('Category').fill('vehicle');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Van' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];

  // One real activity, created online.
  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Service');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Service', { exact: true })).toBeVisible();

  // Open its edit form with no network at all, so the row's own GET fails. The navigation is
  // an in-app one (a click on the timeline entry), not a page load: the point is a failing
  // onMount, not a browser that cannot fetch the app shell.
  await context.setOffline(true);
  await page.getByText('Service', { exact: true }).click();
  // The form comes up blank because the row never loaded. The user types the edit they came to
  // make -- without a title the form's own validation would reject the save before the bug
  // could be reached, so an empty form proves nothing here.
  await page.getByLabel('Title').fill('Service (rescheduled)');
  await page.getByRole('button', { name: 'Save' }).click();

  // Refused, and said so, instead of navigating away as though the edit had been saved. This
  // is asserted while still OFFLINE, so it pins the moment the create would have been queued
  // rather than a server state that only diverges later -- `toPass` on the server count would
  // pass on its first poll, before the replay it is meant to catch has even happened.
  await expect(page.getByText(/could not be loaded/i)).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`/objects/${objectId}/activities/\\d+$`));

  await context.setOffline(false);
  // Nothing was queued, so nothing replays: the server still holds exactly the one activity.
  // Given a moment for a flush to have happened if one had been queued.
  await page.waitForTimeout(1000);
  const after = await page.request.get(`/api/objects/${objectId}/activities`);
  expect((await after.json()) as unknown[]).toHaveLength(1);
});

/**
 * The session lapsing is not the same as being offline: the request reaches the server and is
 * refused. A 401 is still a refusal the user can undo by logging back in, so the write is
 * queued rather than discarded -- and logging in flushes the queue, which none of the outbox's
 * own triggers (load, `online`, `visibilitychange`) do on an SPA login.
 */
test('a write made with an expired session is kept and sent after logging back in', async ({ page, context }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Mower');
  await page.getByLabel('Category').fill('garden');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Mower' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];

  await page.getByRole('button', { name: /Log/ }).first().click();
  // Let the form finish loading before the session is pulled out from under it: its onMount
  // fetches the object and then the title suggestions, and a 401 on one of those would send the
  // user to /login before they ever pressed Save -- a different (and already covered) path.
  await page.waitForLoadState('networkidle');
  await page.getByLabel('Title').fill('Blade sharpening');

  // The session ends between opening the form and saving it -- an expiry, or a password change
  // elsewhere, which ends every other session. The network is fine throughout.
  await context.clearCookies();
  await page.getByRole('button', { name: 'Save' }).click();

  // The unauthorized handler sends the user to the login screen; the entry must not have gone
  // with it.
  await expect(page).toHaveURL(/\/login/);
  await signIn(page);

  await expect(async () => {
    const res = await page.request.get(`/api/objects/${objectId}/activities`);
    expect(res.ok()).toBe(true);
    const activities = (await res.json()) as Array<{ title: string }>;
    expect(activities.filter((a) => a.title === 'Blade sharpening')).toHaveLength(1);
  }).toPass();
});

/**
 * Two accounts, one device. The outbox is a single IndexedDB per origin and a queued write
 * deliberately outlives the session that made it, so the second person to log in must neither
 * send, see, nor destroy the first person's queued write -- it has to still be there, intact,
 * when they come back.
 */
test('a queued write survives somebody else signing in on the same device', async ({ page, context }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Tractor');
  await page.getByLabel('Category').fill('vehicle');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Tractor' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];

  const other = { username: 'zoe', password: 'another correct horse' };
  const created = await page.request.post('/api/users', {
    data: { username: other.username, password: other.password, is_admin: false },
  });
  expect(created.ok()).toBe(true);

  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.waitForLoadState('networkidle');
  await page.getByLabel('Title').fill('Hydraulic oil');

  // The admin's session ends with the entry unsaved, so the write is queued under their id.
  await context.clearCookies();
  // `dispatchEvent` rather than `click`, because this click destroys the thing being clicked:
  // the save 401s, the unauthorized handler navigates to /login, and the button is detached
  // mid-action. `click` re-checks that the element is attached and stable, sees it vanish,
  // retries against a page that no longer has the button, and reports a 30s timeout for what
  // was actually a success. That race is rare -- it failed once in CI and once locally out of
  // dozens of runs -- and it is entirely an artefact of the assertion, not of the app.
  await page.getByRole('button', { name: 'Save' }).dispatchEvent('click');
  await expect(page).toHaveURL(/\/login/);

  // Somebody else signs in on the same device. Their login flushes the outbox -- which must
  // walk straight past a write that is not theirs rather than sending it, having it refused on
  // ownership, and parking it dead.
  await page.getByLabel(/Username|Benutzername/).fill(other.username);
  await page.getByLabel(/Password|Passwort/).fill(other.password);
  await page.getByRole('button', { name: /^Sign in$|^Anmelden$/ }).click();
  await page.waitForURL('**/');
  // Not theirs to see, either: no pending or failed write is surfaced to them.
  await expect(page.getByText(/Waiting to send|Failed/i)).toHaveCount(0);
  await page.waitForTimeout(500);

  // And a reload while they are signed in, which is the flush `main.ts` fires at module load --
  // before anything knows who is signed in. That one used to treat an unknown user as "anyone",
  // so it replayed the admin's write under this session's perfectly valid cookie, had it
  // refused on ownership, and parked it dead.
  await page.reload();
  await page.waitForLoadState('networkidle');
  await page.waitForTimeout(500);

  // The owner comes back, and their entry is still there to send.
  await page.getByRole('button', { name: 'Settings' }).click();
  // Exact: the account section also offers "Sign out everywhere".
  await page.getByRole('button', { name: 'Sign out', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await signIn(page);

  await expect(async () => {
    const res = await page.request.get(`/api/objects/${objectId}/activities`);
    expect(res.ok()).toBe(true);
    const activities = (await res.json()) as Array<{ title: string }>;
    expect(activities.filter((a) => a.title === 'Hydraulic oil')).toHaveLength(1);
  }).toPass();
});

/**
 * A boot with no connection. `/api/auth/*` is NetworkOnly, so the very first request of
 * `loadSession` fails and the app never learns who is signed in -- and the outbox refuses to
 * send while it has no user, since it cannot tell whose ops these are. That state used to be
 * permanent for the life of the page: reconnecting fired a flush that returned immediately, and
 * the queued write sat there with a perfectly valid cookie until a manual reload.
 */
test('a write queued before an offline boot still sends when the connection returns', async ({ page, context }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Chainsaw');
  await page.getByLabel('Category').fill('tool');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('heading', { name: 'Chainsaw' })).toBeVisible();
  const objectId = page.url().match(/\/objects\/(\d+)/)?.[1];

  // The reload below has to be served by the service worker, so wait for it to take control.
  await page.evaluate(() => navigator.serviceWorker.ready.then(() => undefined));

  await context.setOffline(true);
  await page.getByRole('button', { name: /Log/ }).first().click();
  await page.getByLabel('Title').fill('Chain oil');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Chain oil', { exact: true })).toBeVisible();

  // Restart the app with no connection: the shell comes from the precache, but every /api/auth
  // request fails, so this boot ends with the session unknown rather than known-anonymous.
  await page.reload();
  await page.waitForTimeout(500);

  await context.setOffline(false);
  // Coming back to the tab, which is how a user finds out they have a connection again -- and
  // the trigger that has to work, since a captive portal or a 502 never fires `online` at all.
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    document.dispatchEvent(new Event('visibilitychange', { bubbles: true }));
  });

  await expect(async () => {
    const res = await page.request.get(`/api/objects/${objectId}/activities`);
    expect(res.ok()).toBe(true);
    const activities = (await res.json()) as Array<{ title: string }>;
    expect(activities.filter((a) => a.title === 'Chain oil')).toHaveLength(1);
  }).toPass();
});
