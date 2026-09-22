# Land Personal Preferences, Then Clean Up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get the two uncommitted feature threads (calendar recurrence polish, per-account
appearance and digest hour) onto `main` as two clean commits verified against the full CI gate,
then pay down the three debts that work introduced or exposed.

**Architecture:** Phase 1 fixes four small defects found while reading the working tree, runs the
five CI gates locally (clippy, sqlite tests, postgres tests, frontend check/test/build, e2e), and
splits the tree into two commits by feature. Phase 2 is three independent tracks: a sync
re-validation helper plus module splits (backend), per-user notification timezone plus delivery-row
hygiene (feature completion), and one per-account appearance record on the frontend.

**Tech Stack:** Rust (axum, sqlx, sqlite + postgres), Svelte 5, Vitest, Playwright (Pixel 7 /
Chromium).

## State at plan time (2026-09-22)

- `cargo check --tests` clean; `cargo test` (sqlite) exit 0; `npm run check` 0 errors;
  `npm test` 522 passed in 40 files.
- Not yet run locally: `cargo clippy --all-targets -- -D warnings`, the postgres test job,
  `npm run build`, Playwright.
- Working tree: 34 modified files, 5 untracked (`frontend/src/lib/recurrence.ts`,
  `frontend/tests/recurrence.test.ts`, `migrations/{sqlite/0025,postgres/0016}_personal_preferences.sql`,
  `tests/personal_preferences.rs`).

## Global Constraints

- Both new migrations are unreleased, so editing them in place is allowed. Any migration already
  in a tagged release is immutable.
- `schema_parity.rs` must keep passing: every sqlite migration change needs the postgres twin.
- No new runtime or dev dependency in either phase.
- No copy change that alters an existing `aria-label` — e2e finds controls by accessible name.
- Phase 2 tracks must not change user-visible behaviour except where a task says so explicitly.

---

## Phase 1 — Land the in-flight work

### Task 1: Fix the four defects found in review

**Files:**
- Rename: `frontend/tests-e2e/31-calendar-reminders.spec.ts` → `32-calendar-reminders.spec.ts`
- Create: `frontend/tests-e2e/33-personal-preferences.spec.ts`
- Modify: `src/api/notifications.rs` (`out`, lines ~70-95)
- Modify: `migrations/sqlite/0025_personal_preferences.sql`, `migrations/postgres/0016_personal_preferences.sql`

**Interfaces:**
- Consumes: nothing.
- Produces: the file names Task 2's e2e run uses, and the query shape Phase 2 Task 6 builds on.

- [ ] **Step 1: Resolve the e2e number collision**

`31-water.spec.ts` already holds 31. Rename the new calendar spec to `32-` with `git mv` so the
rename is recorded rather than an add plus delete.

- [ ] **Step 2: Move the appearance assertions into their own spec**

The appearance-preference assertions currently live inside the calendar-reminders spec. Cut them
into `33-personal-preferences.spec.ts` (same helpers import, same login flow). A spec named after
the reminder feature must not fail when the preferences feature breaks.

Run: `cd frontend && npx playwright test 32-calendar 33-personal`
Expected: both specs pass, each covering only its own feature.

- [ ] **Step 3: Collapse the duplicate users SELECT**

`out()` runs `SELECT notify_url, notify_format FROM users`, then later
`SELECT notify_hour FROM users` against the same row. Fold `notify_hour` into the first
`query_as` and drop the second. Type becomes `(Option<String>, String, Option<i64>)`.

Run: `cargo test --test notifications --test personal_preferences`
Expected: pass, unchanged assertions.

- [ ] **Step 4: Index the column the history query filters on**

`notification_deliveries` is read by `WHERE user_id = $1` on every Settings → Notifications load
and has no index on `user_id` (`target` is the primary key). Append to both migration files:

```sql
CREATE INDEX notification_deliveries_user ON notification_deliveries (user_id);
```

Run: `cargo test --test schema_parity`
Expected: pass — the two dialect files stay in step.

---

### Task 2: Run the full CI gate locally

**Files:** none modified.

**Interfaces:**
- Consumes: Task 1's fixes.
- Produces: the green evidence Task 3 records in its commit.

- [ ] **Step 1: Backend lint and sqlite tests**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: no warnings, all suites pass. `cargo test` was green before Task 1; a failure here is
Task 1's doing.

- [ ] **Step 2: Postgres suite**

```bash
docker run --rm -d --name logb-pg -e POSTGRES_PASSWORD=logb -p 5432:5432 postgres:16
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:5432/postgres cargo test --all-targets --locked
docker rm -f logb-pg
```

Expected: same pass set as sqlite. This is the gate that catches a dialect-specific default or a
`BIGINT`/`INTEGER` mismatch in the new migration.

- [ ] **Step 3: Frontend**

Run: `cd frontend && npm run check && npm test && npm run build`
Expected: 0 errors, 522+ tests pass, build succeeds.

- [ ] **Step 4: End-to-end**

Run: `cd frontend && npx playwright test`
Expected: every spec passes, including the renamed 32 and new 33.

