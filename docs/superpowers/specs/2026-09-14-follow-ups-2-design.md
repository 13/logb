# Follow-ups after 0.8.2

Status: approved, not implemented. Clears the leftovers deliberately deferred in the objects list,
tags, own types and offline cache projects.

## A. Offline polish

1. **Saved data shown while online.** `NetworkFirst` answers from `logb-api` when the network takes
   longer than 4 s. The page cannot see that today. `frontend/src/lib/api.ts` compares a successful
   response's `Date` header with the moment the request was sent; a response dated more than 60 s
   before that came from the service-worker cache. A `servingSaved` store is set true by such a
   response and false by the next fresh one; the top bar shows the existing
   "Offline — showing saved data" note when offline mode *or* `servingSaved` is true. A response
   without a `Date` header counts as fresh. (The server's HTTP stack sends `Date`; the plan checks.)
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
