import { get, readonly, writable, type Readable } from 'svelte/store';
import { tick } from 'svelte';
import { api, ApiError, clearServingSaved, flushOutbox, isRejection, persistStorage, setOutboxSendGate, setOutboxUser, setUnauthorizedHandler } from '../lib/api';
import { cachesBelongTo, forgetCacheOwner, forgetProfile, recordCacheOwner, rememberedProfile, rememberProfile, userSwitchNeedsReload } from '../lib/cache-owner';
import { clearObjectCache, clearObjectMemory } from '../lib/object-cache';
import { clearCustomTypes, clearStoredTypeLists, loadCustomTypes } from '../lib/type-registry';
import type { Settings, User } from '../lib/types';
import { go } from '../lib/router';

/**
 * How this module navigates. Injected the same way `setUnauthorizedHandler` is, so the session
 * state machine -- which now decides quite a lot: known versus merely unreachable, when the
 * outbox may send, when to retry -- can be tested without a DOM. It was previously untestable
 * only because of this one import, and four review findings landed in it.
 */
let navigate: (path: string, replace?: boolean) => void = go;
export function setNavigateForTesting(fn: (path: string, replace?: boolean) => void): void {
  navigate = fn;
}

/**
 * What `logout`/`logoutEverywhere`'s callers (`SignedIn.svelte`, `settings/Account.svelte`) show
 * when the call rejects. An `ApiError` is the server answering and refusing -- its message is
 * already legible, straight from the response body (see `handle()` in ../lib/api.ts). Anything
 * else -- a `TypeError` from a dropped connection, an aborted request, a malformed response --
 * never reached the server at all, and the honest thing to say is that signing out needs a
 * connection, not whatever the browser happened to throw.
 *
 * Takes the translate function so it always returns the finished, displayable string -- not
 * sometimes an i18n key and sometimes a plain server message, leaving it to the caller to run the
 * result through `$t()` either way (which happened to work only because an unrecognised key
 * renders unchanged, see ../i18n/index.ts). Callers pass `$t`, the current value of the `t` store.
 */
export function signOutErrorMessage(e: unknown, t: (key: string, vars?: Record<string, string | number>) => string): string {
  return e instanceof ApiError ? e.message : t('nav.signout-offline');
}

/** undefined = not loaded yet, null = anonymous */
export const user = writable<User | null | undefined>(undefined);
export const setupRequired = writable<boolean>(false);
export const currency = writable<string>('EUR');
/**
 * True while the app runs as the last user remembered on this device because the session check
 * could not reach the server. `user` is then that remembered profile, not a confirmed session:
 * `sessionKnown` stays false, so the retry listeners keep asking and the outbox keeps holding
 * its writes until a real answer arrives.
 *
 * Read-only outside this module: only the session check may decide it, and a component setting
 * it would show the note (or hide it) without anything about the session having changed.
 */
const offlineState = writable<boolean>(false);
export const offline: Readable<boolean> = readonly(offlineState);

/**
 * Everything a session leaves behind on this device, dropped. Login and logout are SPA
 * navigations (no page reload), so module-scope caches like ObjectDetail's are never cleared on
 * their own between users on a shared device -- every path that ends a session must drop them
 * itself (see the invariant on `clearObjectCache`). The profile and cache owner go too: the next
 * start must neither open offline as this person nor treat the caches as theirs.
 *
 * The queue is NOT cleared: an expired session is exactly when a write must survive until the
 * user signs back in. It is only detached from the current session, so nothing replays or
 * displays it until someone claims it by logging in (see `setOutboxUser` in ../lib/api).
 */
function endSession(): void {
  user.set(null);
  sessionKnown = true;
  offlineState.set(false);
  stopOfflineRetry();
  // A note earned by the ending session's own slow requests must not linger over a signed-out
  // (or about-to-be-someone-else's) screen.
  clearServingSaved();
  // Not awaited: nobody is signed in afterwards, so nothing loads that the old caches could
  // answer, and the next session start claims (and, being ownerless, clears) them again anyway.
  void clearObjectCache();
  clearCustomTypes();
  setOutboxUser(null);
  forgetProfile();
  forgetCacheOwner();
}

setUnauthorizedHandler(() => {
  endSession();
  if (globalThis.location?.pathname !== '/login') navigate('/login', true);
});

/**
 * Whether the app has actually learned who is signed in, as opposed to having learned that
 * nobody is. A boot with no connection cannot tell the two apart -- `/api/auth/*` is
 * NetworkOnly in the service worker, so the very first request rejects -- and the outbox
 * refuses to send while it has no user, by design. Without a retry that state was permanent
 * for the life of the page: reconnecting fired a flush that returned immediately, and the
 * queued writes sat there, with a perfectly valid cookie, until a manual reload.
 *
 * Offline mode never sets this: a remembered profile is not a server's answer.
 */
