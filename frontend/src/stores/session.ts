import { get, readonly, writable, type Readable } from 'svelte/store';
import { tick } from 'svelte';
import { api, flushOutbox, isRejection, persistStorage, setOutboxSendGate, setOutboxUser, setUnauthorizedHandler } from '../lib/api';
import { claimCaches, forgetCacheOwner, forgetProfile, rememberedProfile, rememberProfile } from '../lib/cache-owner';
import { clearObjectCache } from '../lib/object-cache';
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

/** Resolves to whether the session is now KNOWN -- signed in or signed out, as opposed to
 *  still unreachable. Callers that have to show something either way (`Setup`) need to tell
 *  those apart; the retry listeners above only care that it eventually becomes true. */
export function loadSession(): Promise<boolean> {
  if (!inFlight) inFlight = doLoadSession().finally(() => { inFlight = null; });
  return inFlight;
}

/** Drops the on-disk and in-memory caches when they are not `userId`'s (see ./cache-owner).
 *  Resolves only once the service-worker caches are really gone, so a caller that then sets the
 *  user cannot have the first load for them answered from the previous person's cache. */
async function clearCachesUnlessOwnedBy(userId: number): Promise<boolean> {
  if (!claimCaches(userId)) return false;
  await clearObjectCache();
  clearCustomTypes();
  // `clearCustomTypes` forgets only the list of the owner it knows in this tab; after a reload
  // that is nobody, so the previous person's stored list would otherwise stay on disk.
  clearStoredTypeLists();
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
  }
  if ((await clearCachesUnlessOwnedBy(me.id)) && shown?.id === me.id) {
    // Same person still on screen, so App's per-user type load will not rerun on its own.
    void loadCustomTypes(me.id);
  }
  rememberProfile(me);
  offlineState.set(false);
  user.set(me);
  setOutboxUser(me.id);
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
  if (get(user)?.id !== profile.id) user.set({ ...profile });
  setOutboxUser(profile.id);
  offlineState.set(true);
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
  await adoptUser(me);
  sessionKnown = true;
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
  } catch (e) {
    if (isRejection(e)) { user.set(null); sessionKnown = true; }
  }
  return sessionKnown;
}

export async function login(username: string, password: string): Promise<void> {
  const me = await api<User>('POST', '/auth/login', { username, password });
  await adoptUser(me);
  sessionKnown = true;
  const s = await api<Settings>('GET', '/settings');
  currency.set(s.currency);
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
