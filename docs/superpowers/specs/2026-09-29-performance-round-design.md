# Performance round — design

Date: 2026-09-29. Base: `main` at 0.17.1 (`f232e6d`). Branch: `perf-round`.

The user asked for a plan to "improve speed, performance and so on", then said "do all" to it.
The plan came from three read-only audits (database, HTTP/files/build, frontend). This spec
records what each track changes and what it must leave true.

## Already on `perf-round` before the tracks start

- `[profile.dev] debug = "line-tables-only"`, dependencies at `opt-level = 2`, `image` without
  its default features (no AVIF encoder, rayon or EXR; AVIF decoding was never enabled).
- The integration tests are one binary, `tests/it`, plus `tests/timezone` for the tests that
  change the process-wide instance timezone. `target/` went from 48 GB to 2.2 GB, and the
  SQLite suite runs in about 37 s.

## Rules for every track

- Both backends. Every change is tested on SQLite and on PostgreSQL
  (`LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55440/postgres`).
- The write-lock invariant stays true (see `docs/superpowers/specs/2026-09-22-serialized-sqlite-writes-design.md`): nothing between `begin_write` and its commit acquires a second pooled
  connection, reads come from `state.db`, and slow work (file IO, decoding, hashing, network)
  stays outside write transactions.
- With `synchronous = NORMAL`, anything that deletes a file because a committed row stopped
  naming it calls `db::sync_committed` under the write lock first.
- API responses do not change shape unless this spec says so. `docs/openapi.json` stays in step
  with the router (`tests/it/openapi.rs`).
- Migration numbers are assigned here so the tracks do not collide:
  - Track A: SQLite `0029_perf_indexes.sql`, PostgreSQL `0020_perf_indexes.sql`.
  - Track C: SQLite `0030_search_text.sql`, PostgreSQL `0021_search_text.sql`.
  - Track B, only if it needs one: SQLite `0031_*`, PostgreSQL `0022_*`.
  - `tests/it/schema_parity.rs` and `src/copy.rs`'s table list must keep passing.

## Track 0 — measurement

`examples/perf.rs`: seeds a scratch instance (about 500 objects, 20k activities, 2k attachments,
reminders, tags) on SQLite or PostgreSQL. It times the hot endpoints over several runs and prints
median and p95: object list (`all=true`), one object's page, timeline pages, search (common and
rare terms), `/stats`, insights, sync bootstrap and pull, export, import. The baseline is taken
on `perf-round` before any track merges and again after all have merged. The frontend is measured
by bundle size and by the number of sequential requests before the dashboard shows data.

## Track A — backend quick wins and ops

- SQLite pragmas: `cache_size = -32000`, `mmap_size = 268435456`, `temp_store = MEMORY`;
  `PRAGMA optimize` on the hourly tick.
- `mimalloc` as the global allocator (the release binary is static musl).
- Migration: partial indexes on `deleted_at` (`WHERE deleted_at IS NOT NULL`) for objects,
  activities, reminders, attachments and object_types; an index on `reminders(done_activity_id)`.
- Purge: sweep only the `field_clock` rows of uuids purged in the same run; `NOT EXISTS` in place
  of `NOT IN (SELECT file_id ...)`; the orphan sweep unlinks files after its transaction ends
  (having called `sync_committed` first); `prune_delivery_history` moves to the hourly prune.
- Bearer tokens: `last_used_at` is read in the lookup, and the `UPDATE` runs only when it is stale.
- Files: originals are not recompressed (PDF, Office and other already-compressed types), and
  they answer `Range` requests. SHA-256 of an upload or imported blob runs off the async runtime.
- One shared `reqwest::Client` for push, Telegram and the webhook notifier; the Telegram key is
  read once, not per call.
- Compression: brotli and zstd besides gzip. Embedded assets are served without copying, with a
  strong ETag and 304 for `index.html`/`sw.js`, and `workbox-*.js` gets the immutable header.
- Blocking filesystem calls in handlers and the tick move to `spawn_blocking`.
- Dockerfile: dependency layer caching (cargo-chef or BuildKit cache mounts).
- Optional: `reqwest` on `rustls` with `ring` rather than aws-lc, if it builds for every release
  target.

## Track B — hot queries

- Object list `derived` (`src/api/objects.rs`): SQL built per case instead of
  `($1 IS NULL OR ...)`, one aggregate pass over activities (`GROUP BY object_id`) in place of the
  correlated subqueries, computed only for the ids returned. Same for `usage_by_object` and
  `due_readings`.
- `recent_titles`: a window function in place of four correlated subqueries per group.
- `record_create` and `record_update`: `field_clock` written with one multi-row upsert per entity.
  `log_cascade` batched the same way.
- Sync push: an op-count cap per request, above the largest batch the client sends. Documented in
  OpenAPI.
- Export: three queries for the whole user in place of three per object, and JSON written
  compactly inside the existing `spawn_blocking`.
- Insights: fewer sequential queries (merge passes or run them concurrently on the read pool).
- Activities list: one fetch for the page and its count; attachments grouped by id.
- Stats: newest fuel level per object in SQL, the year filter in SQL.

## Track C — search

- A stored `search_text` column on objects and activities: the folded text (the same NFD folding
  `search.rs` does today), written by Rust on every write path — REST create and update, sync
  apply, import, copy between engines — and backfilled at startup for rows where it is NULL.
