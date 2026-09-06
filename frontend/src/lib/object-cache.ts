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

/** Drop every cached object and activities page. Call on logout and on any handled
 *  unauthorized response — this cache must never survive past the session that populated it. */
export function clearObjectCache(): void {
  objects.clear();
  activityPages.clear();
}