let sessionKnown = false;
// Offline mode gives the outbox a user without a confirmed session; this is what keeps it from
// sending under that user until the real check has succeeded.
setOutboxSendGate(() => sessionKnown);
/** Both triggers, for the same reason `flushOutbox` uses both (see ../lib/api.ts): the common
 *  real outage -- a captive portal, weak signal, a proxy returning 502 -- never fires `online`
 *  at all, and coming back to the tab is the moment a user actually finds out they have a
 *  connection again. Cheap to repeat: it only runs while the session is still unknown. */
const retrySession = () => { if (!sessionKnown) void loadSession(); };
/** One attempt at a time. `sessionKnown` only flips after two awaited round trips, so behind a
 *  slow or hanging `/auth/status` every tab switch would otherwise start another complete
 *  `loadSession` -- stacking auth round trips without bound on exactly the flaky connection the
 *  retry exists for, and firing `setOutboxUser`/`flushOutbox` once per attempt that finishes. */
let inFlight: Promise<boolean> | null = null;
globalThis.addEventListener?.('online', retrySession);
globalThis.addEventListener?.('visibilitychange', () => {
  if (globalThis.document?.visibilityState === 'visible') retrySession();
});

/**
 * The 30s poke while stuck in offline mode. `online` and `visibilitychange` above only fire on
 * an actual reconnect event or a tab regaining focus -- neither has to happen on a device that
 * quietly regains signal in the background (a phone in a pocket, a laptop lid left open), which
 * otherwise left the app showing stale data until the user happened to switch away and back.
 *
 * `startOfflineRetry` is idempotent -- `openOffline` may run again on every failed retry attempt
 * while nothing about the situation has changed, and must not stack a second interval each time.
 * It also refuses to start once `sessionKnown` is already true: `openOffline` has a real `await`
 * (the cache clear) between its last `sessionKnown` check and its next synchronous step, so a
 * session confirmed by a racing `adoptUser` call while `openOffline` is unwinding must not leave
 * a timer running for a question that has already been answered. `stopOfflineRetry` is called
 * from every place `sessionKnown` becomes true (`endSession`, `adoptUser` -- which sets it BEFORE
 * calling `stopOfflineRetry`, precisely so `login()`, which has no `inFlight` guard of its own,
 * cannot have a stray tick start a genuinely concurrent `doLoadSession()` while it awaits
 * `adoptUser` -- and the setup-required branch of `doLoadSession`) for the ordinary case; this is
 * the belt-and-suspenders for `openOffline`'s own remaining gap.
 */
let offlineRetryTimer: ReturnType<typeof setInterval> | null = null;
function startOfflineRetry(): void {
  if (sessionKnown || offlineRetryTimer !== null) return;
  offlineRetryTimer = setInterval(() => { if (!sessionKnown) void loadSession(); }, 30_000);
}
function stopOfflineRetry(): void {
  if (offlineRetryTimer === null) return;
  clearInterval(offlineRetryTimer);
  offlineRetryTimer = null;
}

/**
 * Another tab of this app changed who is signed in -- signed in as someone else, signed out, or
 * found setup required -- and wrote that to `localStorage`. This tab would otherwise go on
 * showing the previous user's screens, and loading into the caches the other tab now owns for
 * someone else. So it stops the safe way: shell hidden, in-memory caches dropped, and a reload to
 * the root, which runs the session check against the current cookie and the owner guard again.
 * The on-disk caches are left to the tab that changed the user: it has already cleared them.
 * `userSwitchNeedsReload` decides, and only on a real change, so tabs cannot set each other off.
 */
let reloading = false;
globalThis.addEventListener?.('storage', (e: StorageEvent) => {
  if (reloading || !userSwitchNeedsReload(e.key, e.oldValue, e.newValue, get(user)?.id)) return;
  reloading = true;
  user.set(undefined);
  clearObjectMemory();
  clearCustomTypes();
  globalThis.location?.replace('/');
});

/** Resolves to whether the session is now KNOWN -- signed in or signed out, as opposed to
 *  still unreachable. Callers that have to show something either way (`Setup`) need to tell
 *  those apart; the retry listeners above only care that it eventually becomes true. */
export function loadSession(): Promise<boolean> {
  if (!inFlight) inFlight = doLoadSession().finally(() => { inFlight = null; });
  return inFlight;
}

/** Drops the on-disk and in-memory caches when they are not `userId`'s (see ./cache-owner).
 *  Resolves only once the service-worker caches are really gone, so a caller that then sets the
 *  user cannot have the first load for them answered from the previous person's cache.
 *
 *  The owner is recorded LAST, once the deletes have succeeded. A delete that rejects twice
 *  rejects this too -- the user is then never set, and the owner stays whoever it was, so the
 *  next attempt clears again instead of keeping the previous person's caches as `userId`'s. */
