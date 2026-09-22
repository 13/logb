# Improvements Round Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the four tracks of `docs/superpowers/specs/2026-09-22-improvements-round-design.md`:
search that folds case and accents on both backends, reminder due dates read against each
user's own today, the oversized test and frontend files split by pure moves, and dependency
plus README hygiene.

**Architecture:** Track A replaces the SQL `LIKE` predicates in `api/search.rs` with a
user-scoped scan filtered by `domain::tags::fold`. Track B adds `db::today_in(tz)` and a `tz`
field on `AuthUser`, then threads a `NaiveDate` through every due computation that used the
process-wide `db::today()`. Track C moves code without changing it: `tests/sync.rs` into a
`tests/sync/` directory, pure functions out of two Svelte routes into `lib/*.ts`, and the outbox
half of `lib/api.ts` into `lib/api-outbox.ts` behind a re-export. Track D bumps dependencies and
moves stale upgrade notes out of the README.

**Tech Stack:** Rust 1.97 (axum 0.8, sqlx 0.9 sqlite + postgres via `Any`, chrono-tz), Svelte 5,
TypeScript 6, Vitest, Playwright.

## Global Constraints

- No new runtime or dev dependency in any track.
- No migration is added; `tests/schema_parity.rs` stays green.
- No copy change that alters an existing `aria-label`; e2e finds controls by accessible name.
- A pure move (Track C) changes no behaviour. If a test must change to make a move compile or
  pass, the move changed behaviour: stop and report which.
- Every task ends green on: `cargo clippy --all-targets --locked -- -D warnings`,
  `cargo test --locked`, the postgres suite (`LOGB_TEST_DATABASE_URL=... cargo test --all-targets
  --locked` against a `postgres:16` container), and for frontend tasks `npm run check`,
  `npm test`, `npm run build`. Playwright (`npx playwright test`) runs once per track at the end.
- Commit messages: `type: sentence`, lower case, no trailing period, as in the log.

Suggested order: Tasks 1-3 (deps) → 4-5 (search) → 6 (sync test split) → 7-11 (per-user today)
→ 12-14 (frontend splits) → 15 (upgrade docs). Tracks are independent; the order only keeps
merge conflicts away.

---

## Track D (part 1): dependency bumps

### Task 1: cargo update

**Files:**
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: nothing.
- Produces: the lockfile every later `--locked` run uses.

- [ ] **Step 1: Bump semver-compatible crates**

Run: `cargo update`
Expected: ~25 `Updating` lines (async-compression, clap, encoding_rs, quinn, ...). No `Adding`
or `Removing` lines. If one appears, `git checkout Cargo.lock` and report it.

- [ ] **Step 2: Gate**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: no warnings, every suite `test result: ok`.

- [ ] **Step 3: Commit**

```bash
git add Cargo.lock
git commit -m "chore: update crates within their semver ranges"
```

### Task 2: npm minor and patch bumps

**Files:**
- Modify: `frontend/package.json`, `frontend/package-lock.json`

- [ ] **Step 1: Bump to the wanted versions**

```bash
cd frontend
npm install --save-dev vite@8.3 @playwright/test@1.63 svelte@5.57.1 @types/node@24.13.6
```

