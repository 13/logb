/**
 * Which service-worker cache rule a request falls under. Workbox tests a RegExp `urlPattern`
 * against the whole URL (`https://host/api/...`), so the old `/^\/api\//` patterns never matched
 * and nothing was ever cached -- or protected. These match the path on the same origin instead.
 *
 * SELF-CONTAINED ON PURPOSE: vite-plugin-pwa copies each function into the generated service
 * worker with `toString()`, so a matcher may not use anything from outside its own body -- no
 * imports, no shared constants. `tests/sw-routes.test.ts` rebuilds each one from its source to
 * keep that true.
 */
export type RouteMatch = (ctx: { url: URL; sameOrigin: boolean; request?: Request }) => boolean;

/** Session, administration, exports, sync and anything that must never be answered from disk. */
export const neverCached: RouteMatch = ({ url, sameOrigin }) =>
  sameOrigin &&
  (url.pathname === '/api/stats' || ['/api/auth/', '/api/export', '/api/import', '/api/sync/', '/api/search', '/api/settings', '/api/users',
    '/api/database', '/api/tokens', '/api/me/', '/api/health']
    .some((prefix) => url.pathname === prefix.replace(/\/$/, '') || url.pathname.startsWith(prefix)));

/** Content-addressed blobs: never change under an id. */
export const files: RouteMatch = ({ url, sameOrigin }) => sameOrigin && url.pathname.startsWith('/api/files/');

/** What a household reads with no connection: objects, entries, reminders, types, tags. */
export const householdData: RouteMatch = ({ url, sameOrigin }) =>
  sameOrigin &&
  ['/api/objects', '/api/activities', '/api/reminders', '/api/types', '/api/tags', '/api/stats/energy', '/api/stats/fuel']
    .some((prefix) => url.pathname === prefix || url.pathname.startsWith(`${prefix}/`));

/** Anything else under /api: network only, so a new endpoint is never cached by accident. */
export const otherApi: RouteMatch = ({ url, sameOrigin }) => sameOrigin && url.pathname.startsWith('/api/');
