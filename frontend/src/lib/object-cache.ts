import type { Activity, MemObject } from './types';

/**
 * The last object and activities page fetched for each id, kept only for the lifetime of this
 * tab (module scope, so it survives navigating away and back — a fresh component instance
 * would otherwise lose it on every remount). A dead connection can still show the object it
 * showed a moment ago instead of a bare "failed to fetch" screen.
 *
 * INVARIANT: this cache may only ever hold data for the currently authenticated session.
 * Login and logout are SPA navigations (no page reload), so nothing else clears module-scope
 * state between users on a shared device — every place that ends a session (`logout` /
 * `logoutEverywhere` in `../stores/session.ts`, and the app's unauthorized handler) MUST call
 * `clearObjectCache()`. Just as important: a load must fall back to this cache only on a
 * genuine connectivity failure (see `isRejection` in `./api.ts`) — never on a 401/403/404,
 * which is the server *answering*, possibly about an object that belongs to someone else
 * entirely, not the network failing to deliver the request.
 *
 * The SAME invariant covers the service worker's Workbox caches (`logb-api` / `logb-files`,
 * configured in vite.config.ts): they hold `GET /api/...` responses -- including this same
 * object and activities data, plus the photos it links to via `cover_file_id` -- entirely
 * outside this module, and just as durably across an SPA login/logout. `clearObjectCache()`
 * drops those too, for exactly the reason it drops the Maps below: a `NetworkFirst`/
 * `CacheFirst` hit served after logout would otherwise re-poison this module's cache with the
 * previous user's data on a shared device.
 *
 * Ending a session is not the only way to change hands: a session can simply expire, with no
 * logout to run, and the Workbox caches outlive the tab. So every session START checks whose
 * data the caches hold too (`claimCaches` in `./cache-owner.ts`, called from
 * `../stores/session.ts`) and clears here, before the new user is set and anything loads for
 * them, when that is anyone else -- or nobody recorded.
 */
const objects = new Map<number, MemObject>();
const activityPages = new Map<number, { items: Activity[]; total: number }>();

export function getCachedObject(id: number): MemObject | undefined {
  return objects.get(id);
}

export function setCachedObject(id: number, obj: MemObject): void {
  objects.set(id, obj);
}

export function getCachedActivities(id: number): { items: Activity[]; total: number } | undefined {
  return activityPages.get(id);
}

export function setCachedActivities(id: number, page: { items: Activity[]; total: number }): void {
  activityPages.set(id, page);
}

/** Drop every cached object and activities page, plus the service worker's `logb-api` and
 *  `logb-files` Workbox caches (see the invariant above) -- names must match vite.config.ts
 *  exactly, or a stale SW response keeps answering after this call and this cache gets
 *  re-poisoned from it on the very next load. `globalThis.caches` is guarded because it does
 *  not exist under vitest (node) or in a browser with no service worker support. Call on
 *  logout and on any handled unauthorized response — this cache must never survive past the
 *  session that populated it. */
export function clearObjectCache(): void {
  objects.clear();
  activityPages.clear();
  void globalThis.caches?.delete('logb-api');
  void globalThis.caches?.delete('logb-files');
}