Record failures with their actual output before fixing anything. A failure here is information
about the feature, not a reason to weaken the test.

---

### Task 3: Commit in two feature-shaped commits

**Files:** the whole working tree, split.

**Interfaces:**
- Consumes: Task 2's green run.
- Produces: a clean tree for Phase 2.

- [ ] **Step 1: Stage and commit the recurrence thread**

`frontend/src/lib/recurrence.ts`, `frontend/tests/recurrence.test.ts`,
`frontend/src/lib/DateInput.svelte`, `frontend/src/lib/reminder-form.ts`,
`frontend/src/lib/Reminders.svelte`, `frontend/src/routes/ReminderForm.svelte`,
`frontend/tests/reminder-form.test.ts`, `frontend/tests-e2e/32-calendar-reminders.spec.ts`,
`src/api/reminders.rs`, `src/domain/reminder.rs`, `src/sync/apply.rs`, `src/api/export.rs`,
`src/api/objects.rs`, `tests/reminders.rs`, `tests/reading_reminders.rs`, `tests/sync.rs`, plus the
recurrence paragraphs of `README.md` and the reminder paths of `docs/openapi.json`.

Message: `feat: repeat reminders on calendar schedules`

- [ ] **Step 2: Stage and commit the preferences thread**

`migrations/*/`*`_personal_preferences.sql`, `tests/personal_preferences.rs`,
`src/api/settings.rs`, `src/api/notifications.rs`, `src/notify.rs`, `src/copy.rs`,
`tests/notify.rs`, `frontend/src/stores/settings.ts`, `frontend/src/stores/session.ts`,
`frontend/src/routes/settings/Appearance.svelte`,
`frontend/src/routes/settings/Notifications.svelte`, `frontend/src/lib/types.ts`,
`frontend/src/i18n/{en,de}.ts`, `frontend/tests/settings.test.ts`,
`frontend/tests-e2e/17-notifications.spec.ts`, `frontend/tests-e2e/33-personal-preferences.spec.ts`,
the preferences paragraphs of `README.md`, the `/me/*` paths of `docs/openapi.json`, and the
`Cargo.toml`/`Cargo.lock`/`package.json` version bumps.

Message: `feat: store appearance and digest hour per account`

- [ ] **Step 3: Verify the split holds on its own**

Run: `git stash && cargo check --tests && git stash pop` between the two commits, or check out the
first commit into a worktree and run `cargo test --test reminders --test sync`.
Expected: the first commit compiles and passes without the second. If it does not, the split is
wrong — move the offending hunk, do not merge the commits.

---

## Phase 2 — Pay down what this work exposed

Three independent tracks. They touch disjoint files and can run in any order or in parallel.

### Task 4: Extract the sync field re-validation helper

**Files:**
- Modify: `src/sync/apply.rs` (the reminder-field block at ~line 804)
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: Phase 1.
- Produces: the helper Task 5 moves into a submodule.

- [ ] **Step 1: Write the failing test first**

Add a `tests/sync.rs` case that pushes a field op for every name in the guarded list
(`schedule`, `every_n`, `every_unit`, `repeat_months`, `repeat_counter`, `due_counter`,
`due_date`) with a value the REST handler rejects, and asserts each comes back `Rejected`. Today
the block covers those names but the test suite exercises only some of them.

Run: `cargo test --test sync`
Expected: FAIL on whichever field is under-covered. Record which.

- [ ] **Step 2: Fold the two SELECTs into one and extract a function**

The block currently issues `SELECT ... FROM reminders WHERE client_uuid = $1` and a second
`SELECT o.counter_unit, o.type ... JOIN reminders`. One join answers both. Move the whole thing
to `async fn revalidate_reminder_field(tx, uuid, field, value) -> Result<Option<String>>`
returning the rejection reason, so `apply_op` reads as one call.

Keep the serde round-trip: it is what makes a partial field write reuse `ReminderInput::validate`
instead of duplicating validation rules. Add a comment saying so — the round-trip looks gratuitous
otherwise.

Run: `cargo test --test sync --test reminders`
Expected: pass, including the new cases.

---

### Task 5: Split the oversized backend modules

**Files:**
- Modify: `src/sync/apply.rs` (1515 lines), `src/api/activities.rs` (1334),
  `src/api/export.rs` (1166), `src/api/objects.rs` (1079), `src/api/reminders.rs` (970)

**Interfaces:**
- Consumes: Task 4's helper.
- Produces: nothing later tasks rely on.

- [ ] **Step 1: Split one module, verify, then move to the next**

One module per step, in this order (largest risk-adjusted payoff first): `sync/apply.rs`,
`api/export.rs`, `api/activities.rs`. Each split is a pure move: `pub(crate)` where a symbol
crosses the new boundary, no logic edit in the same commit.

Suggested seams:
- `sync/apply.rs` → `apply/mod.rs` (dispatch), `apply/validate.rs` (Task 4's helper and the
  field guards), `apply/tags.rs` (tag normalisation).
- `api/export.rs` → `export/write.rs` and `export/import.rs` — `validate_import` and its tagging
  helpers are a separate concern from archive writing.
