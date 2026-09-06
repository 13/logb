import { writable } from 'svelte/store';
import { api, flushOutbox, isRejection, setOutboxUser, setUnauthorizedHandler } from '../lib/api';
import { clearObjectCache } from '../lib/object-cache';
import type { Settings, User } from '../lib/types';
import { go } from '../lib/router';

/** undefined = not loaded yet, null = anonymous */
export const user = writable<User | null | undefined>(undefined);
export const setupRequired = writable<boolean>(false);
export const currency = writable<string>('EUR');

// Login and logout are SPA navigations (no page reload), so module-scope caches like
// ObjectDetail's are never cleared on their own between users on a shared device — every path
// that ends a session must drop them itself. See the invariant on `clearObjectCache`.
setUnauthorizedHandler(() => {
  user.set(null);
  sessionKnown = true;
  clearObjectCache();
  // The queue is NOT cleared: an expired session is exactly when a write must survive until
  // the user signs back in. It is only detached from the current session, so nothing replays
  // or displays it until someone claims it by logging in (see `setOutboxUser` in ../lib/api).
  setOutboxUser(null);
  if (location.pathname !== '/login') go('/login', true);
});

/**
 * Whether the app has actually learned who is signed in, as opposed to having learned that
 * nobody is. A boot with no connection cannot tell the two apart -- `/api/auth/*` is
 * NetworkOnly in the service worker, so the very first request rejects -- and the outbox
 * refuses to send while it has no user, by design. Without a retry that state was permanent
 * for the life of the page: reconnecting fired a flush that returned immediately, and the
 * queued writes sat there, with a perfectly valid cookie, until a manual reload.
 */
let sessionKnown = false;
globalThis.addEventListener?.('online', () => { if (!sessionKnown) void loadSession(); });

export async function loadSession(): Promise<void> {
  let status: { setup_required: boolean };
  try {
    status = await api<{ setup_required: boolean }>('GET', '/auth/status');
  } catch {
    // Offline or a flaky boot. Nothing is known yet, so nothing is asserted -- least of all
    // that the user is signed out, which would be a lie the outbox then acts on. The `online`
    // listener above tries again.
    return;
  }
  setupRequired.set(status.setup_required);
  if (status.setup_required) { user.set(null); sessionKnown = true; return; }
  try {
    const me = await api<User>('GET', '/auth/me');
    user.set(me);
    setOutboxUser(me.id);
    sessionKnown = true;
    // The flush `main.ts` fires at module load happens before this, so it knows no user and
    // deliberately sends nothing (see `doFlushOutbox`). This is the boot flush that counts.
    void flushOutbox();
    const s = await api<Settings>('GET', '/settings');
    currency.set(s.currency);
  } catch (e) {
    // A 401 is the server saying nobody is signed in; anything else (offline, a 5xx) says only
    // that we still do not know, so it must not be recorded as an answer.
    if (isRejection(e)) { user.set(null); sessionKnown = true; }
  }
}

export async function login(username: string, password: string): Promise<void> {
  const me = await api<User>('POST', '/auth/login', { username, password });
  user.set(me);
  setOutboxUser(me.id);
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
  user.set(null);
  sessionKnown = true;
  clearObjectCache();
  setOutboxUser(null);
  go('/login', true);
}

/** Ends every session of this account, on every device, this browser included. */
export async function logoutEverywhere(): Promise<void> {
  await api('POST', '/auth/logout-all');
  user.set(null);
  sessionKnown = true;
  clearObjectCache();
  setOutboxUser(null);
  go('/login', true);
}