Expected: `package.json` shows `"vite": "^8.3.0"`, `"@playwright/test": "^1.63.0"`,
`"svelte": "^5.57.1"`, `"@types/node": "^24.13.6"`. `npm outdated` afterwards lists only
`typescript` (7 blocked by svelte-check's peer range `^5 || ^6`), `vitest` (Task 3) and
`@types/node` 26 (not taken).

- [ ] **Step 2: Gate**

Run: `npm run check && npm test && npm run build && npx playwright install chromium`
Expected: 0 errors, 526 tests pass, build succeeds. Playwright 1.63 needs its browser
re-downloaded once; the install step does that.

- [ ] **Step 3: Commit**

```bash
git add frontend/package.json frontend/package-lock.json
git commit -m "chore: bump vite, playwright, svelte and node types"
```

### Task 3: vitest 5 (gated)

**Files:**
- Modify: `frontend/package.json`, `frontend/package-lock.json`

- [ ] **Step 1: Bump**

```bash
cd frontend && npm install --save-dev vitest@5
```

- [ ] **Step 2: Gate**

Run: `npm run check && npm test`
Expected: 526 tests pass. Read vitest's 5.0 release notes only if something fails.

- [ ] **Step 3: Commit or revert**

Green:
```bash
git add frontend/package.json frontend/package-lock.json
git commit -m "chore: move to vitest 5"
```

Red with a fix that is not obvious in the failing test's own file:
```bash
git checkout frontend/package.json frontend/package-lock.json && npm ci
```
and add one line under "Not done" in the spec's Track D section saying what failed.

---

## Track A: search folds case and accents

### Task 4: fold in Rust

**Files:**
- Modify: `src/api/search.rs` (whole `search` fn, `like_pattern`, module tests, doc comment)
- Test: `tests/dialect.rs:34-73`, `tests/search.rs`

**Interfaces:**
- Consumes: `crate::domain::tags::fold(&str) -> String` (NFD, strip combining marks, lowercase).
- Produces: `GET /search` unchanged in shape; matching is now fold-based on both backends.

- [ ] **Step 1: Rewrite the dialect test that pins the bug**

Replace `tests/dialect.rs` lines 34-73 (the doc comment and
`only_postgresql_folds_the_case_of_accented_letters`) with:

```rust
/// Accents fold the same way on both backends: the term and the stored text are both run
/// through `domain::tags::fold` in Rust, so neither SQLite's ASCII-only `LIKE` nor the
/// PostgreSQL cluster's collation decides the answer.
#[tokio::test]
async fn search_folds_accents_on_both_backends() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_activity(&object["id"], "Ölwechsel").await;

    for term in ["ölwechsel", "olwechsel", "ÖLWECH", "olwech"] {
        let results = app.search(term).await;
        assert_eq!(
            hits(&results, "activities"),
            1,
            "{:?} searching {term} for \"Ölwechsel\": {results}",
            app.state.backend
        );
    }
}
```

Also update the module doc at the top of the file: delete the sentence "The one place the two
genuinely cannot be made to agree is pinned per backend below, so that it stays a known
difference instead of a surprise." Keep `use logb::dialect::Backend;` only if another test in
the file still uses it (`objects_sort_without_regard_to_case` does not; check with
`grep -n Backend tests/dialect.rs`; remove the import if unused).

- [ ] **Step 2: Add the tag and punctuation cases to tests/search.rs**

Append to `tests/search.rs`:

```rust
/// A tag matches through the same fold as everything else, and a term made of JSON
/// punctuation matches no tag rather than every tagged row (the guarantee the old
/// `match_tags` special case gave).
#[tokio::test]
async fn tags_are_found_folded_and_punctuation_finds_nothing() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let bike = app.create_object(&app.client, "Tern", Some("km")).await;
    let id = bike["id"].as_i64().unwrap();
    let res = app.client.patch(app.url(&format!("/objects/{id}")))
        .json(&json!({ "tags": ["Fahrräder"] })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let r = app.search("fahrrader").await;
    assert_eq!(r["objects"].as_array().unwrap().len(), 1, "{r}");
    for term in ["[", "\"", "[\"Fahrr"] {
        let r = app.search(term).await;
        assert_eq!(r["objects"].as_array().unwrap().len(), 0, "{term}: {r}");
        assert_eq!(r["activities"].as_array().unwrap().len(), 0, "{term}: {r}");
    }
}
```

`app.search` percent-encodes every byte outside the unreserved set (`tests/common/mod.rs:743`
and `:1048`), so `[` and `"` reach the server as `%5B` and `%22`.

- [ ] **Step 3: Run both to see them fail**

Run: `cargo test --test dialect search_folds --test search tags_are_found`
Expected: dialect FAILS on sqlite (`olwechsel` finds 0); search FAILS on `fahrrader` (0 objects).

- [ ] **Step 4: Rewrite `search`**

In `src/api/search.rs`, delete `like_pattern` and the `#[cfg(test)] mod tests` block. Replace
the doc comment paragraph beginning "The operator is the one thing the two backends spell
differently" (through "rather than a surprise.") with:

```rust
/// Matching is done in Rust, not SQL: both statements select the caller's live rows and
/// `matches` folds each field with `domain::tags::fold` (NFD, combining marks stripped, lower
/// case) before testing `contains`. SQLite's `LIKE` folds only ASCII and PostgreSQL's `ILIKE`
/// folds by the cluster's collation, so a SQL predicate would answer "ölwechsel" differently
/// per backend; folding here makes the two agree, and matches what the objects list already
/// does on the client (`lib/object-list.ts`). A household's rows fit in one scan, the same
/// cost `activities::list_for_object` already pays for its tag filter.
```

Replace the body of `search` from `let pattern = like_pattern(term);` to the end of the
function with:

```rust
    let folded = fold(term);
    // Qualified: the self-join puts two `name` columns in scope, and an unqualified one is
    // ambiguous to PostgreSQL.
    let order = state.backend.name_order("o.name");

    let objects = sqlx::query_as::<_, ObjectHit>(sqlx::AssertSqlSafe(format!(
        "SELECT o.id, o.user_id, o.name, o.type, o.counter_unit, o.fuel_unit, o.description, \
         o.purchase_date, o.purchase_price_cents, o.archived_at, o.cover_attachment_id, \
         o.parent_id, o.created_at, o.updated_at, p.name AS parent_name, o.tags \
         FROM objects o LEFT JOIN objects p ON p.id = o.parent_id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL \
         ORDER BY o.archived_at IS NOT NULL, {order}"
    )))
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let activities = sqlx::query_as::<_, ActivityHit>(
        "SELECT a.id, a.object_id, o.name AS object_name, a.date, a.category, a.title, a.notes, \
         a.counter_value, a.cost_cents, a.weight_grams, a.tags, a.from_place, a.to_place \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
         ORDER BY a.date DESC, a.id DESC",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let skip = offset as usize;
    let mut objects: Vec<ObjectHit> = objects
        .into_iter()
        .filter(|o| matches(&folded, &[Some(&o.name), Some(&o.description)], &o.tags))
        .skip(skip)
        .take(limit as usize + 1)
        .collect();
    let mut activities: Vec<ActivityHit> = activities
        .into_iter()
        .filter(|a| {
            matches(
                &folded,
                &[Some(&a.title), Some(&a.notes), a.from_place.as_deref(), a.to_place.as_deref()],
                &a.tags,
            )
        })
        .skip(skip)
        .take(limit as usize + 1)
        .collect();

    let has_more = objects.len() > limit as usize || activities.len() > limit as usize;
    objects.truncate(limit as usize);
    activities.truncate(limit as usize);

    Ok(Json(SearchResults { objects, activities, has_more }))
}

/// Whether any of `fields`, or any tag in `tags_json`, contains the already-folded `term`.
/// Tags are matched one by one after decoding, so a term made of JSON punctuation cannot match
/// the encoding of every tagged row.
fn matches(term: &str, fields: &[Option<&str>], tags_json: &str) -> bool {
    if fields.iter().flatten().any(|f| fold(f).contains(term)) {
        return true;
    }
    serde_json::from_str::<Vec<String>>(tags_json)
        .map(|tags| tags.iter().any(|t| fold(t).contains(term)))
        .unwrap_or(false)
}
```

Add `use crate::domain::tags::fold;` at the top. Delete the `let like = ...`, `match_tags`,
`object_tags`, `activity_tags` lines.

- [ ] **Step 5: Run the tests**

Run: `cargo test --test dialect --test search`
Expected: all pass, including `search_paginates_without_duplicates` and
`wildcards_in_the_query_are_literal` (`%` and `_` are ordinary characters under `contains`).

- [ ] **Step 6: Reword the trip test's doc comment**

`tests/search.rs:166-168` says the umlaut proves the match runs through
`case_insensitive_like()`. Replace those three lines with:

```rust
/// A trip's places are searched like its title and notes -- and the umlaut proves the fold
/// applies to them too.
```

- [ ] **Step 7: Full gate and commit**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: green.

```bash
git add src/api/search.rs tests/dialect.rs tests/search.rs
git commit -m "feat: search ignores case and accents on both backends"
```

### Task 5: remove the dialect hook and update the README

**Files:**
- Modify: `src/dialect.rs:9, 39-51, 119-120`
- Modify: `README.md:525-529`

- [ ] **Step 1: Delete `case_insensitive_like`**

In `src/dialect.rs`: delete the method and its doc comment (lines 39-51), delete the two
`assert_eq!` lines on it in `each_backend_gets_the_operator_it_understands` (119-120), and
rewrite the module doc's list: item 1 becomes

```
//! 1. Case-insensitive matching. Neither backend folds accents in SQL (SQLite's `LIKE` is
//!    ASCII-only; PostgreSQL's `ILIKE` follows the cluster collation), so search folds in
//!    Rust with `domain::tags::fold` and needs nothing here.
```

and "and only the first four need code here" becomes "and only items 2 to 4 need code here".

- [ ] **Step 2: README**

Replace README lines 525-529 with:

```markdown
`GET /api/search?q=...` returns the caller's own objects and activities whose
name, description, tags, title, notes or trip places contain the term (`limit`,
default 25, caps at 100). The magnifier on the dashboard opens the same thing.
It is a substring scan, not a full-text index: instant at household scale, and
it ignores case and accents on both databases, so `olwechsel` finds `Ölwechsel`.
```

- [ ] **Step 3: Gate and commit**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`

```bash
git add src/dialect.rs README.md
git commit -m "refactor: drop the per-backend LIKE operator search no longer needs"
```

---

## Track C (part 1): split tests/sync.rs

### Task 6: one test binary, thirteen modules

**Files:**
- Create: `tests/sync/main.rs`, `tests/sync/helpers.rs`, `tests/sync/read_paths.rs`,
  `tests/sync/push.rs`, `tests/sync/validation.rs`, `tests/sync/pull.rs`,
  `tests/sync/bootstrap.rs`, `tests/sync/purge.rs`, `tests/sync/cascade.rs`,
  `tests/sync/rest.rs`, `tests/sync/tags_types.rs`, `tests/sync/deleted.rs`,
  `tests/sync/trips.rs`, `tests/sync/energy.rs`
- Delete: `tests/sync.rs`

**Interfaces:**
- Consumes: `tests/common/mod.rs` unchanged.
- Produces: nothing later tasks use.

- [ ] **Step 1: Record the baseline**

Run: `cargo test --test sync 2>&1 | grep -E "^test |test result" > /tmp/sync-before.txt; wc -l /tmp/sync-before.txt`
Expected: 97 `test ... ok` lines plus one `test result` line.

- [ ] **Step 2: Create main.rs and helpers.rs**

`tests/sync/main.rs`:

```rust
//! The sync protocol's integration tests, one binary split by theme. Shared fixtures live in
//! `helpers`; everything else is a pure move from the former `tests/sync.rs`.
#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod helpers;

mod bootstrap;
mod cascade;
mod deleted;
mod energy;
mod pull;
mod purge;
mod push;
mod read_paths;
mod rest;
mod tags_types;
mod trips;
mod validation;
```

`tests/sync/helpers.rs` receives, verbatim, these functions from `tests/sync.rs` with `pub`
added to each: `after_now` (line 11) and its doc, `before_now` (18), `png` (197),
`push_body` (635), `client_uuid` (1290), `object_with_children` (1302), `object_uuid` (4659).
Its header:

```rust
//! Fixtures shared by more than one theme module.
use super::common;
```

- [ ] **Step 3: Cut each theme module**

Every module file starts with:

```rust
use super::common;
use super::helpers::*;
use reqwest::multipart::{Form, Part};
use serde_json::json;
```

Then paste the line ranges below from `tests/sync.rs`, unchanged. Where a range lists a
theme-local helper, it goes into that module (not `helpers.rs`), also unchanged.

| Module | Lines from `tests/sync.rs` | Theme-local helper it keeps |
|---|---|---|
| `read_paths.rs` | 22-196, 214-634 | `export_object` (617) |
| `push.rs` | 639-1289 | — |
| `validation.rs` | 1391-1979 | — |
| `pull.rs` | 1980-2552 | — |
| `bootstrap.rs` | 2553-2717, 4236-4407 | — |
| `purge.rs` | 2718-3003, 3503-3656 | — |
| `cascade.rs` | 3004-3502, 3657-3997 | — |
| `rest.rs` | 3998-4235, 4408-4658 | — |
| `tags_types.rs` | 4667-5227 | `type_create_op` (4888) |
| `deleted.rs` | 5228-5509 | `stored_field`, `field_clock_of`, `assert_set_on_deleted_row_is_rejected` |
| `trips.rs` | 5510-6089 | `create_trip` (5510) |
| `energy.rs` | 6090-6542 | `create_charge` (6090) |

Ranges 197-213 (`png`), 635-638 (`push_body`), 1290-1390 (`client_uuid`,
`object_with_children`) and 4659-4666 (`object_uuid`) went to `helpers.rs` in Step 2; do not
paste them twice. Use `sed -n 'A,Bp' tests/sync.rs >> tests/sync/<module>.rs` so nothing is
retyped.

Remove the unused-import warnings the compiler reports per module (a module that never
uploads has no use for `Form`/`Part`); clippy runs with `-D warnings`, so each file's `use`
lines must be exactly what it uses. This is the only edit besides the file header.

- [ ] **Step 4: Delete the old file and compare**

```bash
git rm -q tests/sync.rs
cargo test --test sync 2>&1 | grep -E "^test |test result" > /tmp/sync-after.txt
diff <(sed 's/^test [a-z_]*::/test /' /tmp/sync-after.txt | sort) <(sort /tmp/sync-before.txt)
```

Expected: no output from `diff` except possibly the `test result` line's timing. Module paths
prefix each test name after the split (`test trips::pushing_...`), which the `sed` strips.
97 tests, all ok.

- [ ] **Step 5: Gate and commit**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`

```bash
git add tests/sync
git commit -m "test: split the sync suite into one binary of theme modules"
```

---

## Track B: due dates read against each user's own today

### Task 7: `db::today_in` and `AuthUser.tz`

**Files:**
- Modify: `src/db.rs:349-360`
- Modify: `src/auth.rs:19-34, 309-312, 344-347, 371-374`
- Modify: `src/api/auth.rs:217-221, 275-277`
- Test: `src/db.rs` unit tests (inline), `tests/personal_preferences.rs`

**Interfaces:**
- Produces: `pub fn db::zone(tz: Option<&str>) -> chrono_tz::Tz`,
  `pub fn db::today_in(tz: Option<&str>) -> chrono::NaiveDate`, `AuthUser.tz: Option<String>`,
  `AuthUser::today(&self) -> chrono::NaiveDate`. Tasks 8-10 use all four.

- [ ] **Step 1: Unit test for the zone fallback**

Add to `src/db.rs`'s existing `#[cfg(test)] mod tests` (create one at the bottom if the file
has none; check with `grep -n "cfg(test)" src/db.rs`):

```rust
    #[test]
    fn a_users_zone_falls_back_to_the_instance_zone() {
        use chrono_tz::Tz;
        assert_eq!(zone(Some("Europe/Berlin")), Tz::Europe__Berlin);
        assert_eq!(zone(Some("Mars/Olympus")), timezone());
        assert_eq!(zone(None), timezone());
    }
```

Run: `cargo test --lib a_users_zone_falls_back`
Expected: FAIL, `zone` not found.

- [ ] **Step 2: Add `zone` and `today_in`**

