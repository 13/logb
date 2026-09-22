# Improvements Round: Search Folding, Per-User Today, File Splits, Hygiene

Date: 2026-09-22. State at design time: `main` at `e3b587d`, working tree clean, clippy clean,
sqlite and postgres suites green, 526 frontend unit tests green, `npm audit` clean. The
2026-09-22 "land preferences and clean up" plan is fully landed, including its Phase 2.

Four independent tracks. Each is its own commit series and can be reordered or dropped without
affecting the others, except that Track D's README edit for timezone semantics depends on
Track B.

Suggested order: D (dependency bumps, small and establishes the baseline) → A → C tests split →
B → C frontend → D docs.

## Global constraints

- No new runtime or dev dependency.
- `tests/schema_parity.rs` stays green. No track adds a migration.
- No copy change that alters an existing `aria-label`; e2e finds controls by accessible name.
- Pure moves (Track C) change no behaviour. A test that has to change during a move means the
  move changed behaviour: stop and report which.
- Each track leaves `cargo clippy --all-targets -- -D warnings`, `cargo test`, the postgres
  suite, `npm run check`, `npm test`, `npm run build` and Playwright green.

---

## Track A: search folds case and accents on both backends

### Problem

`GET /api/search` matches with `LIKE` on sqlite (ASCII case only) and `ILIKE` on postgres, so
`olwechsel` finds `Ölwechsel` on one backend and not the other. `tests/dialect.rs:45`
(`only_postgresql_folds_the_case_of_accented_letters`) pins the divergence as expected
behaviour. The objects list on the client already folds accents (`object-list.ts:17`), so the
list and the search page disagree today.

### Design

Replace the LIKE predicates with a user-scoped scan and a Rust fold, the pattern
`api/activities/query.rs:122-158` and `api/types.rs:117` already use:

- Both statements keep their `SELECT` list, joins, `user_id`/`deleted_at` guards and `ORDER BY`.
  They lose the `{like} $2` clauses, the `LIMIT`/`OFFSET` binds, and the `match_tags`
  JSON-punctuation special case.
- A private `fn matches(term_folded: &str, fields: &[Option<&str>], tags_json: &str) -> bool`
  folds each candidate field with `domain::tags::fold` and tests `contains`. Tags are parsed
  from their JSON text and folded one by one, so a term with `[` or `"` no longer needs
  excluding.
- Objects: `name`, `description`, tags. Activities: `title`, `notes`, `from_place`, `to_place`,
  tags. Same columns as today; `type` and `category` stay excluded for the reasons at
  `search.rs:107-117`.
- Pagination applies after the filter: `skip(offset).take(limit + 1)`, `has_more` from the
  extra row, exactly as now.
- `like_pattern` and its two unit tests go. `Backend::case_insensitive_like` goes from
  `src/dialect.rs`, its unit test and the module doc's item 1 with it. `tests/search.rs:167`'s
  doc comment is reworded.

Cost: one household's live rows per search request. Same shape the timeline filter accepts.
The README's Search paragraph drops the `olwechsel`/`Ölwechsel` caveat and says matching
ignores case and accents on both backends.

### Tests

- `tests/dialect.rs`: `only_postgresql_folds_the_case_of_accented_letters` becomes
  `search_folds_accents_on_both_backends`: `ölwechsel`, `olwechsel`, `ÖLWECH` each find the
  stored `Ölwechsel`; the postgres self-check query is deleted.
- `tests/search.rs`: new case: a tag `Fahrräder` is found by `fahrrader`; a term containing `[`
  finds nothing rather than every tagged row (the guarantee the old special case gave).
- Existing wildcard test keeps passing: `%` and `_` are ordinary characters under `contains`.

---

## Track B: due dates read against each user's own today

### Problem

`users.notify_tz` moves the digest hour into the user's zone, but `db::today()` still decides
which reminders are due, so a user far east or west gets the digest for the instance's day.
README 504-506 states this limitation.

### Scope

In: reminder due evaluation and the digest's day. Out: stats and insights month buckets, trips
summary default date, export and backup file names, `last_used_at` day guard, delivery-history
pruning cutoff. Those stay on the instance day; the README says so.

### Design

**Effective zone.** `db::today_in(tz: Option<&str>) -> String`: parse `tz` as `chrono_tz::Tz`,
fall back to `db::timezone()` on `None` or a parse failure, return the date. `db::today()`
becomes `today_in(None)`. `notify::local_hour`'s inline parse uses the same fallback.