async function clearCachesUnlessOwnedBy(userId: number): Promise<boolean> {
  if (cachesBelongTo(userId)) return false;
  // One retry: a transient failure should not leave the app on "Loading…" until a reload.
  await clearObjectCache().catch(() => clearObjectCache());
  clearCustomTypes();
  // `clearCustomTypes` forgets only the list of the owner it knows in this tab; after a reload
  // that is nobody, so the previous person's stored list would otherwise stay on disk.
  clearStoredTypeLists();
  recordCacheOwner(userId);
  return true;
}

/**
 * Makes `me` -- confirmed by the server -- the user, after making sure nothing on screen or in a
 * cache belongs to someone else. The clear runs before `user.set`, because setting the user is
 * what starts every data load for them (App's type load, the routes it mounts).
 */
async function adoptUser(me: User): Promise<void> {
  const shown = get(user);
  if (shown && shown.id !== me.id) {
    // Offline mode was showing someone else. Unmount the shell first ("Loading…"), so no
    // mounted screen keeps holding their data in its own state once the caches are gone.
    user.set(undefined);
    await tick();
    // A note (or staleness recorded per path) earned by the previous person's requests must not
    // linger over the incoming user's screens -- the same reasoning `endSession` already applies.
    clearServingSaved();
  }
  if ((await clearCachesUnlessOwnedBy(me.id)) && shown?.id === me.id) {
    // Same person still on screen, so App's per-user type load will not rerun on its own.
    void loadCustomTypes(me.id);
  }
  // Carry over a previously remembered currency for the SAME user rather than dropping it for
  // the moment before `/settings` answers again below (`doLoadSession`/`login`, right after this
  // returns) -- an offline start caught in that narrow window would otherwise fall back to the
  // default currency instead of the last one actually known for this device.
  const previous = rememberedProfile();
  const carryCurrency = previous?.id === me.id ? previous.currency : undefined;
  rememberProfile(carryCurrency !== undefined ? { ...me, currency: carryCurrency } : me);
  // Set HERE, not left to the caller: `login()` does not go through `loadSession`'s `inFlight`
  // guard at all, so while this was only set after `await adoptUser(me)` returned, a 30s offline
  // retry tick landing during `login`'s own call to this function (`sessionKnown` still false the
  // whole time) started a genuinely concurrent, unrelated `doLoadSession()` -- not merely a
  // wasted no-op call the way it is everywhere `loadSession`'s guard applies. The session IS
  // confirmed the moment the server handed back `me`; this says so before either of the two
  // statements right below can matter to anything reading `sessionKnown`.
  sessionKnown = true;
  offlineState.set(false);
  stopOfflineRetry();
  user.set(me);
  setOutboxUser(me.id);
}

/**
 * Called when the instance currency changes (`settings/Appearance.svelte`) so the currently
 * signed-in user's remembered profile keeps up immediately -- otherwise an offline start right
 * after the change would show the currency this device knew before it, until the next successful
 * `/settings` load remembers it again. A no-op with nobody signed in (there is no profile to
 * update), which cannot happen in practice since only an admin reaches that screen's save button.
 */
export function rememberCurrentCurrency(newCurrency: string): void {
  const me = get(user);
  if (me) rememberProfile({ ...me, currency: newCurrency });
}

/**
 * The session check could not reach the server. With a remembered profile the app opens as
 * that person, on whatever the caches hold, rather than sitting on "Loading…" -- but nothing
 * is asserted about the session: it stays unknown, the retry keeps trying, and the outbox may
 * queue under this id but not send (see `setOutboxSendGate`).
 */
async function openOffline(): Promise<void> {
  if (sessionKnown) return; // a known session is not undone by one failed request
  const profile = rememberedProfile();
  if (!profile) return;
  // The profile and owner are written together, so they agree -- unless storage was half
  // written or edited. Then the caches are not provably this person's, and must be gone before
  // anything renders for them.
  await clearCachesUnlessOwnedBy(profile.id);
  // Re-checked: `inFlight` serialises `loadSession`, but `login` does not go through it, and a
  // sign-in that completed while the deletes ran must not be overwritten by the old profile.
  if (sessionKnown) return;
  // `currency` rides along on the stored profile (see ../lib/cache-owner.ts) but is not part of
  // `User` -- split it off rather than handing the user store a field nothing there expects.
  const { currency: profileCurrency, ...profileUser } = profile;
  if (get(user)?.id !== profile.id) user.set({ ...profileUser });
  if (profileCurrency !== undefined) currency.set(profileCurrency);
  setOutboxUser(profile.id);
  offlineState.set(true);
  startOfflineRetry();
}

