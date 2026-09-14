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

/**
 * `currency` is optional: it comes from `/settings`, a separate request from the one that
 * establishes the other fields, so it is filled in a moment later (see `rememberProfile`'s
 * second call in ../stores/session.ts) and a profile stored before this field existed has none.
 * Used only so an offline start can show amounts in the right currency without a network call.
 */
export interface Profile { id: number; username: string; is_admin: boolean; lang: string; currency?: string }

/** Resolved inside each caller's `try`: merely reading `globalThis.localStorage` can throw. */
function storageOf(storage?: Storage): Storage | undefined {
  return storage ?? globalThis.localStorage;
}

export function rememberProfile(p: Profile, storage?: Storage): void {
  try {
    // Only these fields: this is kept on disk after the tab closes, so nothing beyond what the
    // shell needs to render goes into it. `currency` is omitted rather than stored as
    // `undefined` -- `JSON.stringify` drops an `undefined` property anyway, but doing it
    // explicitly keeps the "no currency yet" and "currency is EUR" cases from ever looking
    // alike in the raw JSON.
    const { id, username, is_admin, lang, currency } = p;
    const data: Record<string, unknown> = { id, username, is_admin, lang };
    if (currency !== undefined) data.currency = currency;
    storageOf(storage)?.setItem(PROFILE_KEY, JSON.stringify(data));
  } catch { /* full or blocked: the app just cannot open offline next time */ }
}

/** The stored profile, or null when there is none or it is not a whole profile (an older or
 *  hand-edited value must not be handed to the shell as a user). `currency` reads back as
 *  `undefined` for a profile stored before this field existed, or one this device has never
 *  loaded `/settings` for yet -- not a reason to reject the rest of the profile. */
export function rememberedProfile(storage?: Storage): Profile | null {
  try {
    const raw = storageOf(storage)?.getItem(PROFILE_KEY);
    if (!raw) return null;
    const p: unknown = JSON.parse(raw);
    if (typeof p !== 'object' || p === null) return null;
    const { id, username, is_admin, lang, currency } = p as Record<string, unknown>;
    if (!Number.isInteger(id) || typeof username !== 'string' || typeof is_admin !== 'boolean' || typeof lang !== 'string') return null;
    if (currency !== undefined && typeof currency !== 'string') return null;
    return { id: id as number, username, is_admin, lang, ...(currency !== undefined ? { currency: currency as string } : {}) };
  } catch {
    return null;
  }
}

export function forgetProfile(storage?: Storage): void {
  try { storageOf(storage)?.removeItem(PROFILE_KEY); } catch { /* nothing stored to forget */ }
}

/**
 * True only when the caches are recorded as `userId`'s. Anything else -- someone else, nobody
 * recorded, storage that cannot be read -- means they must be cleared before `userId` sees them.
 *
 * Deliberately only a check: the owner is recorded separately (`recordCacheOwner`), and only
 * once the clear has finished. Recording first left a rejected `caches.delete`, or a tab closed
 * mid-delete, with the new user on record as owner of the previous user's caches -- which the
 * next start then kept.
 */
export function cachesBelongTo(userId: number, storage?: Storage): boolean {
  try {
    return storageOf(storage)?.getItem(OWNER_KEY) === String(userId);
  } catch {
    return false;
  }
}

/** Records `userId` as the owner of the caches. Call only once they are provably theirs (emptied). */
export function recordCacheOwner(userId: number, storage?: Storage): void {
  try { storageOf(storage)?.setItem(OWNER_KEY, String(userId)); } catch { /* unrecorded: the next start clears again */ }
}

/** The user id a stored `logb.cache.user` or `logb.session.profile` value names, or null. */
function userIdIn(key: string, value: string | null): number | null {
  if (value === null) return null;
  if (key === OWNER_KEY) return /^\d+$/.test(value) ? Number(value) : null;
  try {
    const id = (JSON.parse(value) as { id?: unknown } | null)?.id;
    return Number.isInteger(id) ? (id as number) : null;
  } catch {
    return null;
  }
}

/**
 * Whether a `storage` event -- another tab of this app writing `localStorage` -- means this tab
 * has to stop showing what it shows. It does when another tab moved the cache owner or the
 * remembered profile to a different user, or removed them (a sign-out, a session that ended,
 * setup being required), while this tab holds a user.
 *
 * Pure, and deliberately narrow so it cannot loop: a tab never receives its own writes, an
 * unchanged value is not a change, and a write naming the user this tab already holds (the same
 * person signing in elsewhere, a language change) is no reason to reload. `key` null is
 * `localStorage.clear()`.
 */
export function userSwitchNeedsReload(key: string | null, oldValue: string | null, newValue: string | null, heldUserId: number | null | undefined): boolean {
  if (heldUserId === null || heldUserId === undefined) return false; // nothing of anyone's on screen
  if (key === null) return true;
  if (key !== OWNER_KEY && key !== PROFILE_KEY) return false;
  if (oldValue === newValue) return false;
  return userIdIn(key, newValue) !== heldUserId;
}

export function forgetCacheOwner(storage?: Storage): void {
  try { storageOf(storage)?.removeItem(OWNER_KEY); } catch { /* nothing stored to forget */ }
}
