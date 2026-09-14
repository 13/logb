# Follow-ups after 0.8.2

Status: approved, not implemented. Clears the leftovers deliberately deferred in the objects list,
tags, own types and offline cache projects.

## A. Offline polish

1. **Saved data shown while online.** `NetworkFirst` answers from `logb-api` when the network takes
   longer than 4 s. The page cannot see that today. `frontend/src/lib/api.ts` compares a successful
   response's `Date` header with the moment the request was sent, corrected for calibrated clock
   skew (see below); a response dated more than 60 s before that (after correction) came from the
   service-worker cache. Staleness is tracked per REQUEST PATH (a `Set<string>` keyed by the exact
   path + query passed to `api()`/`apiPage()`/etc), not as one flag: most screens have several
   requests in flight at once, and a single flag flapped back to "fresh" the instant any ONE of
   them answered quickly, even while another was still visibly showing cached data. A key is added
   by a stale response and removed only by a fresh response to that SAME path; the whole set is
   cleared on a route change (each screen re-fetches what it needs) and when a session ends. The
   `servingSaved` store is true whenever the set is non-empty; the top bar shows the existing
   "Offline — showing saved data" note when offline mode *or* `servingSaved` is true. A response
   without a `Date` header, or one that fails to parse, counts as fresh. (The server's HTTP stack
   sends `Date`; the plan checks.) A route change also bumps a generation counter that each request
   captures alongside its send time; a response whose captured generation is stale (the user has
   since navigated away) is ignored entirely, so it cannot re-add a key the route change already
   cleared once its answer finally arrives.

   Clock skew: a self-hosted instance can have a server clock far off from the client's (no RTC, a
   Raspberry Pi that boots believing it's 1970), which would otherwise show the note permanently
   (server ahead) or hide a real cache hit forever (server behind). `api.ts` calibrates a skew
   estimate from responses that can never be a cache hit — `/api/auth/...` and `/api/settings` are
   `NetworkOnly` — measured from the midpoint of the round trip rather than its start, and skipped
   entirely past a 5 s round trip, so a merely slow response is never itself misread as clock skew
   — and subtracts the estimate before judging any path's staleness.
2. **Sign-out without a connection.** `logout` and `logoutEverywhere` fail on a network error
   before any session state changes. The callers (`SignedIn.svelte`, `settings/Account.svelte`)
   show `nav.signout-offline` ("Signing out needs a connection." / "Zum Abmelden ist eine
   Verbindung nötig.") for a connectivity failure; a server refusal (`ApiError`) keeps its message.
3. **Stuck in offline mode.** Besides `online` and `visibilitychange`, `session.ts` retries the
   session check every 30 s while the session is not known and the app is in offline mode; the
   timer stops once the session is known.
4. **Currency offline.** The remembered profile gains `currency`, stored after `/settings` loads
   and used for `currency` when the app opens offline.

## B. Tags polish

1. **Tap area.** `button.tag::before` and the tag-input remove button's `::before` use a 32 px
   tall hit area centred on the chip (WCAG 2.2 AA asks for at least 24×24 px), instead of 44 px,
   so it barely reaches into the card above.
2. **Tests.** The comma handling in `TagInput.svelte` moves into a pure `splitTyped(tags, value)`
   in `frontend/src/lib/tags.ts` returning `{ tags, text, error }`, with Vitest cases (single and
   multiple commas, trailing comma, a failing segment kept in the text with its error, dedupe).
   Playwright: Enter in an empty tag field submits the object form; typing a 33-character tag and
   pressing Enter shows the error, which is `aria-live="polite"` and linked by
   `aria-describedby`.

## C. Import and sync edges

1. **Import counts.** `ImportCounts` gains `types_created` and `types_merged` (an archive type
   mapped onto an existing type with the same name). Settings > Data shows them in the import
   message (en and de); `docs/openapi.json` documents the fields.
2. **Edits to deleted rows.** A sync `set` op whose row is tombstoned (`deleted_at IS NOT NULL`)
   is rejected with `"this item was deleted"`, for every entity with a whitelist (object,
   activity, reminder, attachment, object_type), checked before field clocks are read or written.
   The offline-sync spec's "Later additions" notes it. Tests per entity on SQLite and PostgreSQL.

## Out of scope

Rewriting old commit trailers; the unmaintained `paste` crate warning (a transitive dependency of
`image`).