async function doLoadSession(): Promise<boolean> {
  let status: { setup_required: boolean };
  try {
    status = await api<{ setup_required: boolean }>('GET', '/auth/status');
  } catch (e) {
    // Offline or a flaky boot. Nothing is known yet, so nothing is asserted -- least of all
    // that the user is signed out, which would be a lie the outbox then acts on. The `online`
    // and `visibilitychange` listeners above try again. Without a server answer (as
    // `isRejection` tells them apart) the last user may still open offline.
    if (!isRejection(e)) await openOffline();
    return false;
  }
  setupRequired.set(status.setup_required);
  if (status.setup_required) {
    user.set(null);
    sessionKnown = true;
    offlineState.set(false);
    stopOfflineRetry();
    setOutboxUser(null);
    // A reset database hands out user ids from 1 again: the previous instance's profile must not
    // open offline, and its caches must not count as the new user 7's because an old user 7 left
    // them. Without an owner recorded, the first sign-in clears them.
    forgetProfile();
    forgetCacheOwner();
    return true;
  }
  let me: User;
  try {
    me = await api<User>('GET', '/auth/me');
  } catch (e) {
    // A 4xx is the server saying nobody is signed in: the session ends as a 401 anywhere else
    // would end it (`/auth/*` does not trigger the unauthorized handler). Anything else
    // (offline, a 5xx) says only that we still do not know, so it must not be recorded as an
    // answer.
    if (isRejection(e)) { endSession(); return true; }
    await openOffline();
    return false;
  }
  await adoptUser(me); // sets sessionKnown = true itself, before it returns
  // The flush `main.ts` fires at module load happens before this, so it knows no user and
  // deliberately sends nothing (see `doFlushOutbox`). This is the boot flush that counts --
  // and after offline mode, the one that finally sends what was queued meanwhile.
  void flushOutbox();
  // A queued upload holds the only copy of that photo, so it is worth asking the browser not
  // to evict it. Fire-and-forget: see `persistStorage`.
  void persistStorage();
  try {
    const s = await api<Settings>('GET', '/settings');
    currency.set(s.currency);
    // A second write, over the one `adoptUser` already made: `/settings` is a separate request
    // that was not answered yet when that one ran, and the currency is what lets an offline
    // start (see `openOffline`) show amounts correctly without a network call.
    //
    // Guarded: this runs after an `await`, during which a 401 elsewhere (or this very function,
    // reached again) can have called `endSession()` -- which forgets the profile precisely so
    // a lapsed or signed-out session does not linger -- or signed in as someone else entirely.
    // Writing unconditionally would resurrect a profile `endSession` just forgot, or attribute
    // this settings answer to a user who is no longer the one on screen.
    if (sessionKnown && get(user)?.id === me.id) rememberProfile({ ...me, currency: s.currency });
  } catch (e) {
    // `/auth/me` has just confirmed the session, so a 4xx here is not the session ending: it is
    // only the settings that could not be read. Signing the user out without forgetting them
    // (as this once did) left a signed-out screen over a remembered profile and owned caches.
    // Settings are optional -- the default currency serves -- so the user stays.
    if (isRejection(e)) {
      console.warn('settings could not be loaded; using the default currency', e);
      currency.set('EUR');
    }
  }
  return sessionKnown;
}

export async function login(username: string, password: string): Promise<void> {
  const me = await api<User>('POST', '/auth/login', { username, password });
  await adoptUser(me); // sets sessionKnown = true itself, before it returns
  const s = await api<Settings>('GET', '/settings');
  currency.set(s.currency);
  // Guarded exactly as in doLoadSession: see the comment there.
  if (sessionKnown && get(user)?.id === me.id) rememberProfile({ ...me, currency: s.currency });
  // Anything queued while the session was expired has been waiting for exactly this. The
  // outbox's own triggers -- load, `online`, `visibilitychange` -- none of them fire on a
  // login, which is an SPA navigation, so without this the writes sit until the user happens
  // to switch away from the tab and back.
  //
  // Twice, for the same reason as `retryDead` (see ../lib/api.ts): `flushOutbox` is
  // `serialize`d, so a single call made while a pre-login pass is still in flight would just
  // join that pass -- the one that is 401ing everything and knows nothing of this user -- and
  // return having sent nothing. The second call is guaranteed to be a genuinely new pass.
  // `.catch` before the chain, not after: `doFlushOutbox` rethrows (an IndexedDB failure in
  // `store.all()`, say), and a plain `.then` would drop the second pass exactly when it is
  // needed -- besides leaving the rejection unhandled.
  void flushOutbox().catch(() => {}).then(() => flushOutbox()).catch(() => {});
}

export async function logout(): Promise<void> {
  await api('POST', '/auth/logout');
  endSession();
  navigate('/login', true);
}

/** Ends every session of this account, on every device, this browser included. */
export async function logoutEverywhere(): Promise<void> {
  await api('POST', '/auth/logout-all');
  endSession();
  navigate('/login', true);
}