In `src/db.rs`, after `pub fn timezone()`:

```rust
/// The zone a user's dates are read in: their own when they chose a valid one under
/// Settings → Notifications (`users.notify_tz`), the instance's otherwise. A stored name that
/// no longer parses is treated as unset rather than failing the request.
pub fn zone(tz: Option<&str>) -> Tz {
    tz.and_then(|raw| raw.parse::<Tz>().ok()).unwrap_or_else(timezone)
}

/// Today in `zone(tz)`. This is the date a reminder's `due_date` is compared against for that
/// user; `today()` below is the instance-wide answer for everything that is not per user.
pub fn today_in(tz: Option<&str>) -> NaiveDate {
    Utc::now().with_timezone(&zone(tz)).date_naive()
}
```

Change `today()`'s body to `today_in(None).to_string()` and its doc's second paragraph to:
"This is the instance's today: file names, delivery-history pruning and every stats bucket use
it. A reminder's due date is read against `today_in` with the user's own zone instead." Add
`use chrono::NaiveDate;` if not already imported.

Run: `cargo test --lib a_users_zone_falls_back`
Expected: PASS.

- [ ] **Step 3: `AuthUser.tz`**

In `src/auth.rs`, after the `token_id` field:

```rust
    /// `users.notify_tz`, the zone the user chose for their digest hour and, from the same
    /// setting, for reading their reminders' due dates. `#[serde(skip)]` because `/auth/me`
    /// returns this struct and the notifications endpoint already reports the zone;
    /// `#[sqlx(default)]` so a query that does not select it still decodes.
    #[serde(skip)]
    #[sqlx(default)]
    pub tz: Option<String>,
```

and after the struct:

```rust
impl AuthUser {
    /// Today in this user's own zone (`db::zone`), the date their reminders are due against.
    pub fn today(&self) -> chrono::NaiveDate {
        crate::db::today_in(self.tz.as_deref())
    }
}
```

Add `u.notify_tz AS tz` to the three SELECTs at `auth.rs:309-312`, `344-347`, `371-374`
(after `u.lang`), and `notify_tz AS tz` to `api/auth.rs:275-277`. Leave the `RETURNING` at
`api/auth.rs:219-221` alone: a freshly set-up admin has no zone yet and `#[sqlx(default)]`
covers it.

- [ ] **Step 4: Integration test that the zone reaches the request**

Append to `tests/personal_preferences.rs`:

```rust
/// `AuthUser` carries the zone on every request, so a handler can read the user's today
/// without a second query. Proven through `/reminders/due`, the first consumer (Task 8):
/// this test is written now and stays red until then.
#[tokio::test]
async fn a_user_in_a_far_eastern_zone_sees_the_instances_tomorrow_as_due() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    // Instance is UTC (the harness sets no timezone). Kiritimati is UTC+14, so from 10:00 UTC
    // onward it is already tomorrow there; before that the two agree and this test would be
    // vacuous, so the due date is chosen from the user's own today instead of "UTC + 1".
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Kiritimati).date_naive();
    let instance_today = chrono::Utc::now().date_naive();
    let res = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Inspection", "due_date": user_today.to_string() }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);

    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": "Pacific/Kiritimati" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let due = app.get_json("/reminders/due").await;
    assert_eq!(due.as_array().unwrap().len(), 1, "due for the user in UTC+14: {due}");
    assert_eq!(due[0]["days_until"], 0);
    let object = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(object["stats"]["due_reminder_count"], 1);

    // Back on the instance zone the same reminder is only due once the instance's date
    // catches up -- which it already has whenever the two calendars agree right now.
    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": null })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let due = app.get_json("/reminders/due").await;
    let expected = usize::from(user_today <= instance_today);
    assert_eq!(due.as_array().unwrap().len(), expected, "due on the instance zone: {due}");
}
```

Run: `cargo test --test personal_preferences a_user_in_a_far`
Expected: compiles; FAILS on the first `due` assertion whenever the two calendars differ, and
passes vacuously when they agree. Either result is acceptable at this task; Task 8 makes it
deterministic in the assertion that matters.

- [ ] **Step 5: Gate and commit**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked --test auth --test personal_preferences --lib`
Expected: everything but the new test passes (it may be red).

```bash
git add src/db.rs src/auth.rs src/api/auth.rs tests/personal_preferences.rs
git commit -m "feat: carry each user's timezone on the authenticated request"
```

### Task 8: reminders read against the caller's today

**Files:**
- Modify: `src/api/reminders.rs` (fn `today` 95-97, `reading_horizon` 99-110, `From` impl
  174-178, `load_owned` 195-208, `out` 210-215, `validate` 249+ at 284 and 301, `list` 365-388,
  `due_for_user` 392-433, `due_list` 437-445, `create` 454, `update` 511, `done` 655-667,
  `snooze` 778-788, unit tests 898-928)
- Modify: `src/sync/apply/reminder.rs:55-87`
- Modify: `src/api/export/import.rs:529-537`
- Modify: `src/api/insights.rs:122-135, 428-434`
- Test: `tests/personal_preferences.rs` (Task 7's test goes green), `tests/reading_reminders.rs`

**Interfaces:**
- Consumes: `AuthUser::today()`, `db::today_in`.
- Produces: `pub(crate) fn reading_horizon(today: NaiveDate) -> String`,
  `pub async fn due_for_user(state, user_id: i64, within_days: i64, today: NaiveDate)`,
  `pub(crate) fn ReminderInput::validate(&mut self, counter_unit: Option<&str>, today: NaiveDate)`,
  `pub async fn insights::usage_by_object(state, user_id, only, today: NaiveDate)`. Tasks 9-10
  call these.

- [ ] **Step 1: A reading-horizon test in the user's zone**

Append to `tests/personal_preferences.rs`:

```rust
/// The one-day reading horizon moves with the user's zone: a reading dated the user's
/// tomorrow counts as the latest, the user's day after tomorrow does not.
#[tokio::test]
async fn the_reading_horizon_follows_the_users_zone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": "Pacific/Kiritimati" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Kiritimati).date_naive();
    let tomorrow = user_today.succ_opt().unwrap();
    let after = tomorrow.succ_opt().unwrap();
    for (date, counter) in [(tomorrow, 1_000), (after, 2_000)] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
            .json(&json!({ "date": date.to_string(), "category": "maintenance", "title": "reading", "counter_value": counter }))
            .send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }
    let object = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(object["stats"]["last_reading_date"], tomorrow.to_string(), "{object}");
}
```

And the western direction, same file:

```rust
/// West of the instance the user's today can still be the instance's yesterday, so a reminder
/// due on the instance's today is not yet due for them.
#[tokio::test]
async fn a_user_in_a_far_western_zone_sees_the_instances_today_as_tomorrow() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let instance_today = chrono::Utc::now().date_naive();
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Pago_Pago).date_naive();
    let res = app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Inspection", "due_date": instance_today.to_string() }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": "Pacific/Pago_Pago" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let due = app.get_json("/reminders/due").await;
    // Before 11:00 UTC the user is still on yesterday and the reminder is a day away.
    let expected = usize::from(instance_today <= user_today);
    assert_eq!(due.as_array().unwrap().len(), expected, "{due}");
    let all = app.get_json(&format!("/objects/{id}/reminders")).await;
    let expected_days = (instance_today - user_today).num_days();
    assert_eq!(all[0]["days_until"], expected_days, "{all}");
}
```

Run: `cargo test --test personal_preferences the_reading_horizon a_user_in_a_far_western`
Expected: FAIL whenever the user's tomorrow is beyond the instance's horizon (from 10:00 UTC);
vacuous otherwise. Task 9 threads `today` into the objects query this asserts on; the test is
written here because the horizon function changes here.

- [ ] **Step 2: Rewrite the module's today plumbing**

In `src/api/reminders.rs`:

Delete `fn today()` (95-97). Change `reading_horizon`:

```rust
/// The latest date a counter reading may carry and still count as "the last reading": one day
/// past the caller's today.
///
/// The reading form dates a reading by the device's own calendar, the server by the user's
/// zone (or `LOGB_TIMEZONE`), and the two disagree for hours every day wherever those differ
/// -- a reading logged at 00:30 in Berlin is dated tomorrow as far as a UTC server knows.
/// Ignoring it would leave the reminder due right after the person did what it asked. One day
/// covers any timezone; a reading dated further out is a typo, and is still ignored.
pub(crate) fn reading_horizon(today: NaiveDate) -> String {
    today.succ_opt().unwrap_or(today).to_string()
}
```

Delete `impl From<ReminderRow> for ReminderOut` (174-178). In the unit tests at 898, 912, 928
replace `ReminderOut::from(x)` with `ReminderOut::build(x, d("2026-09-22"), None)` (`d` is
defined at 885).

`load_owned`: signature `async fn load_owned(state: &App, user: &AuthUser, id: i64)`; bind
`reading_horizon(user.today())` and `user.id`. Update every caller (`grep -n "load_owned(" src/api/reminders.rs`)
to pass `&user`.

`out`: signature `async fn out(state: &App, user: &AuthUser, row: ReminderRow)`; body:

```rust
    let today = user.today();
    let usage = usage_by_object(state, Some(user.id), Some(row.object_id), today)
        .await?
        .remove(&row.object_id);
    Ok(ReminderOut::build(row, today, usage))
