import { writable } from 'svelte/store';
import { api, flushOutbox, setOutboxUser, setUnauthorizedHandler } from '../lib/api';
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
  clearObjectCache();
  // The queue is NOT cleared: an expired session is exactly when a write must survive until
  // the user signs back in. It is only detached from the current session, so nothing replays
  // or displays it until someone claims it by logging in (see `setOutboxUser` in ../lib/api).
  setOutboxUser(null);
  if (location.pathname !== '/login') go('/login', true);
});

export async function loadSession(): Promise<void> {
  const status = await api<{ setup_required: boolean }>('GET', '/auth/status');
  setupRequired.set(status.setup_required);
  if (status.setup_required) { user.set(null); return; }
  try {
    const me = await api<User>('GET', '/auth/me');
    user.set(me);
    setOutboxUser(me.id);
    const s = await api<Settings>('GET', '/settings');
    currency.set(s.currency);
  } catch {
    user.set(null);
  }
}

export async function login(username: string, password: string): Promise<void> {
  const me = await api<User>('POST', '/auth/login', { username, password });
  user.set(me);
  setOutboxUser(me.id);
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
  void flushOutbox().then(() => flushOutbox());
}

export async function logout(): Promise<void> {
  await api('POST', '/auth/logout');
  user.set(null);
  clearObjectCache();
  setOutboxUser(null);
  go('/login', true);
}

/** Ends every session of this account, on every device, this browser included. */
export async function logoutEverywhere(): Promise<void> {
  await api('POST', '/auth/logout-all');
  user.set(null);
  clearObjectCache();
  setOutboxUser(null);
  go('/login', true);
}
