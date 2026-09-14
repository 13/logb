# Offline API cache that actually caches

Status: implemented.

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
and the stored own-type list are cleared before anything else loads, and the new id is stored
only once that clear has finished (a failed delete records nothing, so the next start clears
again). This covers a session that simply expired and another person signing in on the same
device.

When an app opened offline as one user turns out to be signed in as another, the shell is hidden
("Loading…") before the caches are cleared and the new user is set, so no mounted screen keeps the
previous user's data.

When the instance reports that setup is required (a reset database, whose user ids start over),
the stored profile and cache owner are forgotten, so a new user with an old id inherits neither.

Other open tabs follow a user switch: they listen for `storage` events on `logb.cache.user` and
`logb.session.profile`, and when another tab changes either to a different user or removes it,
they hide the shell, drop their in-memory caches and reload to `/`, which runs the session check
and the owner guard again. A write naming the user the tab already holds does nothing.

Uploaded files (`/api/files/{id}` and `/api/files/{id}/thumb`) are served `Cache-Control: private,
no-cache` with a strong `ETag` from the stored file's sha256 (with a `-thumb` suffix for the
thumbnail). The browser HTTP cache therefore revalidates every reuse, and a `304` is answered only
after the ownership check, so another user on the same browser gets a 404, never the previous
user's bytes. The service worker's `logb-files` cache is cleared per user as above.

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
  when it matches; a rejected `caches.delete` leaves the owner unchanged and the next claim
  clears again; the pure decision whether a `storage` event requires a reload; a 403/404 from
  `/settings` after a good `/auth/me` keeps the user with the default currency.
- Cargo (`tests/attachments.rs`, SQLite and PostgreSQL): file and thumbnail responses carry
  `private, no-cache` and an `ETag`; the owner's matching `If-None-Match` gets 304, another user's
  gets 404, a different or absent validator gets 200.
- Playwright (`25-offline-cache.spec.ts`, against the production build with the service worker):
  - open the objects list and an object online; go offline (`context.setOffline(true)`); reload;
    the offline note, the list and the object page still show their data;
  - user A's session lapses (cookies cleared), B signs in through the form: B's own object is
    shown and none of A's, online and offline;
  - `/api/auth/me` is never answered from cache: after signing out online and going offline, a
    reload stays on the loading screen (nothing can say who is signed in), showing neither the
    objects list nor the previous user's objects;
  - the owner check alone, with no 401: A loads objects under the service worker, the cookies are
    replaced by B's (signed in through the API), and a load shows B's object and not A's, online
    and after an offline reload.

## Out of scope

Caching search, statistics or settings offline; background sync of cached reads; changing the
offline outbox.