**Who carries the zone.** `AuthUser` gains `pub tz: Option<String>` with `#[serde(skip)]` and
`#[sqlx(default)]`, selected as `u.notify_tz` in the five queries that build one
(`auth.rs:309, 344, 371`; `api/auth.rs:217, 275`). `AuthUser::today(&self) -> NaiveDate`
wraps `db::today_in`. The column keeps its name; the API already exposes it as `timezone`.

**Reminders (`src/api/reminders.rs`).** The module-local `fn today()` is deleted. `impl
From<ReminderRow> for ReminderOut` is deleted; its three unit-test callers use
`ReminderOut::build(row, today, None)` with an explicit date. Every handler passes
`user.today()`:

- `reading_horizon()` becomes `reading_horizon(today: NaiveDate) -> String`; the six
  `select_reminders` bind sites and `insights::usage_by_object` pass the caller's today.
- `out`, `list`, `due_for_user` take `today: NaiveDate`. `due_for_user(state, user_id, within,
  today)` is the seam the digest uses.
- `ReminderInput::validate` takes `today` for the two start-date defaults.
- `done` and `snooze` use `user.today()` for their fallbacks and clamps.

**Objects (`src/api/objects.rs`).** `derived` and `due_readings` take `today: NaiveDate` and
bind it as `$2`/`$4` in place of `db::today()`/`reading_horizon()`. Callers pass `user.today()`.
The INVARIANT comment at `objects.rs:402` gains one line: both encodings now read the same
caller-supplied date.

**Digest (`src/notify.rs`).** `items_for(state, r)` computes `db::today_in(r.notify_tz)` and
passes it to `due_for_user`. `claim_delivery` takes the recipient's day so "once per destination
per day" is the recipient's day; the instance webhook keeps the instance day. The
`local_hour` doc paragraph about the instance's day is replaced by one stating the new rule.

**Frontend.** No change: the client never compares `due_date` against a device date; it renders
the server's `due` and `days_until`.

**Docs.** README Time section: a user who has chosen a timezone under Settings → Notifications
has their reminders' due dates read against today in that zone; everything else on the instance
day. README 504-506 rewritten. `docs/openapi.json` descriptions for `due`, `days_until` and
`NotificationSettings.timezone` say which today applies.

### Tests

New cases in `tests/personal_preferences.rs` (per-request zone, so no process-wide static and
no race):

- Instance UTC, user `Pacific/Kiritimati` (UTC+14), reminder due on the instance's tomorrow:
  `GET /reminders/due` says `due: true` for that user and `days_until: 0`; the object's
  `due_reminder_count` is 1. A second user with no zone sees `due: false`, count 0.
- Same fixture, user `Pacific/Pago_Pago` (UTC−11), reminder due on the instance's today: not
  due for that user, due for the zone-less user.
- Reading horizon follows the user: a reading dated the user's tomorrow counts, the user's day
  after tomorrow does not.
- `tests/notify.rs`: user in UTC+14 with a reminder due on the instance's tomorrow and a
  delivery hour that has already passed in their zone receives a digest listing it; the
  delivery row's `day` is the user's day.
- `tests/objects.rs::due_reminder_count_agrees_with_each_reminders_due_flag` runs once more
  with a zoned user.

Existing tests that read `logb::db::today()` or `Utc::now().date_naive()` keep passing: the
test harness leaves the instance on UTC and no existing test sets `notify_tz`.

---

## Track C: split oversized files by pure moves

### `tests/sync.rs` → `tests/sync/`

97 tests, 14 helpers, no section comments. Becomes one test binary:

```
tests/sync/main.rs        mod declarations; `#[path = "../common/mod.rs"] mod common;`
tests/sync/helpers.rs     after_now, before_now, png, push_body, client_uuid, object_uuid,
                          object_with_children
tests/sync/read_paths.rs  lines 22-616: revalidation, invariants, REST delete, read paths after
                          delete (+ export_object)
tests/sync/push.rs        639-1289: core push, timestamps, whitelist, ownership, batch, FK value
tests/sync/validation.rs  1391-1979: cross-user join, delete op, value validation and batch
                          isolation
