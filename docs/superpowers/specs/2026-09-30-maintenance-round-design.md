# Maintenance round (2026-09-30)

After the UI overhaul (0.19–0.23) nothing user-visible is planned. This round clears the
dependency queue, the leftovers the performance and UI rounds wrote down, and the code-health
items the hardening and performance rounds both put off. The user said "do all" to that list.

Branch `maint-round` off `main` at `c75c5b2`. Wave 1 runs five tracks in parallel worktrees;
wave 2 (the file splits) starts once wave 1 is merged, because moving code while other tracks
edit it only produces conflicts.

## Ground rules

- No push, PR merge, tag or release without the user saying so. Dependabot PRs close themselves
  once `main` contains their bumps.
- API responses keep their shape unless a track below says otherwise. `docs/openapi.json` stays
  in step with the router (`tests/it/openapi.rs`).
- Migrations, if any track needs one: Track 4 owns SQLite `0031_*` / PostgreSQL `0022_*`.
  `tests/it/schema_parity.rs` and `src/copy.rs`'s table list must keep passing.
- Gates per track: `cargo clippy --all-targets -- -D warnings`, `cargo test` on SQLite and on
  PostgreSQL (`LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55440/postgres`, the
  `logb-perf-pg` container), and for frontend changes `npm run check`, unit tests, build, and the
  Playwright specs that cover what changed.
- Performance claims are measured with `examples/perf.rs` before and after, on SQLite at least.

## Track 1 — dependencies (branch `maint-deps`)

- Cargo: thiserror 2.0.21, aes-gcm 0.11, p256 0.14 (`src/push.rs` uses `to_encoded_point`, which
  0.14 renamed; the Dockerfile's `--locked` build must pass too).
- npm: the minor/patch group, `@types/node` 26 (CI runs Node 26).
- GitHub Actions majors: checkout 7, setup-node 7, docker/login 4, metadata 6, setup-buildx 4.
  Read each changelog for breaking inputs used in `ci.yml` and `release.yml`; `release.yml`
  still needs a prerelease-tag run before the next real release, which is the user's call.
- TypeScript 7 stays blocked (svelte-check 4.7.6 peers `^5 || ^6`): ignore `typescript` majors in
  `dependabot.yml` with a comment saying why.

## Track 2 — the fallback language loads on demand too (branch `maint-i18n`)

`en.ts` is in the entry chunk as the fallback; `de.ts` is already its own chunk. Make `en` a
chunk as well, so the entry carries no dictionary. `main.ts` already waits for the active
locale before mounting; the fallback for a missing key must still work (load `en` alongside a
non-English locale only if parity is not guaranteed by a test — prefer a test that both
dictionaries have the same keys). The service worker must still precache both, so an offline
start in either language works. Report the entry chunk's size before and after.

## Track 3 — import writes rows in batches (branch `maint-import`)

`src/api/export/import.rs` calls `record::record_create` once per row (about 4 s of SQLite's
11.8 s for 20k activities). Add a batched form in `src/sync/record.rs` (multi-row `INSERT`,
chunked below both databases' parameter limits) and use it from import, keeping the sync feed
identical: same entities, cursors and field clocks as the per-row path, which a test must pin.
Also batch the row `INSERT`s themselves where the import already knows every row up front.

## Track 4 — the reads the last round skipped (branch `maint-reads`)

- `/sync/bootstrap` (0.4–0.6 s, 1.5 MB for the benchmark data): profile, then cut what is
  avoidable (per-row queries, repeated serialization, fields the client ignores). Response
  shape unchanged unless the frontend is changed in step and the spec updated.
- `/stats` (~35 ms SQLite) and `/tags` (~29 ms): profile and fix what the profile shows.
- PostgreSQL timeline first page (6.0 ms vs 4.3 before the perf round): look, fix if cheap.

## Track 5 — metrics and a schema-checked API description (branch `maint-api`)

- `/metrics` in Prometheus text format, **off by default** (`LOGB_METRICS_TOKEN`: unset means
  the route answers 404; set means it answers only to `Authorization: Bearer <token>`). Request
  count and latency by matched route and status class, DB pool size/idle, write-lock wait,
  outstanding background work if cheaply available, build version. No new heavy dependency if a
  small hand-written exporter will do. Documented in the README and `docs/openapi.json`.
- `tests/it/openapi.rs` checks routes lexically. Add a check that responses match the documented
  schemas: for a representative request per documented `GET`/`POST` path, validate the JSON the
  integration harness receives against the schema in `docs/openapi.json` (a small validator or a
  dev-dependency such as `jsonschema`). Fix the document where it is wrong.

## Wave 2 — Track 6: split the large files (branch `maint-split`)

`src/sync/apply/mod.rs` (1,045 lines), `src/api/objects.rs` (1,190), `src/copy.rs` (735): split
along the seams already there (per entity, per concern), pure moves plus the `use` changes they
need, no behaviour change. One commit per file so each can be reviewed as a move
(`git diff --color-moved`).

## Not in this round

- Features: none is planned; the user will be asked.
- The real-phone check of the Save bar above the keyboard needs the user and a phone.