- The query matches with `LIKE` on `search_text`, paged in SQL, so results and paging are the same
  as today's.
- PostgreSQL: a `pg_trgm` GIN index when the extension can be created, falling back to a plain
  scan when it cannot. SQLite: an FTS5 `trigram` table kept in step with the column, when the
  SQLite build has it. Terms shorter than three characters fall back to `LIKE`.
- Results must be identical to today's for every existing search test.

## Track D — frontend

- Startup: `/auth/status` and `/auth/me` in parallel (or answered from the remembered profile
  first), IndexedDB reads alongside the network, and the dashboard's reminders fetched in the same
  `Promise.all` as its objects. The service worker's `networkTimeoutSeconds` drops from 4 to 2.
- Settings: its loads run in parallel.
- Object page: the hero image uses the thumbnail, with `decoding="async"`.
- Routes load lazily; the second locale loads on demand.
- Formatters cached per locale and options; one collator per sort; dashboard filter debounced,
  its query normalised once.
- Dashboard: archived list fetched only on its tab (count from what is already known), and a
  snooze updates the list in place.
- Stats: energy, fuel and water fetched once per mount; `/api/stats/water` cached for offline.
- Service worker: separate caches for thumbnails and originals.
- Outbox: counts kept in a store, not a full `getAll()` per TopBar mount; a flush with an empty
  queue returns at once.
- Smaller: search's `replaceState` inside the debounce and aborting stale requests,
  `loading="lazy"` and `decoding="async"` on thumbnails, the Notifications page's 2 s timer only
  while a link is pending, `foldReadings` as a `$derived`.

## Deliberately left out

- `panic = "abort"`: a panic in one handler would take the server down.
- Timeline virtualisation: only matters past about a thousand visible entries.
- Precompressed embedded assets: compression runs once per response on a 113 KB gzip bundle,
  and the service worker keeps repeat visits off the network.

## Track C — decisions

- **What is stored.** `objects.search_text` and `activities.search_text` (nullable TEXT, SQLite
  0030 / PostgreSQL 0021) hold exactly what the old per-request loop folded: object `name`,
  `description` and each tag; activity `title`, `notes`, `from_place`, `to_place` and each tag.
  Each field is folded by `domain::tags::fold` and the fields are joined by U+001F, so a hit
  still has to lie inside one field. A term containing U+001F (or a NUL, which PostgreSQL
  refuses) answers nothing without a query. Tags live as JSON in the row itself, so there is no
  tag rename to follow; `type` and parent/object names are not searched, so renaming a type or
  an object refreshes nothing else.
- **Who writes it.** `src/search_text.rs` is the one place that knows the fields. Every write
  path calls `refresh_objects`/`refresh_activities` inside its write transaction, reading back
  what it stored: REST create and update of objects and activities, a sync `set` of a searched
  field, and an import (one pass over the user's still-NULL rows before commit). Deletes and
  cascades only set `deleted_at`, which search already filters. `--copy-to` copies the column
  like any other (the fold is the same Rust on both sides) along with the `settings` row.
  Writes are batched (`UPDATE ... FROM (VALUES ...)`, 500 rows per statement) and skipped when
  the text did not change. Each path has a test in `tests/it/search.rs` (copy in
  `tests/it/copy.rs`).
- **Backfill and versioning.** At startup, after the migrations, rows with NULL are folded 500 at
  a time, one short write transaction per batch. `search_text::FOLD_VERSION` is recorded in
  `settings` (`search_fold_version`); when it differs, the column is cleared first and refilled,
  so changing the fold or the field list is a constant bump.
- **Query.** Same order, paging and `has_more` as before, now in SQL: objects unarchived first
  then by name (ties now broken by id rather than left to the database), activities by date and
  id descending, `LIMIT limit+1 OFFSET offset` per list. The predicate is `dialect::contains`:
  `instr(search_text, $2) > 0` on SQLite -- exact, and without `LIKE`'s ASCII case folding and
  its 50 000-byte pattern limit -- and `search_text LIKE $2 ESCAPE '\'` with `\`, `%`, `_`
  escaped on PostgreSQL, the form `pg_trgm` can serve.
- **Acceleration.** PostgreSQL: `CREATE EXTENSION IF NOT EXISTS pg_trgm` and GIN
  `gin_trgm_ops` indexes on both columns, from Rust at startup rather than in a migration, so a
  refused extension is an INFO line and a plain scan instead of a server that will not start.
  Not in the schema, so `schema_parity` does not see it. With 20k activities the activities
  query's scan takes about 10 ms; the index takes a rare term to about 1 ms, and the planner
  scans as before for terms under three characters. SQLite: no FTS5. The scan of one narrow
  column over one user's rows is about 10 ms at 20k activities (`instr` and `LIKE` measured the
  same), and FTS5 would add a second copy of the text, shadow tables that `copy::TABLES` and
  `schema_parity` would have to learn about, and a second place to keep in step.
- **Timings** (debug build, `GET /search` end to end, 500 objects and 20k activities, median of
  15): SQLite 176-290 ms before, 3-12 ms after; PostgreSQL 116-230 ms before, 2.5-21 ms after
  (with the trigram index and fresh statistics). The one-off backfill of those 20.5k rows took
  0.4 s on SQLite and 2 s on PostgreSQL.
