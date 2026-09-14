/**
 * Whose data the on-disk caches hold, and who last signed in on this device.
 *
 * The service worker's `logb-api`/`logb-files` caches and the stored own-type list survive a
 * reload, a closed tab and an expired session -- so "clear them when a session ends" is not
 * enough on its own: a session that simply lapsed never runs a logout, and the next person to
 * sign in on the device would be served the previous person's objects from cache. Recording the
 * owner lets every session start check it before anything loads.
 *
 * The profile is what lets the app open without a connection at all: `/api/auth/*` is never
 * cached, so without it an offline start could not say who is signed in and stayed on
 * "Loading…" for good.
 *
 * Every access is wrapped: `localStorage` throws in some private modes and when site data is
 * blocked. Failing closed there means remembering nothing and always clearing.
 */

const OWNER_KEY = 'logb.cache.user';
const PROFILE_KEY = 'logb.session.profile';

export interface Profile { id: number; username: string; is_admin: boolean; lang: string }

/** Resolved inside each caller's `try`: merely reading `globalThis.localStorage` can throw. */
function storageOf(storage?: Storage): Storage | undefined {
  return storage ?? globalThis.localStorage;
}

export function rememberProfile(p: Profile, storage?: Storage): void {
  try {
    // Only these fields: this is kept on disk after the tab closes, so nothing beyond what the
    // shell needs to render goes into it.
    const { id, username, is_admin, lang } = p;
    storageOf(storage)?.setItem(PROFILE_KEY, JSON.stringify({ id, username, is_admin, lang }));
  } catch { /* full or blocked: the app just cannot open offline next time */ }
}

/** The stored profile, or null when there is none or it is not a whole profile (an older or
 *  hand-edited value must not be handed to the shell as a user). */
export function rememberedProfile(storage?: Storage): Profile | null {
  try {
    const raw = storageOf(storage)?.getItem(PROFILE_KEY);
    if (!raw) return null;
    const p: unknown = JSON.parse(raw);
    if (typeof p !== 'object' || p === null) return null;
    const { id, username, is_admin, lang } = p as Record<string, unknown>;
    if (!Number.isInteger(id) || typeof username !== 'string' || typeof is_admin !== 'boolean' || typeof lang !== 'string') return null;
    return { id: id as number, username, is_admin, lang };
  } catch {
    return null;
  }
}

export function forgetProfile(storage?: Storage): void {
  try { storageOf(storage)?.removeItem(PROFILE_KEY); } catch { /* nothing stored to forget */ }
}

/** True when the caches belong to someone else (or to nobody recorded) and must be cleared; records `userId` as the owner. */
export function claimCaches(userId: number, storage?: Storage): boolean {
  try {
    const s = storageOf(storage);
    // No storage means no record of whose the caches are, and "unknown" must clear.
    if (!s) return true;
    const same = s.getItem(OWNER_KEY) === String(userId);
    s.setItem(OWNER_KEY, String(userId));
    return !same;
  } catch {
    return true;
  }
}

export function forgetCacheOwner(storage?: Storage): void {
  try { storageOf(storage)?.removeItem(OWNER_KEY); } catch { /* nothing stored to forget */ }
}