```

Every `out(&state, user.id, row)` call (lines 473, 486, 495, 569, 735, 741, 809, 850) becomes
`out(&state, &user, row)`.

`validate`: signature `pub(crate) fn validate(&mut self, counter_unit: Option<&str>, today: NaiveDate)`.
Line 284: `self.due_date = Some(today.to_string());`. Line 301:
`.unwrap_or(today)` instead of `.unwrap_or_else(today)`. Callers at 454 and 511 add
`, user.today()` as the second argument.

`list`: bind `reading_horizon(user.today())`, replace `let today = today();` with
`let today = user.today();`, and pass `today` to `usage_by_object`.

`due_for_user`: add parameter `today: NaiveDate` after `within_days`, bind
`reading_horizon(today)`, delete `let today = today();`, pass `today` to `usage_by_object`.
`due_list` calls `due_for_user(&state, user.id, q.within_days.clamp(0, 365), user.today())`.

`done`: `.unwrap_or_else(today)` at 655 becomes `.unwrap_or_else(|| user.today())`; `.max(today())`
at 667 becomes `.max(user.today())`. `snooze`: `let today = today();` at 778 becomes
`let today = user.today();`.

- [ ] **Step 3: The two callers outside the module**

`src/sync/apply/reminder.rs`: add `u.notify_tz` to the SELECT at 61-63 (`... o.counter_unit,
o.type, u.notify_tz FROM reminders r JOIN objects o ON o.id = r.object_id JOIN users u ON u.id =
o.user_id WHERE r.client_uuid = $1`), extend the `Row` tuple type and destructuring with
`notify_tz: Option<String>` at the end, and change the last line to:

```rust
    Ok(input.validate(unit, crate::db::today_in(notify_tz.as_deref())).err().map(|e| e.to_string()))
```

`src/api/export/import.rs:529-537`: `.validate(<unit expr>, user.today())` — the handler already
has `user: AuthUser` in scope (line 129).

`src/api/insights.rs::usage_by_object`: add `today: NaiveDate` as the last parameter; replace
lines 126-129 with

```rust
    let from = (today - chrono::Duration::days(RATE_WINDOW_DAYS * 2)).to_string();
```

and bind `super::reminders::reading_horizon(today)`. Its callers at insights.rs:40 and 421 pass
`user.today()`; line 434 binds `super::reminders::reading_horizon(user.today())`. Line 199's
`today` (month buckets) stays on `db::today()`: per the spec, insights windows are instance-day.

- [ ] **Step 4: Compile and run the reminder suites**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked --test reminders --test reading_reminders --test sync --test export --test insights --test personal_preferences`
Expected: green except `the_reading_horizon_follows_the_users_zone` and the object count line
of `a_user_in_a_far_eastern_zone_sees_the_instances_tomorrow_as_due`, both of which wait for
Task 9. `objects.rs` still calls `reading_horizon()` with no argument and will not compile until
Task 9 — so do Task 9's Step 2 in the same working tree before running this gate, and commit
the two tasks together if that is simpler. Either way the commit message below stands.

- [ ] **Step 5: Commit**

```bash
git add src/api/reminders.rs src/sync/apply/reminder.rs src/api/export/import.rs src/api/insights.rs tests/personal_preferences.rs
git commit -m "feat: read reminder due dates against the caller's own today"
```

### Task 9: objects' due counts and readings use the caller's today

**Files:**
- Modify: `src/api/objects.rs` (`derived` 397-450, `due_readings` 454-497, `stats` 525-530,
  `with_stats` 598-604, `list` 648, and the `INVARIANT` comment 402-406)
- Modify: `src/api/reminders.rs:660` (`stats` caller)
- Test: `tests/objects.rs`

**Interfaces:**
- Consumes: `reading_horizon(today)`, `usage_by_object(..., today)`, `AuthUser::today()`.
- Produces: `pub async fn objects::stats(state, object_id: i64, today: NaiveDate)`.

- [ ] **Step 1: Extend the invariant test with a zoned user**

In `tests/objects.rs`, after `due_reminder_count_agrees_with_each_reminders_due_flag`'s final
assertion, add a second test:

```rust
/// The SQL count and the Rust `is_due` must agree for a user whose today is not the
/// instance's. Same fixture shape as above, one reminder on the boundary date.
#[tokio::test]
async fn due_reminder_count_agrees_for_a_user_in_another_zone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({ "hour": 8, "timezone": "Pacific/Kiritimati" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Kiritimati).date_naive();
    for (title, date) in [("On the day", user_today), ("Tomorrow there", user_today.succ_opt().unwrap())] {
        let res = app.client.post(app.url(&format!("/objects/{id}/reminders")))
            .json(&json!({ "title": title, "due_date": date.to_string() })).send().await.unwrap();
        assert_eq!(res.status(), 201);
    }
    let reminders = app.get_json(&format!("/objects/{id}/reminders")).await;
    let due_flags = reminders.as_array().unwrap().iter().filter(|r| r["due"] == true).count();
    assert_eq!(due_flags, 1, "{reminders}");
    let object = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(object["stats"]["due_reminder_count"], 1, "{object}");
}
```

Check `tests/objects.rs` imports `serde_json::json` (it does; `grep -n "use serde_json" tests/objects.rs`)
and that `chrono-tz` is reachable from tests (it is a runtime dependency, so `chrono_tz::` works
in integration tests via the lib? No — integration tests only see the crate's public API and
their own dev-dependencies. `chrono` and `chrono_tz` are already used by `tests/notify.rs:89-106`,
so this is fine).

Run: `cargo test --test objects due_reminder_count_agrees_for_a_user`
Expected: FAIL on `due_reminder_count` (0 or 2, depending on the hour) before Step 2; the
`due` flags already agree after Task 8.

- [ ] **Step 2: Thread `today` through `derived`, `due_readings`, `stats`, `with_stats`**

`derived(state, user_id, only, today: NaiveDate)`: bind `today.to_string()` for `$2` and
`super::reminders::reading_horizon(today)` for `$4`; call `due_readings(state, user_id, only, today)`
and `usage_by_object(state, user_id, only, today)`.

`due_readings(state, user_id, only, today: NaiveDate)`: delete `let today_str = db::today();`
and the `let Some(today) = parse(...) else { ... }` block; bind
`super::reminders::reading_horizon(today)`.

`stats(state, object_id, today: NaiveDate)` passes `today` to `derived`. Its one caller,
`reminders.rs:660`, becomes `stats(&state, r.object_id, user.today()).await?.current_counter`.

`with_stats(state, user: &AuthUser, object)` passes `user.today()` to `derived`; its four
callers (677, 773, 782, 1033) pass `&user`. `list` at 648:
`derived(&state, Some(user.id), None, user.today())`.

Extend the INVARIANT comment with one sentence: "Both encodings read the same `today`, the
caller's own (`AuthUser::today`), so a user in another zone gets one answer from both."

- [ ] **Step 3: Full gate**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: green, including the two tests from Tasks 7-8 and this task's.

Then the postgres suite:

```bash
docker run --rm -d --name logb-pg -e POSTGRES_PASSWORD=logb -p 5432:5432 postgres:16
until docker exec logb-pg pg_isready -q; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:5432/postgres cargo test --all-targets --locked
docker rm -f logb-pg
```

Expected: same pass set.

- [ ] **Step 4: Commit**

```bash
git add src/api/objects.rs src/api/reminders.rs tests/objects.rs
git commit -m "feat: count due reminders against the caller's own today"
```

### Task 10: the digest's day is the recipient's day

**Files:**
- Modify: `src/notify.rs` (`items_for` 112-127, `claim_delivery` 307-333, `local_hour` doc
  360-372, `tick` 413-440)
- Test: `tests/notify.rs`

**Interfaces:**
- Consumes: `due_for_user(state, id, 0, today)`, `db::today_in`.
- Produces: nothing later tasks use.

- [ ] **Step 1: Failing test**

Append to `tests/notify.rs`:

```rust
/// A recipient far east of the instance gets a digest for their own today, and the delivery
/// row records that day, not the instance's.
#[tokio::test]
async fn the_digest_covers_the_recipients_own_day() {
    let (url, inbox) = webhook().await;
    let app = common::spawn_with(|c| c.notify_hour = 18).await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let user_today = chrono::Utc::now().with_timezone(&chrono_tz::Pacific::Kiritimati).date_naive();
    app.client.post(app.url(&format!("/objects/{id}/reminders")))
        .json(&json!({ "title": "Inspection", "due_date": user_today.to_string() }))
        .send().await.unwrap();
    app.client.put(app.url("/me/notifications")).json(&json!({"url":url,"format":"text"})).send().await.unwrap();
    // Hour 0 in Kiritimati: the tick below runs at an instance hour that is past it there.
    app.client.put(app.url("/me/notifications/hour"))
        .json(&json!({"hour":0, "timezone":"Pacific/Kiritimati"})).send().await.unwrap();

    logb::notify::tick(&app.state, 23).await.unwrap();
    let received = inbox.received();
    assert_eq!(received.len(), 1, "no digest for the recipient's own today");
    assert!(received[0].2.contains("Inspection"), "{}", received[0].2);
    let day: (String,) = sqlx::query_as("SELECT day FROM notification_deliveries WHERE target = $1")
        .bind(format!("webhook:{}", 1)).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(day.0, user_today.to_string());
}
```

