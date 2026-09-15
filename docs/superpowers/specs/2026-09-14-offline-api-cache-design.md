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
user's bytes. The service worker's `logb-files` cache is cleared per user as above. For entries the
browser cached under the old `immutable` header, before this scheme existed, every successful
setup/login/logout/logout-all response also carries `Clear-Site-Data: "cache"`, so those bytes
don't outlive the session that fetched them.

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

### A1. Saved data shown while online

`NetworkFirst` (see "Matchers" above) answers from `logb-api` once the network has taken longer
than its 4s timeout -- an ordinary slow connection, not a lost one, so `offline` mode itself
never sees it. `frontend/src/lib/api.ts` compares a successful response's `Date` header with the
moment the request was sent, minus a calibrated clock-skew estimate (`servedFromCache`,
unit-tested directly): a response dated more than 60s before that, after correction, came from
the cache, not the network just now.

Staleness is kept per REQUEST PATH -- a `Set<string>` of paths (query string included, exactly as
passed to `api()`/`apiPage()`/etc) currently answering stale -- not as one flag. Most screens have
several requests in flight together (the dashboard alone fires five); a single "last response
wins" flag flapped back to false the instant any ONE of them answered fresh, even while another
was still visibly showing data from `logb-api`. A key is added by a stale response and removed
only by a FRESH response to that SAME path, never by an unrelated one. The whole set is cleared
(`clearServingSaved`) on a route change -- wired to the router's `path` store, since each screen
re-fetches what it needs and staleness recorded for the previous screen stops being meaningful --
and when a session ends (`endSession` in ../stores/session.ts). The `servingSaved` store is true
whenever the set is non-empty; the top bar shows the existing offline note whenever `offline` mode
*or* `servingSaved` is true. A response with no `Date` header, or one that fails to parse, counts
as fresh -- there is nothing there to prove otherwise. (Confirmed empirically: LogB's own HTTP
stack always sends `Date`, so this only ever fires on an actual cache hit.)

A route change (or a session end) also bumps a `routeGeneration` counter; each request captures
the current generation alongside `sentAt` when it is sent, and a response whose captured
generation no longer matches is ignored entirely -- a request made for a screen the user has since
left cannot re-add a key the route change already cleared once its (possibly very late) answer
finally arrives.

A screen that replaces its query WITHOUT a route change -- the activity timeline's category and
tag filters, and its refresh -- calls `supersedeStale(prefix)` (`/objects/<id>/activities?`)
before it fetches. That drops the stale keys under the prefix, so the note does not stay up over
a list that now came fresh, and ignores answers to requests under the prefix that were sent
before the call (each request is numbered as it is sent), so a slow stale answer to the old query
cannot put its key back. Loading older entries ("Show N older") does not supersede: the rows
already shown stay on screen, and so does their staleness. The objects list filters and sorts in
the browser from one fetch, and search and statistics are never cached, so no other screen needs
this.

Clock skew: a self-hosted instance's server clock can be far off from the client's -- no RTC on a
Raspberry Pi that boots believing it's 1970, or simply the wrong timezone -- by much more than the
60s threshold above, in either direction. Uncorrected, that would show the note permanently
(server ahead of the client) or hide a genuine cache hit forever (server behind, so a stale cached
response still reads as "recent enough"). `api.ts` maintains a calibrated skew estimate, updated
from every response to a path that can NEVER be a cache hit: `/api/auth/...` and `/api/settings`
are `NetworkOnly` (see "Matchers" above), so their `Date` header always reflects a live request
made moments ago -- any gap from it is clock skew, not cache age. Measured from the MIDPOINT of
the round trip (`(sentAt + Date.now()) / 2`), not from `sentAt` itself -- the server wrote its
`Date` header partway through the trip back, and the midpoint is the best guess at "when" without
a timestamp from the server -- and skipped entirely when the round trip took more than 5s: a slow
response's own latency would otherwise be misread as clock skew, once badly enough (a 70s round
trip read as ~70s of skew) to make every later FRESH response look like it came from the cache.
Both endpoints are requested on every session check, so the estimate keeps recalibrating rather
than trusting one reading for the life of the tab. `servedFromCache(dateHeader, sentAt, skewMs)`
takes the current estimate as its third argument and subtracts it before comparing.

### A2. Sign-out without a connection

`logout` and `logoutEverywhere` already fail before changing any session state when the request
never reaches the server (they simply propagate the fetch error). Their callers -- `SignedIn.svelte`
and `settings/Account.svelte`, both the "Sign out" and "Sign out everywhere" buttons -- show
`nav.signout-offline` ("Signing out needs a connection." / "Zum Abmelden ist eine Verbindung
nötig.") for that case, and the server's own message for an `ApiError`. `signOutErrorMessage(e,
$t)` (`../stores/session.ts`) tells the two apart by `instanceof ApiError` and takes the translate
function so it always returns the finished, displayable string -- the server's message unchanged
for an `ApiError`, `$t('nav.signout-offline')` otherwise -- so both callers just do
`error = signOutErrorMessage(e, $t)`.

### A3. Stuck in offline mode

Besides `online` and `visibilitychange`, `session.ts` also polls every 30s while offline mode
holds and the session is still not known -- a device can quietly regain a connection with
neither event firing (nothing reconnected in the network-interface sense, no tab switch). The
timer starts the first time the app opens offline and stops the moment the session becomes known
(a real sign-in, sign-out, or 401), whichever attempt gets there first; repeated failed attempts
in the meantime never start a second, overlapping one.

### A4. Currency offline

The remembered profile gains an optional `currency`, filled in by a second write once `/settings`
loads (a separate request from the one that establishes the rest of the profile) and used to set
`currency` when the app opens offline. A profile stored before this field existed, or one from a
device that has not loaded `/settings` yet this session, simply has no `currency` -- the store
keeps its default (`EUR`) in that case, exactly as it does online before `/settings` answers.

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