tests/sync/pull.rs        1980-2552: cursor, paging, horizon, gone
tests/sync/bootstrap.rs   2553-2717 and 4236-4407: bootstrap, epoch
tests/sync/purge.rs       2718-3003 and 3503-3656: purge, orphaned field_clock sweep
tests/sync/cascade.rs     3004-3502 and 3657-3997: cascades, file delete, op shape
tests/sync/rest.rs        3998-4235 and 4408-4658: REST↔sync interplay, cycles, cover, done
tests/sync/tags_types.rs  4667-5227: tags, custom types (+ type_create_op)
tests/sync/deleted.rs     5228-5509: set on a deleted row (+ its three helpers)
tests/sync/trips.rs       5510-6089 (+ create_trip)
tests/sync/energy.rs      6090-6542 (+ create_charge)
```

Each module starts with `use super::helpers::*;`, `use super::common;` and the two original
`use` lines. Nothing else changes. `tests/sync.rs` is deleted in the same commit, since cargo
would otherwise see two targets named `sync`. Verification: `cargo test --test sync` lists 97
tests before and after.

`tests/common/mod.rs` stays as it is: one harness, already grouped by concern.

### `frontend/src/routes/ActivityForm.svelte` → `lib/activity-form.ts`

Move as pure functions, each with a unit test in `frontend/tests/activity-form.test.ts`:

- `buildActivityInput(input, texts, object, chargedFull, t)` from `buildInput` (320-362); `t`
  passed in, the precedent `activityTitle` sets.
- `activityToFormText(a, weightUnit)` from the hydration mapping (148-159).
- `changeWeightUnit(state, next)` returning the new tuple (310-318).
- `resolveCategory(offered, wanted, current)` replacing the two copies at 124-134 and 180-187.
- `counterBelowLast`, `weightDeviates`, `firstExifDate` from the derived warnings (79-88).
- `parseCategoryParam(search)`; the `location.search` read stays in the component.
- `optimisticActivity(tempId, oid, body)` from the literal at 295-305.
- `hashToNegativeId(s)` from `mintTempId` (261-266), shared with ObjectDetail.

### `frontend/src/routes/ObjectDetail.svelte` → new `lib/object-detail.ts`

`pendingToActivity(op, oid)`, `hashToNegativeId` (same function as above, one definition),
`foldTitle`, `filterPendingOps(ops, filters)`, `tagParam(search)`, `offersTrip`,
`offersEnergy`, `resourceCategory`, `nextUrl(href, tab)`. New
`frontend/tests/object-detail.test.ts` covers each; `pendingToActivity`'s typeof guards get one
case per guarded field.

### `frontend/src/lib/api.ts` → `api.ts` + `api-outbox.ts`

Lines 303-778 (store seam, queued writes, flush, queue reads, queued-op mutation, dead letters)
move to `api-outbox.ts`, which imports `api`, `handle` (made exported) and `isOurs`/`setOutboxUser`
from `api.ts`. `api.ts` ends with `export * from './api-outbox'` so no call site changes.
`frontend/tests/outbox-replay.test.ts` imports from `api-outbox` directly;
`frontend/tests/api.test.ts` is untouched. `vi.mock('../src/lib/api')` in
`type-registry.test.ts` keeps working because it mocks `api` only.

---

## Track D: docs and dependency hygiene

### Docs

- New `docs/upgrading.md` holding README's "Upgrading to 0.3.0" section (lines 51-86) and the
  weight-tracking upgrade note (lines 143-145), each under a version heading. README keeps a
  one-line pointer under "Released images". Release notes stay auto-generated by
  `softprops/action-gh-release`; no CHANGELOG file.
- README Time and Reminder notifications sections updated for Track B (see there). README
  Search paragraph updated for Track A.

### Dependencies

- `cargo update` (semver-compatible only; `--dry-run` shows ~25 patch bumps). Commit
  `Cargo.lock` alone.
- npm: `vite` 8.3, `@playwright/test` 1.63, `svelte` patch, `@types/node` to the wanted 24.x.
  One commit.
- `vitest` 5 in its own commit: bump, run `npm test` and `npm run check`; if a fix is not
  obvious within the commit, revert the bump and note why in the plan.
- Not done: TypeScript 7 (`svelte-check` 4.7 peers `^5 || ^6`), `@types/node` 26 (CI runs
  node 26 but the types major brings no needed API), Rust edition 2024 (not asked, churn
  without payoff).

---

## Out of scope

- Denormalised search columns or a postgres extension.
- Per-user timezone for stats, insights, trips, export names, delivery-history pruning.
- Splitting `tests/common/mod.rs`, `api/objects.rs`, `api/reminders.rs`.
- Updating the ~30 `lib/api` import sites to `api-outbox`.
- Route-level code splitting: the bundle is 108 KB gzipped and precached by the service worker.