- `api/activities.rs` → handlers vs. the query builders behind them.

Run after each split: `cargo clippy --all-targets -- -D warnings && cargo test --locked`
Expected: identical pass set. A diff that changes behaviour during a move is a bug in the move.

- [ ] **Step 2: Stop at the point of diminishing return**

`api/objects.rs` and `api/reminders.rs` are under 1100 lines and cohesive. Split them only if
Step 1 reveals a seam that removes duplication, not to hit a line count.

---

### Task 6: Finish the notification feature

**Files:**
- Modify: `migrations/sqlite/00xx_notify_timezone.sql`, `migrations/postgres/00xx_notify_timezone.sql` (new)
- Modify: `src/notify.rs` (`tick`, `recipients`, `claim_delivery`), `src/api/notifications.rs`
- Modify: `frontend/src/routes/settings/Notifications.svelte`, `frontend/src/i18n/{en,de}.ts`
- Test: `tests/notify.rs`, `tests/personal_preferences.rs`

**Interfaces:**
- Consumes: Phase 1's `users.notify_hour`.
- Produces: nothing later tasks rely on.

- [ ] **Step 1: Failing test for a user in another timezone**

`tick(state, hour_now)` compares `hour_now` — the instance's hour — against the user's chosen
hour. A per-account hour without a per-account timezone means a user abroad gets the digest at the
wrong local time. Add a `tests/notify.rs` case: user with `notify_tz = "America/New_York"` and
`notify_hour = 8`, instance in UTC, assert the digest fires at 13:00 UTC and not at 08:00.

Run: `cargo test --test notify`
Expected: FAIL — fires at 08:00 UTC today.

- [ ] **Step 2: Add `users.notify_tz` and resolve the hour per recipient**

New nullable `TEXT` column in both dialects, validated by parsing into `chrono_tz::Tz` on write
(`chrono-tz = "0.10.4"` is already a dependency, and `db::timezone()` already returns a `Tz`).
`tick` computes each recipient's local hour instead of comparing against `hour_now` directly; the
instance webhook keeps using `LOGB_NOTIFY_HOUR` in the instance timezone.

Decide explicitly and write it in the README: whether a per-user timezone also moves that user's
notion of "due today". `db::today()` is instance-wide, so leaving due-date evaluation alone means a
user in Auckland gets their 08:00 digest listing the instance's today. Recommended: move only the
delivery hour in this task and note the limitation, since changing due-date evaluation per user
touches reminders, stats and export.

Run: `cargo test --test notify --test personal_preferences`
Expected: pass.

- [ ] **Step 3: Prune delivery rows that outlived their target**

`notification_deliveries` keys rows by `push:{id}`, and `push_to`/`tick` delete a subscription when
the push service says it is gone — the delivery row stays forever. Delete the matching row in the
same transaction as the subscription delete, and drop rows whose `day` is older than 30 days during
`tick`. Assert both in `tests/notify.rs`.

Run: `cargo test --test notify`
Expected: pass, with the table empty of stale targets after a gone-subscription run.

- [ ] **Step 4: Surface the timezone in Settings → Notifications**

A select beside the hour, defaulting to the instance timezone, with both locales' copy. Extend
`frontend/tests-e2e/33-personal-preferences.spec.ts` rather than adding a spec.

---

### Task 7: One appearance record per account on the frontend

**Files:**
- Modify: `frontend/src/stores/settings.ts`, `frontend/src/stores/session.ts`
- Test: `frontend/tests/settings.test.ts`

**Interfaces:**
- Consumes: Phase 1.
- Produces: nothing later tasks rely on.

- [ ] **Step 1: Pin current behaviour in tests before touching it**

Today the module holds three localStorage keys (`logb.appearance-overrides`,
`logb.appearance-accounts`, `logb.appearance-devices`) plus module-level `owner` and a `revision`
counter that guards against a slow `GET /me/appearance` overwriting a newer local edit. Write tests
for the behaviours that must survive: a late GET does not clobber a change made while it was in
flight; a device override survives logout and login; switching accounts on one device does not leak
the other account's appearance; the first login on a fresh device adopts the pre-login settings.

Run: `cd frontend && npm test -- settings`
Expected: pass against the current implementation. These tests are the contract for Step 2.

- [ ] **Step 2: Collapse to one keyed store**

Replace the three keys with `logb.appearance` holding
`Record<string, { account: LocalSettings | null; device: LocalSettings | null; override: boolean }>`,
and replace the `revision` counter with comparing the in-flight request's account id and the
settings value captured when the request was issued. Keep `settings` itself as the single store the
UI subscribes to.

Run: `cd frontend && npm test && npx playwright test 33-personal`
Expected: the Step 1 tests pass unchanged. If one has to change to make the refactor work, the
refactor changed behaviour — stop and report which.

---

## Out of scope

- Exporting per-account preferences in the JSON archive. The archive carries no user settings at
  all today (`notify_url`, `notify_format` and `lang` are absent too), so preferences are consistent
  with it. Changing that is its own feature.
- Any refactor of `api/stats.rs`, `domain/insights.rs` or the trip/charging modules. Untouched by
  this work.
