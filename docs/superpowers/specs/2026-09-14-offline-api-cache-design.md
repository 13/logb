# Offline API cache that actually caches

Status: approved, not implemented.

## Problem

`frontend/vite.config.ts` configures Workbox runtime caching with regular expressions such as
`/^\/api\//`. Workbox's `RegExpRoute` tests a pattern against the full URL
(`https://host/api/objects`), so none of the four API rules has ever matched:

- nothing under `/api` is cached, so an app started without a connection shows "Failed to fetch"
  on the objects list, and a restart loses everything but the offline outbox;
- the `NetworkOnly` rules meant to guarantee that sign-in state and exports are never served from a
  cache protected nothing -- harmless only because nothing was cached at all.

Fixing the matchers turns on persistent, on-disk caching of authenticated responses, so the fix
must also decide what may be cached and make sure one user never sees another's data.

## Matchers

Every rule becomes a function matcher on the same origin and the path:
`({ url, sameOrigin }) => sameOrigin && url.pathname.startsWith('/api/…')`. Rules are listed in
this order, first match wins:

1. **Never cached** (`NetworkOnly`): `/api/auth/`, `/api/export`, `/api/import`, `/api/sync/`,
   `/api/search`, `/api/settings`, `/api/users`, `/api/database`, `/api/tokens`, `/api/me/`,
   `/api/stats`, `/api/health`, and anything else not listed under 2 or 3.
2. **Content-addressed files** (`CacheFirst`, `logb-files`, 300 entries, 30 days): `/api/files/`.
3. **Data a household reads offline** (`NetworkFirst`, `logb-api`, 4 s network timeout, 200
   entries, 7 days, only status 200): `/api/objects`, `/api/activities`, `/api/reminders`,
   `/api/types`, `/api/tags`.

Only `GET` requests are ever cached (Workbox's default for runtime routes; stated explicitly).

The matchers live in a small pure module (`frontend/src/lib/sw-routes.ts`) imported by
`vite.config.ts`, so they can be unit-tested.

## One user's data only

Already true: signing out, signing out everywhere and a 401 clear `logb-api` and `logb-files`
(`clearObjectCache` in `object-cache.ts`).

New: the id of the user whose data the caches hold is kept in `localStorage`
(`logb.cache.user`). When a session is established (sign-in or the session check on app start)
for a different user id -- or when none is stored yet -- both caches, the in-memory object cache
and the stored own-type list are cleared before anything else loads, and the new id is stored.
This covers a session that simply expired and another person signing in on the same device.

## Starting offline

A cache alone does not open the app: on start, `loadSession` asks `/api/auth/status` and
`/api/auth/me`, which are never cached, so without a connection the signed-in user stays unknown
and the shell shows "Loading…" indefinitely.

After every successful session check or sign-in, a small profile of the signed-in user --
`{ id, username, is_admin, lang }`, the fields the shell needs -- is kept in `localStorage`
(`logb.session.profile`). On start, when the session check fails for lack of a connection (not a
401) and a profile is stored, the app opens as that user in **offline mode**:

- the shell renders normally, with a small "Offline -- showing saved data" note in the top bar;
- data screens read from the service-worker cache and the in-memory caches as usual;
- entries logged offline go to the outbox as they do today, attached to that user id, but the
  outbox does not send anything until a real session check has succeeded -- `sessionKnown` stays
  false in offline mode and the flush waits for it;
- when the connection returns, the normal retry runs the real session check: the same user
  continues seamlessly; a 401 goes through the unauthorized handler (sign-in screen, caches
  cleared, outbox detached but kept, as today); a different user id clears caches and profile
  first.

Signing out, signing out everywhere and a 401 remove the stored profile together with the caches.
On a shared device where someone never signed out, their saved data opens offline -- the same
exposure as leaving the app signed in, which it effectively is.

## Comments and specs

`object-cache.ts` and `type-registry.ts` comments, and the own-types spec's "Offline" bullet, are
corrected to say what is cached.

## Tests

- Vitest (`sw-routes.test.ts`): each path above lands on the intended rule; a foreign-origin
  `/api/objects` matches nothing; `/api/auth/me` is never cached; unknown `/api/…` paths are
  network-only.
- Vitest: the user-change guard clears caches when the stored id differs or is missing, and not
  when it matches.
- Playwright (`25-offline-cache.spec.ts`, against the production build with the service worker):
  - open the objects list and an object online; go offline (`context.setOffline(true)`); reload;
    the list and the object page still show their data;
  - sign out; go offline; sign in is impossible offline, so instead: sign in as user A online, load
    the list, sign out, sign in as user B online, go offline and reload: none of A's objects
    appear;
  - `/api/auth/me` is never answered from cache: after signing out online and going offline, a
    reload shows the sign-in screen, not A's session.

## Out of scope

Caching search, statistics or settings offline; background sync of cached reads; changing the
offline outbox.