Run: `cargo test --test notify the_digest_covers`
Expected: FAIL from 10:00 UTC (the reminder is not due on the instance's today, so no digest);
vacuous before. Run it once more after Step 2 at any hour; the `day` assertion is what
distinguishes the two whenever the calendars differ.

- [ ] **Step 2: Implement**

`items_for`:

```rust
async fn items_for(state: &App, r: &Recipient) -> Result<Vec<DueItem>, AppError> {
    let public_url = state.config.public_url.as_deref();
    let today = crate::db::today_in(r.notify_tz.as_deref());
    Ok(due_for_user(state, r.id, 0, today)
```

`claim_delivery(state, target, user_id, day: &str)`: delete `let day = db::today();`, use the
parameter. In `tick`, extend the `plans` tuple with the day: the instance entry pushes
`db::today()`, each recipient entry pushes `crate::db::today_in(r.notify_tz.as_deref()).to_string()`
(compute it once per recipient as `let day = ...;` before the `if let Some(url)` line), and the
loop destructures `(target, owner, day, destination, digest)` and calls
`claim_delivery(state, &target, owner, &day)`.

Replace the `local_hour` doc paragraph "The digest's *day* is still the instance's day ..." with:

```rust
/// The digest's day is the recipient's too: `items_for` reads due dates against
/// `db::today_in(notify_tz)`, and `claim_delivery` records that day, so "once per destination
/// per day" is counted in the recipient's calendar. Delivery-history pruning and the instance
/// webhook stay on the instance's day.
```

- [ ] **Step 3: Gate and commit**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked --test notify --test notifications --test reading_reminders`
Expected: green.

```bash
git add src/notify.rs tests/notify.rs
git commit -m "feat: send the digest for the recipient's own day"
```

### Task 11: README and OpenAPI for per-user today

**Files:**
- Modify: `README.md:432-438, 504-506`
- Modify: `docs/openapi.json` (`due` ~3897, `days_until` ~3901, `NotificationSettings.timezone` ~5485)

- [ ] **Step 1: README Time section**

After the paragraph ending "rendered in the reader's locale." (line 438) add:

```markdown
A person who picks their own timezone under Settings → Notifications has their reminders read
against today in that zone instead: due dates, the day a counter reading still counts as the
latest, the object's due count and the digest they receive. Statistics, insights, the trip
summary's default date and export file names stay on the instance's day.
```

Replace lines 504-506 ("Each user can choose ... at their own breakfast.") with:

```markdown
Each user can choose a daily delivery hour in Settings → Notifications, and the timezone that
hour is read in — their own, or the instance's. That zone also decides which day the digest
covers, so someone far east or west reads their own today, not the server's.
```

- [ ] **Step 2: OpenAPI descriptions**

`due`: `"Due now against the caller's own today (their notification timezone, else the instance's), accounting for any live snooze"`.
`days_until`: prepend `"Measured from the caller's own today. "` to its existing description.
`NotificationSettings.timezone`: `"The recipient's own IANA timezone, when they chose one. Also the zone their reminders' due dates are read against."`

Run: `cargo test --test openapi`
Expected: pass (descriptions are free text; the route set is unchanged).

- [ ] **Step 3: Playwright for Track B, then commit**

Run: `cd frontend && npx playwright test`
Expected: all specs pass; nothing in the client changed, this proves the server's `due` still
drives the dashboard.

```bash
git add README.md docs/openapi.json
git commit -m "docs: state which today a user's reminders are read against"
```

---

## Track C (part 2): frontend splits

### Task 12: ActivityForm logic into lib/activity-form.ts

**Files:**
- Modify: `frontend/src/lib/activity-form.ts`
- Modify: `frontend/src/routes/ActivityForm.svelte:79-97, 117-134, 143-159, 176-188, 255-266,
  295-305, 310-362`
- Test: `frontend/tests/activity-form.test.ts`

**Interfaces:**
- Produces (all exported from `lib/activity-form.ts`):
  - `hashToNegativeId(s: string): number` (Task 13 imports it)
  - `resolveCategory(offered: Category[], wanted: Category | null, current: Category): { category: Category; touched: boolean }`
  - `parseCategoryParam(search: string): Category | null`
  - `counterBelowLast(counterText: string, lastCounter: number | null): boolean`
  - `weightDeviates(category: Category, weightText: string, unit: WeightUnit, latestGrams: number | null | undefined): boolean`
  - `firstExifDate(attachments: { taken_at: string | null }[]): string | null`
  - `interface FormTexts { weightText, costText, counterText, quantityText, meterReadingText, fromText, toText, durationText: string; chargedFull: boolean }`
  - `activityToFormText(a: Activity, weightUnit: WeightUnit): FormTexts & { distance: number | null }`
  - `interface WeightState { text: string; unit: WeightUnit; originalText: string; originalUnit: WeightUnit; original: number | null }`
  - `changeWeightUnitState(s: WeightState, next: WeightUnit): WeightState`
  - `buildActivityInput(input: ActivityInput, texts: FormTexts, weight: WeightState, object: MemObject | null, t: (k: string) => string): ActivityInput`
  - `optimisticActivity(tempId: number, oid: number, body: ActivityInput): Activity`

- [ ] **Step 1: Write the tests first**

Append to `frontend/tests/activity-form.test.ts` (imports extended accordingly):

```ts
import { hashToNegativeId, resolveCategory, parseCategoryParam, counterBelowLast, weightDeviates, firstExifDate, activityToFormText, changeWeightUnitState, buildActivityInput, optimisticActivity } from '../src/lib/activity-form';

describe('hashToNegativeId', () => {
  it('is negative, stable and never zero', () => {
    expect(hashToNegativeId('op-1')).toBeLessThan(0);
    expect(hashToNegativeId('op-1')).toBe(hashToNegativeId('op-1'));
    expect(hashToNegativeId('')).toBe(-1);
  });
});

describe('resolveCategory', () => {
  it('takes a wanted category the object offers and marks it touched', () => {
    expect(resolveCategory(['trip', 'fuel'], 'trip', 'maintenance')).toEqual({ category: 'trip', touched: true });
  });
  it('falls back to the first offered category when the current one is not offered', () => {
    expect(resolveCategory(['weight'], null, 'maintenance')).toEqual({ category: 'weight', touched: false });
  });
  it('keeps a current category that is offered', () => {
    expect(resolveCategory(['repair', 'maintenance'], null, 'maintenance')).toEqual({ category: 'maintenance', touched: false });
  });
  it('ignores a wanted category the object does not offer', () => {
    expect(resolveCategory(['repair'], 'trip', 'repair')).toEqual({ category: 'repair', touched: false });
  });
});

describe('parseCategoryParam', () => {
  it('reads a known category and ignores anything else', () => {
    expect(parseCategoryParam('?category=trip')).toBe('trip');
    expect(parseCategoryParam('?category=nope')).toBeNull();
    expect(parseCategoryParam('')).toBeNull();
  });
});

describe('warnings', () => {
  it('flags a counter below the last known one', () => {
    expect(counterBelowLast('100', 200)).toBe(true);
    expect(counterBelowLast('300', 200)).toBe(false);
    expect(counterBelowLast('', 200)).toBe(false);
    expect(counterBelowLast('100', null)).toBe(false);
  });
  it('flags a weight more than ten percent off the latest', () => {
    expect(weightDeviates('weight', '90', 'kg', 80_000)).toBe(true);
    expect(weightDeviates('weight', '81', 'kg', 80_000)).toBe(false);
    expect(weightDeviates('repair', '90', 'kg', 80_000)).toBe(false);
    expect(weightDeviates('weight', '90', 'kg', null)).toBe(false);
  });
  it('picks the first attachment with an EXIF date', () => {
    expect(firstExifDate([{ taken_at: null }, { taken_at: '2026-01-02T10:00:00Z' }])).toBe('2026-01-02');
    expect(firstExifDate([])).toBeNull();
  });
});

describe('activityToFormText', () => {
  it('renders every nullable field as text', () => {
    const t = activityToFormText({ ...a(1, '2026-01-01'), category: 'trip', cost_cents: 1234, counter_value: 500, quantity_milli: 42_500, meter_reading_milli: 1_500, charged_full: 1, from_place: 'A', to_place: 'B', duration_minutes: 90, weight_grams: 80_000, start_counter: 400 }, 'kg');
    expect(t).toMatchObject({ costText: '12.34', counterText: '500', quantityText: '42.5', meterReadingText: '1.5', chargedFull: true, fromText: 'A', toText: 'B', durationText: '1:30', distance: 100 });
    expect(t.weightText).toBe('80');
  });
  it('renders nulls as empty strings', () => {
    const t = activityToFormText(a(1, '2026-01-01'), 'kg');
    expect(t).toMatchObject({ costText: '', counterText: '', quantityText: '', meterReadingText: '', chargedFull: false, fromText: '', toText: '', durationText: '', distance: null });
  });
});

describe('changeWeightUnitState', () => {
  it('converts the typed text and remembers the original grams', () => {
    const s = changeWeightUnitState({ text: '80', unit: 'kg', originalText: '', originalUnit: 'kg', original: null }, 'lb');
    expect(s.unit).toBe('lb');
    expect(s.original).toBe(80_000);
    expect(Number(s.text)).toBeCloseTo(176.4, 1);
  });
  it('reuses the original grams when the text is untouched', () => {
    const s = changeWeightUnitState({ text: '176.4', unit: 'lb', originalText: '176.4', originalUnit: 'lb', original: 80_000 }, 'kg');
    expect(s.original).toBe(80_000);
    expect(s.text).toBe('80');
  });
  it('only switches the unit when the text is not a number', () => {
    const s = changeWeightUnitState({ text: 'abc', unit: 'kg', originalText: '', originalUnit: 'kg', original: null }, 'lb');
    expect(s).toEqual({ text: 'abc', unit: 'lb', originalText: '', originalUnit: 'kg', original: null });
  });
});

const texts = { weightText: '', costText: '', counterText: '', quantityText: '', meterReadingText: '', fromText: '', toText: '', durationText: '', chargedFull: true };
const weight = { text: '', unit: 'kg' as const, originalText: '', originalUnit: 'kg' as const, original: null };
const t = (k: string) => k;

describe('buildActivityInput', () => {
  it('nulls every trip field on a non-trip entry', () => {
    const out = buildActivityInput({ ...emptyActivity(), category: 'repair', start_counter: 5, battery_used_pct: 3 }, { ...texts, fromText: 'A', toText: 'B', durationText: '1:00', counterText: '700' }, weight, null, t);
    expect(out).toMatchObject({ start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null, counter_value: 700 });
  });
  it('keeps trip fields on a trip and reads the end from the input', () => {
    const out = buildActivityInput({ ...emptyActivity(), category: 'trip', start_counter: 400, counter_value: 600 }, { ...texts, fromText: ' A ', toText: '', durationText: '0:45' }, weight, null, t);
    expect(out).toMatchObject({ start_counter: 400, counter_value: 600, from_place: 'A', to_place: null, duration_minutes: 45 });
  });
  it('sends fuel quantity and charged_full only for fuel', () => {
    const fuel = buildActivityInput({ ...emptyActivity(), category: 'fuel' }, { ...texts, quantityText: '42,5' }, weight, null, t);
    expect(fuel).toMatchObject({ quantity_milli: 42_500, charged_full: 1 });
    const repair = buildActivityInput({ ...emptyActivity(), category: 'repair' }, { ...texts, quantityText: '42,5' }, weight, null, t);
    expect(repair).toMatchObject({ quantity_milli: null, charged_full: 0 });
  });
  it('a weight entry gets grams and the category word as its title', () => {
    const out = buildActivityInput({ ...emptyActivity(), category: 'weight', title: '' }, { ...texts, weightText: '80', costText: '5' }, weight, null, t);
    expect(out).toMatchObject({ weight_grams: 80_000, title: 'cat.weight', cost_cents: null, counter_value: null });
  });
  it('copies tags into a plain array', () => {
    const tags = ['a'];
    const out = buildActivityInput({ ...emptyActivity(), tags }, texts, weight, null, t);
    expect(out.tags).toEqual(['a']);
    expect(out.tags).not.toBe(tags);
  });
});

describe('optimisticActivity', () => {
  it('fills every nullable field and marks the row pending', () => {
    const out = optimisticActivity(-7, 3, { ...emptyActivity(), title: 'x' });
    expect(out).toMatchObject({ id: -7, object_id: 3, title: 'x', pending: true, attachments: [], charged_full: 0, estimated: 0, meter_reset: 0, tags: [] });
    expect(typeof out.created_at).toBe('string');
  });
});
```

Expected strings follow the existing formatters: `weightInput` is `String(Number(x.toFixed(3)))`
so 80 000 g reads `'80'`; `centsToInput` is `toFixed(2)`; `formatDuration(90)` is `'1:30'`;
`tripDistance` needs both `start_counter` and `counter_value`. The `toBeCloseTo(176.4, 1)`
allows `weightInput`'s three-decimal rounding of 80 kg in pounds.

Run: `cd frontend && npm test -- activity-form`
Expected: FAIL, the new exports do not exist.

- [ ] **Step 2: Add the functions to `lib/activity-form.ts`**

Append (imports at top: `import { parseWeight, weightInput, changeWeightUnitValue } from './weight'; import { centsToInput, parseMoney, parseQuantity } from './format'; import { formatDuration, parseDuration, tripDistance } from './trip'; import { CATEGORIES, type MemObject, type WeightUnit } from './types';` merged with the existing import lines):

```ts
/** A stable negative id derived from a string (an outbox op id), so a queued row can sit in an
 *  `id`-keyed list beside real, always-positive ids. Shared by ActivityForm (minting a temp id)
 *  and ObjectDetail (rendering a queued op). */
export function hashToNegativeId(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return -(Math.abs(h) || 1);
}

/** Which category a new entry starts on: a `?category=` the object offers wins and counts as
 *  the user's pick; otherwise the current one if offered, else the first offered. */
export function resolveCategory(offered: Category[], wanted: Category | null, current: Category): { category: Category; touched: boolean } {
  if (wanted && offered.includes(wanted)) return { category: wanted, touched: true };
  if (!offered.includes(current)) return { category: offered[0], touched: false };
  return { category: current, touched: false };
}

/** The `?category=` of a `.../activities/new` link; unknown values are ignored. */
export function parseCategoryParam(search: string): Category | null {
  const c = new URLSearchParams(search).get('category');
  return c && (CATEGORIES as readonly string[]).includes(c) ? (c as Category) : null;
}

export function counterBelowLast(counterText: string, lastCounter: number | null): boolean {
  return counterText !== '' && lastCounter !== null && Number(counterText) < lastCounter;
}

export function weightDeviates(category: Category, weightText: string, unit: WeightUnit, latestGrams: number | null | undefined): boolean {
  if (category !== 'weight' || latestGrams == null) return false;
  const grams = parseWeight(weightText, unit);
  return Number.isFinite(grams) && Math.abs(grams - latestGrams) / latestGrams > 0.1;
}

export function firstExifDate(attachments: { taken_at: string | null }[]): string | null {
  return attachments.map(exifDate).find((d) => d !== null) ?? null;
}

export interface FormTexts {
  weightText: string; costText: string; counterText: string; quantityText: string; meterReadingText: string;
  fromText: string; toText: string; durationText: string; chargedFull: boolean;
}

/** The loaded row as the form's text fields show it. */
export function activityToFormText(a: Activity, weightUnit: WeightUnit): FormTexts & { distance: number | null } {
  return {
    weightText: weightInput(a.weight_grams, weightUnit),
    costText: centsToInput(a.cost_cents),
    counterText: a.counter_value === null ? '' : String(a.counter_value),
    quantityText: a.quantity_milli === null ? '' : String(a.quantity_milli / 1000),
    meterReadingText: a.meter_reading_milli == null ? '' : String(a.meter_reading_milli / 1000),
    chargedFull: a.charged_full === 1,
    fromText: a.from_place ?? '',
    toText: a.to_place ?? '',
    durationText: a.duration_minutes === null ? '' : formatDuration(a.duration_minutes),
    distance: tripDistance(a),
  };
}

export interface WeightState { text: string; unit: WeightUnit; originalText: string; originalUnit: WeightUnit; original: number | null }

/** Switching kg/lb converts the typed number without drifting: an untouched value goes back
 *  to its original grams rather than through two roundings. */
export function changeWeightUnitState(s: WeightState, next: WeightUnit): WeightState {
  const untouched = s.original !== null && s.text === s.originalText && s.unit === s.originalUnit;
  const grams = untouched ? s.original! : parseWeight(s.text, s.unit);
  if (!Number.isFinite(grams)) return { ...s, unit: next };
  const text = changeWeightUnitValue(s.text, s.unit, next);
  return { text, unit: next, originalText: text, originalUnit: next, original: grams };
}

function storedGrams(w: WeightState): number {
  return w.original !== null && w.text === w.originalText && w.unit === w.originalUnit ? w.original : parseWeight(w.text, w.unit);
}

/** The request body for the current form state: category-gated so a field typed under one
 *  category cannot ride along after switching to another. */
export function buildActivityInput(input: ActivityInput, texts: FormTexts, weight: WeightState, object: MemObject | null, t: (k: string) => string): ActivityInput {
  const cat = input.category;
  const isTrip = cat === 'trip';
  const isSession = cat === 'session';
  const isWeight = cat === 'weight';
  const isUsage = cat === 'usage';
  const resourceUnit = object?.resource_unit ?? object?.fuel_unit ?? null;
  const liquid = resourceUnit === 'l' || resourceUnit === 'gal';
  const water = object?.resource_kind === 'water';
  const meter = object?.measurement_mode === 'meter';
  const usageMode = object?.measurement_mode === 'usage';
  return {
    ...input,
    title: isWeight ? (input.title || t('cat.weight')) : input.title,
    weight_grams: isWeight ? storedGrams(weight) : null,
    tags: [...(input.tags ?? [])],
    cost_cents: isWeight ? null : parseMoney(texts.costText),
    counter_value: isWeight ? null : isTrip ? input.counter_value : (String(texts.counterText).trim() === '' ? null : Number(texts.counterText)),
    quantity_milli: cat === 'fuel' || (isUsage && !meter) ? parseQuantity(texts.quantityText) : null,
    charged_full: (cat === 'fuel' || (isUsage && !water)) && texts.chargedFull ? 1 : 0,
    fuel_level_pct: (cat === 'fuel' || isUsage) && liquid && !water ? (input.fuel_level_pct ?? null) : null,
    meter_reading_milli: isUsage && water && meter ? parseQuantity(texts.meterReadingText) : null,
    period_start: isUsage && usageMode ? (input.period_start || null) : null,
    period_end: isUsage && usageMode ? (input.period_end || null) : null,
    estimated: isUsage ? (input.estimated ?? 0) : 0,
    meter_reset: isUsage && meter ? (input.meter_reset ?? 0) : 0,
    start_counter: isTrip ? input.start_counter : null,
    from_place: isTrip || isSession ? (texts.fromText.trim() || null) : null,
    to_place: isTrip ? (texts.toText.trim() || null) : null,
    duration_minutes: isTrip || isSession ? parseDuration(texts.durationText) : null,
    battery_used_pct: isTrip ? input.battery_used_pct : null,
  };
}

/** The row shown while a create is still in the outbox. */
export function optimisticActivity(tempId: number, oid: number, body: ActivityInput): Activity {
  const now = new Date().toISOString();
  return {
    id: tempId, object_id: oid, ...body, tags: body.tags ?? [],
    start_counter: body.start_counter ?? null, from_place: body.from_place ?? null,
    to_place: body.to_place ?? null, duration_minutes: body.duration_minutes ?? null,
    battery_used_pct: body.battery_used_pct ?? null, charged_full: body.charged_full ?? 0, weight_grams: body.weight_grams ?? null,
    fuel_level_pct: body.fuel_level_pct ?? null,
    meter_reading_milli: body.meter_reading_milli ?? null, period_start: body.period_start ?? null, period_end: body.period_end ?? null,
    estimated: body.estimated ?? 0, meter_reset: body.meter_reset ?? 0,
    created_at: now, updated_at: now, attachments: [], pending: true,
  };
}
```

Move the comments from the component's `buildInput` (the reasons for each gate) onto the
matching lines here; they explain the backend's rejections and belong with the logic.

Run: `npm test -- activity-form`
Expected: PASS.

- [ ] **Step 3: Use them in the component**

In `ActivityForm.svelte`:

- 79-88: `const counterWarn = $derived(counterBelowLast(counterText, lastCounter));`,
  `const weightWarn = $derived(weightDeviates(input.category, weightText, weightUnit, object?.stats.latest_weight_grams));`,
  `const photoDate = $derived(firstExifDate(attachments));`.
- 94-97: `function categoryParam() { return parseCategoryParam(location.search); }`.
- 117-134 and 176-188: both blocks become
  ```ts
  const picked = resolveCategory(list, categoryParam(), input.category);
  input.category = picked.category;
  if (picked.touched) categoryTouched = true;
  ```
  (in the `$effect`, keep `untrack(() => input.category)` as the third argument).
- 148-159:
  ```ts
  const f = activityToFormText(a, weightUnit);
  weightText = f.weightText; costText = f.costText; counterText = f.counterText; quantityText = f.quantityText;
  meterReadingText = f.meterReadingText; chargedFull = f.chargedFull; fromText = f.fromText; toText = f.toText;
  durationText = f.durationText; distance = f.distance;
  originalWeightText = weightText; originalWeightUnit = weightUnit; originalWeight = a.weight_grams ?? null;
  ```
- 261-266: `function mintTempId() { return hashToNegativeId(newOpId()); }` keeping the comment.
- 295-305: `saved = result ?? optimisticActivity(tempId, oid, body);`.
- 310-318:
  ```ts
  function changeWeightUnit(next: WeightUnit) {
    const s = changeWeightUnitState({ text: weightText, unit: weightUnit, originalText: originalWeightText, originalUnit: originalWeightUnit, original: originalWeight }, next);
    weightText = s.text; weightUnit = s.unit; originalWeightText = s.originalText; originalWeightUnit = s.originalUnit; originalWeight = s.original;
  }
  ```
- 320-362:
  ```ts
  function buildInput(): ActivityInput {
    return buildActivityInput(
      input,
      { weightText, costText, counterText, quantityText, meterReadingText, fromText, toText, durationText, chargedFull },
      { text: weightText, unit: weightUnit, originalText: originalWeightText, originalUnit: originalWeightUnit, original: originalWeight },
      object, $t,
    );
  }
  ```
- Drop imports the component no longer uses (`parseWeight`, `weightInput`, `changeWeightUnitValue`,
  `centsToInput`, `parseMoney`, `parseQuantity`, `formatDuration`, `parseDuration`, `tripDistance`,
  `exifDate`, `CATEGORIES`) — `svelte-check` reports each unused one.

- [ ] **Step 4: Gate and commit**

Run: `npm run check && npm test && npm run build && npx playwright test 02-lifecycle 16-form-errors 28-trips 29-charging 30-weight 31-water`
Expected: 0 errors, all tests pass, the six specs that drive the activity form pass.

```bash
git add frontend/src/lib/activity-form.ts frontend/src/routes/ActivityForm.svelte frontend/tests/activity-form.test.ts
git commit -m "refactor: move the activity form's pure logic into its lib module"
```

### Task 13: ObjectDetail logic into lib/object-detail.ts

**Files:**
- Create: `frontend/src/lib/object-detail.ts`, `frontend/tests/object-detail.test.ts`
- Modify: `frontend/src/routes/ObjectDetail.svelte:41-43, 49-53, 174-230, 364-372`

**Interfaces:**
- Consumes: `hashToNegativeId` from `lib/activity-form.ts`, `foldTag` from `lib/tags.ts`.
- Produces: `tagParam(search: string): string | null`, `offersTrip(o: MemObject | null): boolean`,
  `offersEnergy(o: MemObject | null): boolean`, `resourceCategory(o: MemObject | null): 'usage' | 'fuel'`,
  `foldTitle(title: string): string`, `pendingToActivity(op: QueuedOp, oid: number): Activity`,
  `filterPendingOps(ops: QueuedOp[], f: { category: string | null; tagFilter: string | null; titleFilter: string | null }): QueuedOp[]`,
  `nextUrl(href: string, tab: string): string`.

- [ ] **Step 1: Tests**

`frontend/tests/object-detail.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { tagParam, offersTrip, offersEnergy, resourceCategory, foldTitle, pendingToActivity, filterPendingOps, nextUrl } from '../src/lib/object-detail';
import type { QueuedOp } from '../src/lib/outbox';

function op(id: string, body: Record<string, unknown>): QueuedOp {
  return { id, kind: 'activity.create', path: '/objects/1/activities', body, attempts: 0 };
}

describe('tagParam', () => {
  it('trims and treats blank as absent', () => {
    expect(tagParam('?tag=%20oil%20')).toBe('oil');
    expect(tagParam('?tag=')).toBeNull();
    expect(tagParam('')).toBeNull();
  });
});

describe('what an object offers', () => {
  it('trips need a distance unit', () => {
    expect(offersTrip({ counter_unit: 'km' } as never)).toBe(true);
    expect(offersTrip({ counter_unit: 'h' } as never)).toBe(false);
    expect(offersTrip(null)).toBe(false);
  });
  it('energy needs a fuel or resource unit', () => {
    expect(offersEnergy({ fuel_unit: 'kWh', resource_unit: undefined } as never)).toBe(true);
    expect(offersEnergy({ fuel_unit: null, resource_unit: 'l' } as never)).toBe(true);
    expect(offersEnergy({ fuel_unit: null } as never)).toBe(false);
  });
  it('a resource logs usage, everything else fuel', () => {
    expect(resourceCategory({ resource_kind: 'water' } as never)).toBe('usage');
    expect(resourceCategory({} as never)).toBe('fuel');
  });
});

describe('pendingToActivity', () => {
  it('maps a full body and marks the row pending with a negative id', () => {
    const a = pendingToActivity(op('x', { date: '2026-01-01', category: 'trip', title: 't', notes: 'n', counter_value: 600, start_counter: 400, from_place: 'A', to_place: 'B', duration_minutes: 30, battery_used_pct: 5, charged_full: 1, fuel_level_pct: 50, meter_reading_milli: 10, period_start: '2026-01-01', period_end: '2026-01-31', estimated: 1, meter_reset: 1, tags: ['a'], cost_cents: 5, quantity_milli: 7, weight_grams: 8 }), 9);
    expect(a.id).toBeLessThan(0);
    expect(a).toMatchObject({ object_id: 9, pending: true, category: 'trip', title: 't', notes: 'n', counter_value: 600, start_counter: 400, from_place: 'A', to_place: 'B', duration_minutes: 30, battery_used_pct: 5, charged_full: 1, fuel_level_pct: 50, meter_reading_milli: 10, period_start: '2026-01-01', period_end: '2026-01-31', estimated: 1, meter_reset: 1, tags: ['a'], cost_cents: 5, quantity_milli: 7, weight_grams: 8, attachments: [] });
  });
  it('defaults every wrongly typed field', () => {
    const a = pendingToActivity(op('y', { date: 5, category: undefined, title: 1, tags: 'nope', charged_full: 'x' }), 1);
    expect(a).toMatchObject({ category: 'other', title: '', notes: '', counter_value: null, charged_full: 0, tags: [], estimated: 0, meter_reset: 0 });
    expect(a.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe('filterPendingOps', () => {
  const ops = [op('1', { category: 'repair', title: ' Oil ', tags: ['Fahrräder'] }), op('2', { category: 'fuel', title: 'x', tags: [] })];
  it('matches category, folded tag and folded title', () => {
    expect(filterPendingOps(ops, { category: 'repair', tagFilter: null, titleFilter: null }).map((o) => o.id)).toEqual(['1']);
    expect(filterPendingOps(ops, { category: null, tagFilter: 'fahrrader', titleFilter: null }).map((o) => o.id)).toEqual(['1']);
    expect(filterPendingOps(ops, { category: null, tagFilter: null, titleFilter: 'oil' }).map((o) => o.id)).toEqual(['1']);
    expect(filterPendingOps(ops, { category: null, tagFilter: null, titleFilter: null })).toHaveLength(2);
  });
});

describe('nextUrl', () => {
  it('drops the default tab and any tag, keeps another tab', () => {
    expect(nextUrl('https://x/objects/5?tag=oil&tab=timeline', 'timeline')).toBe('/objects/5');
    expect(nextUrl('https://x/objects/5?tag=oil', 'info')).toBe('/objects/5?tab=info');
  });
});

it('foldTitle trims and lowercases', () => {
  expect(foldTitle('  Oil Change ')).toBe('oil change');
});
```

Run: `npm test -- object-detail`
Expected: FAIL, module missing.

- [ ] **Step 2: Create `lib/object-detail.ts`**

Move the functions from `ObjectDetail.svelte` verbatim, parameterising what they read from
component scope:

```ts
import { hashToNegativeId } from './activity-form';
import { foldTag } from './tags';
import type { QueuedOp } from './outbox';
import type { Activity, ActivityInput, Category, MemObject } from './types';

/** The `?tag=` of a search string, trimmed; blank or absent is `null`. */
export function tagParam(search: string): string | null {
  return new URLSearchParams(search).get('tag')?.trim() || null;
}

/** Whether a trip can be logged here at all -- the same condition `categoriesFor`'s
 *  `counterUnit` argument checks, so the button and the form's category list never disagree. */
export function offersTrip(o: MemObject | null): boolean {
  return o?.counter_unit === 'km' || o?.counter_unit === 'mi';
}

export function offersEnergy(o: MemObject | null): boolean {
  return (o?.resource_unit ?? o?.fuel_unit) != null;
}

export function resourceCategory(o: MemObject | null): 'usage' | 'fuel' {
  return o?.resource_kind ? 'usage' : 'fuel';
}

/** The same fold the server's `title` filter uses (`fold_title` in `api/activities`). */
export function foldTitle(title: string): string {
  return title.trim().toLowerCase();
}

/** A queued 'activity.create' has no server row yet, so it renders straight from what the form
 *  queued. `pending: true` tells `Timeline` to dim it and refuse navigation into its fake id. */
export function pendingToActivity(op: QueuedOp, oid: number): Activity {
  const b = op.body as Partial<ActivityInput>;
  const now = new Date().toISOString();
  return {
    id: hashToNegativeId(op.id), object_id: oid,
    date: typeof b.date === 'string' ? b.date : now.slice(0, 10),
    category: (b.category as Category) ?? 'other',
    title: typeof b.title === 'string' ? b.title : '',
    notes: typeof b.notes === 'string' ? b.notes : '',
    weight_grams: typeof b.weight_grams === 'number' ? b.weight_grams : null,
    counter_value: typeof b.counter_value === 'number' ? b.counter_value : null,
    cost_cents: typeof b.cost_cents === 'number' ? b.cost_cents : null,
    quantity_milli: typeof b.quantity_milli === 'number' ? b.quantity_milli : null,
    start_counter: typeof b.start_counter === 'number' ? b.start_counter : null,
    from_place: typeof b.from_place === 'string' ? b.from_place : null,
    to_place: typeof b.to_place === 'string' ? b.to_place : null,
    duration_minutes: typeof b.duration_minutes === 'number' ? b.duration_minutes : null,
    battery_used_pct: typeof b.battery_used_pct === 'number' ? b.battery_used_pct : null,
    charged_full: typeof b.charged_full === 'number' ? b.charged_full : 0,
    fuel_level_pct: typeof b.fuel_level_pct === 'number' ? b.fuel_level_pct : null,
    meter_reading_milli: typeof b.meter_reading_milli === 'number' ? b.meter_reading_milli : null,
    period_start: typeof b.period_start === 'string' ? b.period_start : null,
    period_end: typeof b.period_end === 'string' ? b.period_end : null,
    estimated: typeof b.estimated === 'number' ? b.estimated : 0,
    meter_reset: typeof b.meter_reset === 'number' ? b.meter_reset : 0,
    created_at: now, updated_at: now, attachments: [],
    pending: true, tags: Array.isArray(b.tags) ? b.tags : [],
  };
}

/** The server filters the loaded page by category, tag and title; a queued entry has not
 *  reached it, so it is filtered here the same way. */
export function filterPendingOps(ops: QueuedOp[], f: { category: string | null; tagFilter: string | null; titleFilter: string | null }): QueuedOp[] {
  const wantedTag = f.tagFilter === null ? null : foldTag(f.tagFilter);
  const wantedTitle = f.titleFilter === null ? null : foldTitle(f.titleFilter);
  return ops
    .filter((o) => !f.category || o.body.category === f.category)
    .filter((o) => wantedTag === null || (Array.isArray(o.body.tags) && (o.body.tags as string[]).some((x) => foldTag(x) === wantedTag)))
    .filter((o) => wantedTitle === null || (typeof o.body.title === 'string' && foldTitle(o.body.title) === wantedTitle));
}

/** The address for the current tab: the default tab is left out and `?tag=` is dropped, since
 *  the tag filter is session state once read. */
export function nextUrl(href: string, tab: string): string {
  const url = new URL(href);
  if (tab === 'timeline') url.searchParams.delete('tab'); else url.searchParams.set('tab', tab);
  url.searchParams.delete('tag');
  return url.pathname + url.search;
}
```

In the component: `tagParam()` becomes `tagParam(location.search)` at both call sites; the three
`$derived` at 49-53 call the lib functions with `object`; delete `pendingId`, `pendingToActivity`,
`foldTitle`; `pendingActivities` becomes

```ts
  async function pendingActivities(): Promise<Activity[]> {
    const ops = await pendingOpsFor(`/objects/${oid}/activities`);
    return filterPendingOps(ops, { category, tagFilter, titleFilter }).map((o) => pendingToActivity(o, oid));
  }
```

and the URL effect at 364-372 becomes

```ts
  $effect(() => {
    const next = nextUrl(location.href, tab);
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });
```

Keep the existing comments above each replaced block. Remove `foldTag` from the component's
imports if nothing else there uses it.

- [ ] **Step 3: Gate and commit**

Run: `npm run check && npm test && npm run build && npx playwright test 05-pagination 19-offline-edit 22-objects-list 23-tags 27-last-done`
Expected: green.

```bash
git add frontend/src/lib/object-detail.ts frontend/tests/object-detail.test.ts frontend/src/routes/ObjectDetail.svelte
git commit -m "refactor: move the object page's pure logic into a lib module"
```

### Task 14: api.ts transport and outbox halves

**Files:**
- Create: `frontend/src/lib/api-outbox.ts`
- Modify: `frontend/src/lib/api.ts`
- Modify: `frontend/tests/outbox-replay.test.ts:2`

**Interfaces:**
- `api.ts` exports `handle` (was private) and keeps every existing export by re-exporting
  `api-outbox.ts`.

- [ ] **Step 1: Move lines 303-778**

```bash
cd frontend/src/lib
sed -n '303,778p' api.ts > api-outbox.ts
sed -i '303,778d' api.ts
```

Prepend to `api-outbox.ts`:

```ts
/**
 * The offline outbox's API surface: queued writes, the flush pass and the queue reads the UI
 * shows. Split from `api.ts`, which keeps the plain transport (`api`, `apiPage`, `upload`), the
 * stale-response tracking and the session hooks. Everything here is re-exported from `api.ts`,
 * so callers import from there as before.
 */
import { createLock, enqueue, newOpId, pendingCount, removeQueuedActivity, removeQueuedObject, replay, serialize, SkipOp, updateQueuedActivityBody, updateQueuedObjectBody, type OutboxStore, type QueuedOp } from './outbox';
import { idbStore } from './idb';
import { ApiError, isRejection } from './api-error';
import { api, isOurs, outboxUser, sendGateOpen, upload } from './api';
```

The moved half reads exactly five things from the transport half: `api` (11 uses), `upload`
(14), `isOurs` (5), `currentUserId` (7) and `sessionConfirmed` (1). The last two are `let`
bindings, which cannot be re-exported live, so add to `api.ts` right after `setOutboxSendGate`:

```ts
/** The outbox half (`api-outbox.ts`) reads these two through functions: a `let` cannot be
 *  shared across modules, and the values change at sign-in and sign-out. */
export function outboxUser(): number | null { return currentUserId; }
export function sendGateOpen(): boolean { return sessionConfirmed(); }
```

make `isOurs` `export function isOurs(...)`, and in `api-outbox.ts` replace each read of
`currentUserId` with `outboxUser()` and the one `sessionConfirmed()` call with `sendGateOpen()`
(`grep -n "currentUserId\|sessionConfirmed" api-outbox.ts` must then print nothing). Trim
`api.ts`'s line 2 to the `./outbox` names the transport half still uses and drop `idbStore` from
it; `svelte-check` lists each unused import. `handle` stays private to `api.ts`: nothing in the
moved half calls it directly.

Append to `api.ts`:

```ts
export * from './api-outbox';
```

- [ ] **Step 2: Point the outbox test at the new module**

`frontend/tests/outbox-replay.test.ts:2`: change `'../src/lib/api'` to `'../src/lib/api-outbox'`
for every name except `ApiError` and `setUnauthorizedHandler`, which stay from `'../src/lib/api'`
(two import lines).

- [ ] **Step 3: Gate and commit**

Run: `npm run check && npm test && npm run build && npx playwright test 04-offline 06-outbox-order 19-offline-edit 25-offline-cache`
Expected: 0 errors; the same 526 + new tests pass; offline specs green.

```bash
git add frontend/src/lib/api.ts frontend/src/lib/api-outbox.ts frontend/tests/outbox-replay.test.ts
git commit -m "refactor: split the outbox half out of lib/api"
```

---

## Track D (part 2): upgrade notes

### Task 15: docs/upgrading.md

**Files:**
- Create: `docs/upgrading.md`
- Modify: `README.md:36-86, 143-145`

- [ ] **Step 1: Build the file from the README's own lines**

Line numbers refer to `README.md` at commit `2878ed1`; confirm with `sed -n '51p;86p;143p;145p' README.md`
(expected: the `### Upgrading to 0.3.0` heading, the `would actually happen.` line, and the
two lines beginning `Weight and unit preferences` / `while preserving its data`).

```bash
{
  printf '# Upgrading\n\nA new image applies any pending database migrations when it starts. Take a snapshot first\n(README → Backup) whenever a release below says so.\n\n## 0.13.0: body weight\n\n'
  sed -n '143,145p' README.md
  printf '\n## 0.3.0: object types\n\n'
  sed -n '53,86p' README.md
} > docs/upgrading.md
sed -i '143,145d;51,87d' README.md
```

(`53,86` skips the heading and its blank line; `51,87` removes the heading through the blank
line after the section.) Then, after README line 42 ("release notes mention a schema change —
see [Backup](#backup)."), add a paragraph: "Release-specific notes live in
[docs/upgrading.md](docs/upgrading.md)." Read the result once: the "Weight tracking" section
must end on the line about older archives defaulting to kg, and "On a phone" must follow
"Released images" directly.

- [ ] **Step 2: Check no link broke**

Run: `grep -n "upgrading-to\|#upgrading" README.md docs/*.md frontend/src -r`
Expected: no anchor references to the removed heading.

- [ ] **Step 3: Commit**

```bash
git add docs/upgrading.md README.md
git commit -m "docs: move release upgrade notes out of the README"
```

---

## Final gate

After the last task:

```bash
cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
cd frontend && npm run check && npm test && npm run build && npx playwright test
```

and the postgres suite as in Task 9 Step 3. Tracks A and B are user-visible, so the next
release is `0.16.0`; the version bump (`Cargo.toml`, `Cargo.lock`, `frontend/package.json`,
`frontend/package-lock.json`) and the tag are done only when asked.
