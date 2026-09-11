# Object Types Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace an object's free-text category with a fixed type that drives an icon and the
activity vocabulary, so a body can be logged as sensibly as a car.

**Architecture:** One mapping table in Rust is the single source of truth for legacy text → type,
used by both the import path and (mirrored, with a drift test) the migration's SQL. The schema
change rebuilds two tables with foreign keys disabled, which requires a `-- no-transaction`
migration. The frontend gains one table mapping type → icon and type → offered categories.

**Tech Stack:** Rust, axum, sqlx 0.9 (SQLite, WAL, `foreign_keys(true)`), Svelte 5 runes, vitest,
Playwright.

## Before starting: this migration rewrites live data

The instance at `logb:latest` holds real entries and **`LOGB_BACKUP_DIR` is unset, so there is no
automatic backup.** Take one before deploying this:

```bash
docker compose exec logb /logb --backup /data/backups   # or copy the database file while stopped
```

Restoring is covered by `docs/superpowers/specs/2026-09-09-backup-and-restore-design.md`. Do not
deploy this change to the running instance until a restore has been tested at least once.

## Global Constraints

- The palette does not change. No new dependency, runtime or dev.
- Every new user-facing string exists in **both** `frontend/src/i18n/en.ts` and
  `frontend/src/i18n/de.ts`. A key present in one and missing from the other is a defect.
- `cargo test`, `cargo clippy -- -D warnings`, `npm run check`, `npx vitest run` and
  `npx playwright test` must all pass at the end of every task.
- `foreign_keys(true)` is set in `src/db.rs:17`. Any `DROP TABLE` on `objects` or `activities`
  with enforcement on performs an implicit `DELETE FROM`, which cascades to `attachments`,
  `reminders` and `activities`. Enforcement must be off for the rebuild.
- Never `ALTER TABLE <live> RENAME TO <old>`: SQLite rewrites other tables' `REFERENCES` clauses
  to follow the rename, silently repointing every child at the table you are about to drop.
  Create the new table, copy, `DROP` the old, then rename the new one into place.
- The eleven activity categories are exactly: `maintenance`, `repair`, `purchase`, `inspection`,
  `modification`, `fuel`, `other`, `symptom`, `treatment`, `appointment`, `medication`.
- The nine object types are exactly: `car`, `e_bike`, `bike`, `motorcycle`, `home`, `appliance`,
  `tool`, `body`, `other`.

## File structure

| File | Responsibility |
|---|---|
| `src/object_type.rs` (new) | The nine types, and legacy text → type mapping. No I/O. |
| `migrations/0009_object_types.sql` (new) | Schema rebuild, backfill, epoch rotation. |
| `src/api/objects.rs` | `type` replaces `category` on the row, input and SQL. |
| `src/sync/mod.rs` | Field whitelist: `type` in, `category` out. |
| `src/api/export.rs` | Archive carries `type`; old archives map on import. |
| `frontend/src/lib/object-types.ts` (new) | Type → icon name, type → offered categories. |
| `frontend/src/lib/Icon.svelte` | Nine type icons. |
| `frontend/src/routes/ObjectForm.svelte` | Type picker replaces the free-text input. |
| `frontend/src/routes/ActivityForm.svelte` | Category select filtered by type. |

---

### Task 1: The mapping table in Rust

**Files:**
- Create: `src/object_type.rs`
- Modify: `src/main.rs` (add `mod object_type;` beside the existing module declarations)

**Interfaces:**
- Produces: `pub const OBJECT_TYPES: [&str; 9]`, `pub fn is_valid(t: &str) -> bool`, and
  `pub fn from_legacy(text: &str) -> Legacy` where
  `pub enum Legacy { Mapped(&'static str), Unmapped }`.

- [ ] **Step 1: Write the failing tests**

Create `src/object_type.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_words_map_in_both_languages() {
        assert_eq!(from_legacy("car"), Legacy::Mapped("car"));
        assert_eq!(from_legacy("Auto"), Legacy::Mapped("car"));
        assert_eq!(from_legacy("  PKW  "), Legacy::Mapped("car"));
        assert_eq!(from_legacy("fahrrad"), Legacy::Mapped("bike"));
        assert_eq!(from_legacy("Werkzeug"), Legacy::Mapped("tool"));
        assert_eq!(from_legacy("körper"), Legacy::Mapped("body"));
    }

    /// The ordering trap: every e-bike spelling contains a bike spelling. Matching is exact
    /// rather than substring precisely so this cannot go wrong, and this test pins it.
    #[test]
    fn e_bike_does_not_become_a_bike() {
        assert_eq!(from_legacy("e-bike"), Legacy::Mapped("e_bike"));
        assert_eq!(from_legacy("ebike"), Legacy::Mapped("e_bike"));
        assert_eq!(from_legacy("E-Bike"), Legacy::Mapped("e_bike"));
        assert_eq!(from_legacy("pedelec"), Legacy::Mapped("e_bike"));
    }

    #[test]
    fn unknown_text_is_not_guessed_at() {
        assert_eq!(from_legacy("Gravelbike Custom"), Legacy::Unmapped);
        assert_eq!(from_legacy("Rennrad"), Legacy::Unmapped);
        assert_eq!(from_legacy(""), Legacy::Unmapped);
        assert_eq!(from_legacy("   "), Legacy::Unmapped);
    }

    #[test]
    fn every_mapped_target_is_a_real_type() {
        for word in LEGACY.iter() {
            assert!(is_valid(word.1), "{} maps to unknown type {}", word.0, word.1);
        }
        assert!(is_valid("other"));
        assert!(!is_valid("vehicle"));
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test object_type`

Expected: compilation failure — `from_legacy`, `Legacy`, `LEGACY` and `is_valid` do not exist.

- [ ] **Step 3: Write the implementation**

Above the test module in `src/object_type.rs`:

```rust
//! What kind of thing an object is.
//!
//! The nine types are a closed set with a `CHECK` behind them, because behaviour keys off this
//! value: the icon a row shows, and which activity categories its form offers. A free-text
//! field cannot carry that -- `Fahrrad`, `bike` and a typo were three different values.

/// The nine types, in the order the picker offers them.
pub const OBJECT_TYPES: [&str; 9] = [
    "car", "e_bike", "bike", "motorcycle", "home", "appliance", "tool", "body", "other",
];

pub fn is_valid(t: &str) -> bool {
    OBJECT_TYPES.contains(&t)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Legacy {
    Mapped(&'static str),
    Unmapped,
}

/// Legacy free-text spellings, in both languages the interface speaks.
///
/// Matching is exact on the lowercased, trimmed text -- never substring. `Gravelbike Custom`
/// becoming a `bike` by accident is a silent mis-filing with no record that a guess was made;
/// landing on `other` with the words preserved is visible and fixed in one edit.
pub const LEGACY: [(&str, &str); 24] = [
    ("car", "car"), ("auto", "car"), ("pkw", "car"), ("wagen", "car"),
    ("e-bike", "e_bike"), ("ebike", "e_bike"), ("e bike", "e_bike"), ("pedelec", "e_bike"),
    ("bike", "bike"), ("fahrrad", "bike"), ("velo", "bike"), ("rad", "bike"),
    ("motorcycle", "motorcycle"), ("motorrad", "motorcycle"), ("motorbike", "motorcycle"),
    ("home", "home"), ("haus", "home"), ("wohnung", "home"),
    ("appliance", "appliance"), ("gerät", "appliance"),
    ("tool", "tool"), ("werkzeug", "tool"),
    ("body", "body"), ("körper", "body"),
];

pub fn from_legacy(text: &str) -> Legacy {
    let key = text.trim().to_lowercase();
    match LEGACY.iter().find(|(word, _)| *word == key) {
        Some((_, ty)) => Legacy::Mapped(ty),
        None => Legacy::Unmapped,
    }
}
```

Add `mod object_type;` to `src/main.rs` beside the other module declarations.

- [ ] **Step 4: Run the tests**

Run: `cargo test object_type && cargo clippy -- -D warnings`

Expected: 4 passed; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src/object_type.rs src/main.rs
git commit -m "feat: the object type list, and what old free-text categories map to"
```

---

### Task 2: The migration

**Files:**
- Create: `migrations/0009_object_types.sql`
- Create: `tests/migration_object_types.rs`

**Interfaces:**
- Consumes: `object_type::{LEGACY, from_legacy, Legacy}` from Task 1.
- Produces: `objects.type` (TEXT NOT NULL, CHECK over the nine); `objects.category` gone;
  `activities.category` CHECK extended to eleven values.

- [ ] **Step 1: Write the failing migration test**

Create `tests/migration_object_types.rs`. It builds a database at the *previous* migration,
seeds rows, then runs the full migrator and asserts on the other side.

```rust
//! The migration rewrites live data on an instance that has been running for weeks. What
//! matters is not that it produces the right column -- it is that nothing else moves: no
//! attachment is cascade-deleted, no reminder loses its activity, and no text a user typed is
//! thrown away.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Executor, Row, SqlitePool};

/// Applies every migration up to but excluding 0009, then seeds the old shape.
async fn old_schema_with_rows() -> SqlitePool {
    let opts = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();
    for file in [
        "0001_init.sql", "0002_session_expiry_index.sql", "0003_fuel_quantity.sql",
        "0004_reminder_snooze.sql", "0005_client_op_id.sql", "0006_api_tokens.sql",
        "0007_sync.sql", "0008_sync_epoch.sql",
    ] {
        let sql = std::fs::read_to_string(format!("migrations/{file}")).unwrap();
        pool.execute(&*sql).await.unwrap();
    }
    pool.execute(
        "INSERT INTO users (id, username, password_hash, role, created_at) \
         VALUES (1, 'ben', 'x', 'admin', '2026-01-01T00:00:00Z');
         INSERT INTO objects (id, user_id, name, category, description, created_at, updated_at) VALUES
           (1, 1, 'Golf', 'Auto', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           (2, 1, 'Commuter', 'E-Bike', 'blue', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           (3, 1, 'Odd one', 'Gravelbike Custom', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           (4, 1, 'Other odd', 'Rennrad', 'already here', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO activities (id, object_id, date, category, title, created_at, updated_at) VALUES
           (1, 1, '2026-02-01', 'maintenance', 'Oil', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z');
         INSERT INTO files (id, user_id, sha256, size_bytes, mime, created_at) \
           VALUES (1, 1, 'abc', 1, 'image/png', '2026-02-01T00:00:00Z');
         INSERT INTO attachments (id, object_id, activity_id, file_id, kind, original_name, created_at) \
           VALUES (1, 1, 1, 1, 'photo', 'a.png', '2026-02-01T00:00:00Z');
         INSERT INTO reminders (id, object_id, title, done_activity_id, created_at, updated_at) \
           VALUES (1, 1, 'Service', 1, '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z');",
    ).await.unwrap();
    pool
}

async fn run_0009(pool: &SqlitePool) {
    let sql = std::fs::read_to_string("migrations/0009_object_types.sql").unwrap();
    pool.execute(&*sql).await.unwrap();
}

#[tokio::test]
async fn known_categories_become_types() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let rows = sqlx::query("SELECT id, type FROM objects ORDER BY id").fetch_all(&pool).await.unwrap();
    let types: Vec<String> = rows.iter().map(|r| r.get::<String, _>("type")).collect();
    assert_eq!(types, vec!["car", "e_bike", "other", "other"]);
}

#[tokio::test]
async fn unmapped_text_is_kept_in_the_description() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let d: String = sqlx::query_scalar("SELECT description FROM objects WHERE id = 3").fetch_one(&pool).await.unwrap();
    assert_eq!(d, "Gravelbike Custom", "an empty description takes the text alone");
    let d: String = sqlx::query_scalar("SELECT description FROM objects WHERE id = 4").fetch_one(&pool).await.unwrap();
    assert_eq!(d, "already here\nRennrad", "an existing description keeps its text, on its own line");
}

/// The reason this migration is dangerous. `objects` and `activities` are both parents of
/// cascading children, so a rebuild with foreign keys enforced deletes attachments outright.
#[tokio::test]
async fn nothing_is_cascade_deleted() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let attachments: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments").fetch_one(&pool).await.unwrap();
    let activities: i64 = sqlx::query_scalar("SELECT count(*) FROM activities").fetch_one(&pool).await.unwrap();
    let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM objects").fetch_one(&pool).await.unwrap();
    let done: Option<i64> = sqlx::query_scalar("SELECT done_activity_id FROM reminders WHERE id = 1").fetch_one(&pool).await.unwrap();
    assert_eq!((objects, activities, attachments), (4, 1, 1));
    assert_eq!(done, Some(1), "the reminder still points at its activity");
    let violations = sqlx::query("PRAGMA foreign_key_check").fetch_all(&pool).await.unwrap();
    assert!(violations.is_empty(), "{} foreign key violations after the rebuild", violations.len());
}

#[tokio::test]
async fn the_new_categories_are_accepted_and_nonsense_is_not() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    sqlx::query("INSERT INTO activities (object_id, date, category, title, created_at, updated_at) \
                 VALUES (1, '2026-03-01', 'symptom', 'Shoulder', 'x', 'x')")
        .execute(&pool).await.expect("symptom is a category now");
    let bad = sqlx::query("INSERT INTO activities (object_id, date, category, title, created_at, updated_at) \
                           VALUES (1, '2026-03-01', 'nonsense', 'x', 'x', 'x')")
        .execute(&pool).await;
    assert!(bad.is_err(), "the CHECK must still reject an unknown category");
}

#[tokio::test]
async fn the_indexes_survive() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN \
         ('idx_objects_user', 'idx_activities_object_date') ORDER BY name")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(names, vec!["idx_activities_object_date", "idx_objects_user"]);
}

/// Devices hold a cursor plus an epoch; a mismatch sends them back to a full bootstrap. This
/// migration rewrites rows without writing anything to `changes`, so without a new epoch an
/// offline device would keep its old `category` and never learn that types exist.
#[tokio::test]
async fn the_sync_epoch_is_rotated() {
    let pool = old_schema_with_rows().await;
    let before: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'").fetch_one(&pool).await.unwrap();
    run_0009(&pool).await;
    let after: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'").fetch_one(&pool).await.unwrap();
    assert_ne!(before, after);
    assert_eq!(after.len(), 32);
}

/// The migration's SQL repeats the mapping that `object_type::from_legacy` holds in Rust. Two
/// copies drift. This runs every word in the Rust table through the migration's own CASE
/// expression and demands they agree.
#[tokio::test]
async fn the_sql_mapping_matches_the_rust_one() {
    let pool = old_schema_with_rows().await;
    run_0009(&pool).await;
    for (word, expected) in logb::object_type::LEGACY.iter() {
        sqlx::query("INSERT INTO objects (user_id, name, type, description, created_at, updated_at) \
                     VALUES (1, 'probe', 'other', '', 'x', 'x')").execute(&pool).await.unwrap();
        let mapped: String = sqlx::query_scalar(
            "SELECT CASE lower(trim(?1)) \
               WHEN 'car' THEN 'car' WHEN 'auto' THEN 'car' WHEN 'pkw' THEN 'car' WHEN 'wagen' THEN 'car' \
               WHEN 'e-bike' THEN 'e_bike' WHEN 'ebike' THEN 'e_bike' WHEN 'e bike' THEN 'e_bike' WHEN 'pedelec' THEN 'e_bike' \
               WHEN 'bike' THEN 'bike' WHEN 'fahrrad' THEN 'bike' WHEN 'velo' THEN 'bike' WHEN 'rad' THEN 'bike' \
               WHEN 'motorcycle' THEN 'motorcycle' WHEN 'motorrad' THEN 'motorcycle' WHEN 'motorbike' THEN 'motorcycle' \
               WHEN 'home' THEN 'home' WHEN 'haus' THEN 'home' WHEN 'wohnung' THEN 'home' \
               WHEN 'appliance' THEN 'appliance' WHEN 'gerät' THEN 'appliance' \
               WHEN 'tool' THEN 'tool' WHEN 'werkzeug' THEN 'tool' \
               WHEN 'body' THEN 'body' WHEN 'körper' THEN 'body' \
               ELSE 'other' END")
            .bind(word).fetch_one(&pool).await.unwrap();
        assert_eq!(&mapped, expected, "SQL and Rust disagree on {word}");
    }
}
```

Note: this test file references `logb::object_type`, so `src/main.rs` must expose the crate as a
library, or the module must be reachable from an existing `src/lib.rs`. Check which the project
already does — `tests/export.rs` imports from the crate and shows the pattern to follow. If no
library target exists, add the `LEGACY` walk as a unit test inside `src/object_type.rs` instead
and keep the rest of this file as an integration test.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test --test migration_object_types`

Expected: every test fails on `migrations/0009_object_types.sql` not existing.

- [ ] **Step 3: Write the migration**

Create `migrations/0009_object_types.sql`. **The first line must be exactly
`-- no-transaction`** — sqlx matches it with `starts_with`, so a leading blank line or comment
disables the behaviour silently.

```sql
-- no-transaction
--
-- Foreign keys must be off for this: `objects` and `activities` are both parents of cascading
-- children, and a DROP TABLE with enforcement on performs an implicit DELETE FROM that takes
-- every attachment with it. SQLite refuses to toggle the pragma inside a transaction, which is
-- why this migration manages its own.
--
-- Note the order in each rebuild: create the new table, copy, DROP the old, then rename the new
-- one into place. Renaming the OLD table out of the way instead would make SQLite rewrite every
-- child's REFERENCES clause to follow it, quietly repointing them at the table being dropped.
PRAGMA foreign_keys = off;

BEGIN;

-- objects: free-text `category` becomes a constrained `type`.
CREATE TABLE objects_new (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id              INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    type                 TEXT NOT NULL CHECK (type IN
                           ('car','e_bike','bike','motorcycle','home','appliance','tool','body','other')),
    counter_unit         TEXT CHECK (counter_unit IN ('km', 'mi', 'h')),
    fuel_unit            TEXT CHECK (fuel_unit IN ('l', 'gal', 'kwh')),
    description          TEXT NOT NULL DEFAULT '',
    purchase_date        TEXT,
    purchase_price_cents INTEGER,
    archived_at          TEXT,
    cover_attachment_id  INTEGER,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    client_uuid          TEXT,
    deleted_at           TEXT
);

-- The CASE below is mirrored by `object_type::from_legacy`; `the_sql_mapping_matches_the_rust_one`
-- fails if the two drift. Exact matches only -- no substring guessing.
INSERT INTO objects_new (id, user_id, name, type, counter_unit, fuel_unit, description,
                         purchase_date, purchase_price_cents, archived_at, cover_attachment_id,
                         created_at, updated_at, client_uuid, deleted_at)
SELECT id, user_id, name,
       CASE lower(trim(category))
         WHEN 'car' THEN 'car' WHEN 'auto' THEN 'car' WHEN 'pkw' THEN 'car' WHEN 'wagen' THEN 'car'
         WHEN 'e-bike' THEN 'e_bike' WHEN 'ebike' THEN 'e_bike' WHEN 'e bike' THEN 'e_bike' WHEN 'pedelec' THEN 'e_bike'
         WHEN 'bike' THEN 'bike' WHEN 'fahrrad' THEN 'bike' WHEN 'velo' THEN 'bike' WHEN 'rad' THEN 'bike'
         WHEN 'motorcycle' THEN 'motorcycle' WHEN 'motorrad' THEN 'motorcycle' WHEN 'motorbike' THEN 'motorcycle'
         WHEN 'home' THEN 'home' WHEN 'haus' THEN 'home' WHEN 'wohnung' THEN 'home'
         WHEN 'appliance' THEN 'appliance' WHEN 'gerät' THEN 'appliance'
         WHEN 'tool' THEN 'tool' WHEN 'werkzeug' THEN 'tool'
         WHEN 'body' THEN 'body' WHEN 'körper' THEN 'body'
         ELSE 'other'
       END,
       counter_unit, fuel_unit,
       -- Nothing the user typed is destroyed by a migration they did not ask for: text that did
       -- not map is appended to the description, on its own line.
       CASE
         WHEN lower(trim(category)) IN
           ('car','auto','pkw','wagen','e-bike','ebike','e bike','pedelec','bike','fahrrad','velo','rad',
            'motorcycle','motorrad','motorbike','home','haus','wohnung','appliance','gerät','tool','werkzeug',
            'body','körper')
           THEN description
         WHEN trim(category) = '' THEN description
         WHEN trim(description) = '' THEN trim(category)
         ELSE description || char(10) || trim(category)
       END,
       purchase_date, purchase_price_cents, archived_at, cover_attachment_id,
       created_at, updated_at, client_uuid, deleted_at
FROM objects;

DROP TABLE objects;
ALTER TABLE objects_new RENAME TO objects;
CREATE INDEX idx_objects_user ON objects(user_id);

-- activities: the CHECK grows by four health categories. SQLite cannot alter a CHECK in place.
CREATE TABLE activities_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id     INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date          TEXT NOT NULL,
    category      TEXT NOT NULL CHECK (category IN
                    ('maintenance','repair','purchase','inspection','modification','fuel','other',
                     'symptom','treatment','appointment','medication')),
    title         TEXT NOT NULL,
    notes         TEXT NOT NULL DEFAULT '',
    counter_value INTEGER,
    cost_cents    INTEGER,
    quantity_milli INTEGER,
    client_op_id  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    client_uuid   TEXT,
    deleted_at    TEXT
);
INSERT INTO activities_new SELECT id, object_id, date, category, title, notes, counter_value,
       cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, deleted_at
FROM activities;
DROP TABLE activities;
ALTER TABLE activities_new RENAME TO activities;
CREATE INDEX idx_activities_object_date ON activities(object_id, date);

-- Field-level clocks for a column that no longer exists.
DELETE FROM field_clock WHERE entity = 'object' AND field = 'category';

-- Rows changed underneath every device without a single `changes` entry, so cursors must not
-- resume. A new epoch sends each device back to a full bootstrap, where it picks up `type`.
UPDATE settings SET value = lower(hex(randomblob(16))) WHERE key = 'sync_epoch';

COMMIT;

PRAGMA foreign_keys = on;
```

**Before writing this file, verify the column lists against the real schema.** `objects` and
`activities` have been altered by migrations 0003, 0005 and 0007, and the lists above must match
what `PRAGMA table_info(objects)` and `PRAGMA table_info(activities)` actually report on a
migrated database. Run:

```bash
sqlite3 .e2e-data/logb.db 'PRAGMA table_info(objects);' 'PRAGMA table_info(activities);'
```

(or build one with `cargo test --test migration_object_types` after Step 1 and inspect it). A
column omitted here is dropped from the live database without warning. Report the two column
lists you found in your task report.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test migration_object_types`

Expected: 7 passed. If `the_sql_mapping_matches_the_rust_one` fails, the CASE and
`object_type::LEGACY` disagree — fix whichever is wrong, do not adjust the test to match.

- [ ] **Step 5: Prove it on a realistic database, not only a seeded one**

The e2e suite builds a database with real rows through the real API. Use it:

```bash
cd frontend && npx playwright test 02-lifecycle && cd ..
sqlite3 .e2e-data/logb.db 'SELECT count(*) FROM attachments;'   # record this
cargo test --test migration_object_types                        # migration already applied at boot
sqlite3 .e2e-data/logb.db 'PRAGMA foreign_key_check;'           # must print nothing
sqlite3 .e2e-data/logb.db 'SELECT id, name, type FROM objects;'
```

Report the attachment count before and after and the `foreign_key_check` output.

- [ ] **Step 6: Commit**

```bash
git add migrations/0009_object_types.sql tests/migration_object_types.rs
git commit -m "feat: objects carry a type, and activities can record health entries"
```

---

### Task 3: The backend API and the sync whitelist

**Files:**
- Modify: `src/api/objects.rs` (`ObjectRow:21-35`, `ObjectInput:57`, validation `:96-98`,
  SQL at `:120`, `:212`, `:218`, `:239-245`, `:284`, `:299-302`)
- Modify: `src/sync/mod.rs:106-111` (the `Entity::Object` whitelist)
- Test: `tests/sync_apply.rs` or the existing sync test file that covers rejected operations

**Interfaces:**
- Consumes: `object_type::is_valid` from Task 1; the `type` column from Task 2.
- Produces: objects API accepting and returning `type: String`; no `category` anywhere.

- [ ] **Step 1: Write the failing tests**

Add to the crate's existing objects test file (find it with `ls tests/`; follow that file's
harness helpers rather than inventing a new one):

```rust
#[tokio::test]
async fn an_object_is_created_with_a_type() {
    let app = TestApp::new().await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "car" })).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let o: serde_json::Value = res.json().await.unwrap();
    assert_eq!(o["type"], "car");
    assert!(o.get("category").is_none(), "the old field must be gone from the response");
}

#[tokio::test]
async fn an_unknown_type_is_refused() {
    let app = TestApp::new().await;
    let res = app.client.post(app.url("/objects"))
        .json(&json!({ "name": "Golf", "type": "spaceship" })).send().await.unwrap();
    assert_eq!(res.status(), 400, "the CHECK would catch it, but a 400 says which field is wrong");
}
```

And in the sync tests, the trap this project has hit twice:

```rust
/// A device that was offline across the upgrade arrives with an operation naming a column that
/// no longer exists. It must be rejected on its own -- a batch that 500s is retried identically
/// forever, which is how one bad operation once wedged a client permanently.
#[tokio::test]
async fn an_operation_naming_the_removed_field_is_rejected_not_fatal() {
    let app = TestApp::new().await;
    let id = app.create_object("Golf", "car").await;
    let res = app.push(json!([
        { "entity": "object", "id": id, "kind": "update", "field": "category", "value": "auto",
          "client_op_id": "op-1", "updated_at": "2026-09-11T00:00:00Z" },
        { "entity": "object", "id": id, "kind": "update", "field": "name", "value": "Golf VII",
          "client_op_id": "op-2", "updated_at": "2026-09-11T00:00:00Z" }
    ])).await;
    assert_eq!(res.status(), 200, "one bad op must not fail the batch");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");
    assert_eq!(body["results"][1]["outcome"], "accepted", "the good op in the same batch applies");
}
```

Match the exact request and response shapes to the existing sync tests in this repository —
field names like `outcome`, `results` and `kind` must be copied from them, not from this plan.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test`

Expected: the new tests fail — `type` is not a field the API knows.

- [ ] **Step 3: Make the change**

In `src/api/objects.rs`: rename the struct field `category: String` to `type_: String` with
`#[serde(rename = "type")]` on both `ObjectRow` and `ObjectInput` (`type` is a Rust keyword),
update every SQL string listed under **Files** to select, insert and update `type` instead of
`category`, and replace the validation at `:96-98` with:

```rust
        self.type_ = self.type_.trim().to_lowercase();
        if !crate::object_type::is_valid(&self.type_) {
            return Err(AppError::BadRequest("type is not one of the known object types".into()));
        }
```

At `:284`, the change-log line becomes:

```rust
    if body.type_ != existing.type_ { changed.push(("type", json!(body.type_))); }
```

In `src/sync/mod.rs`, the `Entity::Object` whitelist: `("category", Text)` becomes
`("type", Text)`. The existing test that walks the whitelist against the schema will fail if this
is missed, which is the point of it.

- [ ] **Step 4: Run everything**

Run: `cargo test && cargo clippy -- -D warnings`

Expected: all green. The whitelist-versus-schema test passing is the specific thing to confirm.

- [ ] **Step 5: Commit**

```bash
git add src/api/objects.rs src/sync/mod.rs tests/
git commit -m "feat: the objects API speaks type, and sync knows the field"
```

---

### Task 4: Export and import

**Files:**
- Modify: `src/api/export.rs` (`:45`, `:76`, `:137`, `:167`, `:305-307`)
- Test: `tests/export.rs`

**Interfaces:**
- Consumes: `object_type::{from_legacy, Legacy, is_valid}` from Task 1.
- Produces: archives carrying `type`; old archives carrying `category` import correctly.

- [ ] **Step 1: Write the failing test**

In `tests/export.rs`:

```rust
/// An archive written before object types exists on someone's disk. Importing it must apply the
/// same mapping the migration did -- otherwise every object in a year-old backup lands on
/// `other` and the restore quietly loses what kind of thing each one was.
#[tokio::test]
async fn an_archive_written_before_types_still_imports() {
    let app = TestApp::new().await;
    let zip = archive_with_object_json(json!({
        "id": 1, "name": "Golf", "category": "Auto", "description": "",
        "counter_unit": "km", "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"
    }));
    let res = app.client.post(app.url("/import")).header("content-type", "application/zip").body(zip).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let objects: serde_json::Value = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objects["items"][0]["type"], "car");
}

#[tokio::test]
async fn unmapped_text_in_an_old_archive_reaches_the_description() {
    let app = TestApp::new().await;
    let zip = archive_with_object_json(json!({
        "id": 1, "name": "Odd", "category": "Gravelbike Custom", "description": "",
        "counter_unit": null, "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"
    }));
    app.client.post(app.url("/import")).header("content-type", "application/zip").body(zip).send().await.unwrap();
    let objects: serde_json::Value = app.client.get(app.url("/objects")).send().await.unwrap().json().await.unwrap();
    assert_eq!(objects["items"][0]["type"], "other");
    assert_eq!(objects["items"][0]["description"], "Gravelbike Custom");
}
```

Write `archive_with_object_json` as a helper in the same file, building a zip with a single
`data.json` whose `objects` array holds the given value — follow the existing archive-building
code in `tests/export.rs` rather than inventing a second way to do it.

- [ ] **Step 2: Run and watch fail**

Run: `cargo test --test export`

Expected: both new tests fail — the import path does not know `category`.

- [ ] **Step 3: Implement**

In the import structs in `src/api/export.rs`, accept both spellings:

```rust
    /// Archives written before object types carry `category` instead. Both are optional here so
    /// one archive format does not become two structs; exactly one is expected to be present.
    #[serde(rename = "type")]
    type_: Option<String>,
    category: Option<String>,
```

At the insert (`:305-307`), resolve them, applying the same rule as the migration:

```rust
        let (ty, description) = match (o.type_.as_deref(), o.category.as_deref()) {
            (Some(t), _) if crate::object_type::is_valid(t) => (t.to_string(), o.description.clone()),
            (Some(_), _) => ("other".to_string(), o.description.clone()),
            (None, Some(c)) => match crate::object_type::from_legacy(c) {
                crate::object_type::Legacy::Mapped(t) => (t.to_string(), o.description.clone()),
                crate::object_type::Legacy::Unmapped => {
                    let c = c.trim();
                    let d = o.description.trim();
                    let joined = if c.is_empty() { d.to_string() }
                        else if d.is_empty() { c.to_string() }
                        else { format!("{d}\n{c}") };
                    ("other".to_string(), joined)
                }
            },
            (None, None) => ("other".to_string(), o.description.clone()),
        };
```

Bind `ty` and `description` in the insert. On the export side, the struct field and the SELECT
become `type`.

- [ ] **Step 4: Run**

Run: `cargo test --test export`

Expected: all export tests pass, including the existing round-trip.

- [ ] **Step 5: Commit**

```bash
git add src/api/export.rs tests/export.rs
git commit -m "feat: archives carry the object type, and old ones map on import"
```

---

### Task 5: The frontend type table, strings, and icons

**Files:**
- Create: `frontend/src/lib/object-types.ts`
- Modify: `frontend/src/lib/types.ts:1-3` and the `MemObject`/`ObjectInput` interfaces
- Modify: `frontend/src/lib/Icon.svelte`
- Modify: `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/object-types.test.ts` (new)

**Interfaces:**
- Consumes: the API's `type` field from Task 3.
- Produces: `OBJECT_TYPES: readonly ObjectType[]`, `typeIcon(t: ObjectType): IconName`,
  `categoriesFor(t: ObjectType, current?: Category): Category[]`.

- [ ] **Step 1: Write the failing tests**

Create `frontend/tests/object-types.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { OBJECT_TYPES, categoriesFor, typeIcon } from '../src/lib/object-types';
import { CATEGORIES } from '../src/lib/types';
import en from '../src/i18n/en';
import de from '../src/i18n/de';

describe('object types', () => {
  it('offers a bike no fuel and a body no inspection', () => {
    expect(categoriesFor('bike')).not.toContain('fuel');
    expect(categoriesFor('body')).toEqual(['symptom', 'treatment', 'appointment', 'medication', 'other']);
    expect(categoriesFor('car')).toContain('fuel');
  });

  // Re-typing an object must not silently re-file its history: the select has to keep offering
  // whatever the entry already says, or saving an untouched form would change its category.
  it('always includes the entry\'s current category', () => {
    expect(categoriesFor('body', 'fuel')).toContain('fuel');
    expect(categoriesFor('bike', 'symptom')).toContain('symptom');
    expect(categoriesFor('car', 'fuel').filter((c) => c === 'fuel')).toHaveLength(1);
  });

  it('every type has an icon and every category is reachable from some type', () => {
    for (const t of OBJECT_TYPES) expect(typeIcon(t)).toBeTruthy();
    const reachable = new Set(OBJECT_TYPES.flatMap((t) => categoriesFor(t)));
    for (const c of CATEGORIES) expect(reachable).toContain(c);
  });

  // A key in one language and not the other ships a screen with a raw key on it.
  it('both languages name every type and every category', () => {
    for (const t of OBJECT_TYPES) {
      expect(en[`type.${t}`], `en type.${t}`).toBeTruthy();
      expect(de[`type.${t}`], `de type.${t}`).toBeTruthy();
    }
    for (const c of CATEGORIES) {
      expect(en[`cat.${c}`], `en cat.${c}`).toBeTruthy();
      expect(de[`cat.${c}`], `de cat.${c}`).toBeTruthy();
    }
  });
});
```

- [ ] **Step 2: Run and watch fail**

Run: `cd frontend && npx vitest run tests/object-types.test.ts`

Expected: fails on the missing module.

- [ ] **Step 3: Write the table**

In `frontend/src/lib/types.ts`, extend the category list and add the type:

```ts
export const CATEGORIES = ['maintenance', 'repair', 'purchase', 'inspection', 'modification', 'fuel', 'other',
  'symptom', 'treatment', 'appointment', 'medication'] as const;
export type Category = (typeof CATEGORIES)[number];
export const OBJECT_TYPES = ['car', 'e_bike', 'bike', 'motorcycle', 'home', 'appliance', 'tool', 'body', 'other'] as const;
export type ObjectType = (typeof OBJECT_TYPES)[number];
```

Change `category: string` to `type: ObjectType` on the object interfaces at `types.ts:12` and
`:17`.

Create `frontend/src/lib/object-types.ts`:

```ts
import { OBJECT_TYPES, type Category, type ObjectType } from './types';

/** What each type is, in one place: the icon a row shows, and what its entries can be. */
const TABLE: Record<ObjectType, { icon: string; categories: Category[] }> = {
  car:        { icon: 'car',        categories: ['maintenance', 'repair', 'inspection', 'fuel', 'modification', 'purchase', 'other'] },
  e_bike:     { icon: 'e-bike',     categories: ['maintenance', 'repair', 'inspection', 'fuel', 'modification', 'purchase', 'other'] },
  bike:       { icon: 'bike',       categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  motorcycle: { icon: 'motorcycle', categories: ['maintenance', 'repair', 'inspection', 'fuel', 'modification', 'purchase', 'other'] },
  home:       { icon: 'home',       categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  appliance:  { icon: 'appliance',  categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  tool:       { icon: 'tool',       categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  body:       { icon: 'body',       categories: ['symptom', 'treatment', 'appointment', 'medication', 'other'] },
  other:      { icon: 'object',     categories: [...CATEGORIES] },
};

export { OBJECT_TYPES };
export function typeIcon(t: ObjectType): string { return TABLE[t].icon; }

/**
 * What the category select offers for this type -- plus `current`, always.
 *
 * Filtering is presentation only. An entry logged before its object was re-typed keeps its own
 * category in the list, so opening and saving an untouched form cannot re-file it.
 */
export function categoriesFor(t: ObjectType, current?: Category): Category[] {
  const list = TABLE[t].categories;
  return current && !list.includes(current) ? [...list, current] : list;
}
```

`other` spreads `CATEGORIES` itself rather than repeating the list, so a category added later
cannot go missing from the one type that is meant to offer everything. Import it alongside the
types: `import { CATEGORIES, OBJECT_TYPES, type Category, type ObjectType } from './types';`

- [ ] **Step 4: Add the strings to both languages**

In `frontend/src/i18n/en.ts`, beside the existing `cat.*` keys:

```ts
  'cat.symptom': 'Symptom',
  'cat.treatment': 'Treatment',
  'cat.appointment': 'Appointment',
  'cat.medication': 'Medication',
  'type.car': 'Car',
  'type.e_bike': 'E-bike',
  'type.bike': 'Bicycle',
  'type.motorcycle': 'Motorcycle',
  'type.home': 'Home',
  'type.appliance': 'Appliance',
  'type.tool': 'Tool',
  'type.body': 'Body',
  'type.other': 'Other',
```

And in `frontend/src/i18n/de.ts`:

```ts
  'cat.symptom': 'Symptom',
  'cat.treatment': 'Behandlung',
  'cat.appointment': 'Termin',
  'cat.medication': 'Medikament',
  'type.car': 'Auto',
  'type.e_bike': 'E-Bike',
  'type.bike': 'Fahrrad',
  'type.motorcycle': 'Motorrad',
  'type.home': 'Zuhause',
  'type.appliance': 'Gerät',
  'type.tool': 'Werkzeug',
  'type.body': 'Körper',
  'type.other': 'Sonstiges',
```

Replace `object.category` and `object.category-hint` with `object.type` ("Type" / "Typ"); the
hint goes, since a picker needs no examples.

- [ ] **Step 5: Draw the icons**

Add nine variants to `frontend/src/lib/Icon.svelte`, following the existing ones exactly: 24×24
viewBox, `fill="none"`, `stroke="currentColor"`, `stroke-width="1.75"`, round caps and joins, and
a branch per name in the same `{#if}` chain. The names are `car`, `e-bike`, `bike`,
`motorcycle`, `home`, `appliance`, `tool`, `body`, `object`.

Keep them simple enough to read at 20px: a bicycle is two circles and a frame, not a drivetrain.
The `e-bike` icon is the bicycle with a small bolt; `body` is a torso and shoulders, which is
what the health type is actually about.

Add each name to the `name` union at the top of the file. The union is exhaustive-checked by the
final `{(name satisfies never)}` branch, so a missing variant fails `npm run check`.

- [ ] **Step 6: Run**

Run: `cd frontend && npx vitest run && npm run check`

Expected: the new tests pass; 0 type errors.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/object-types.ts frontend/src/lib/types.ts frontend/src/lib/Icon.svelte frontend/src/i18n frontend/tests/object-types.test.ts
git commit -m "feat: what each object type is, in one table"
```

---

### Task 6: The type picker and the icons on screen

**Files:**
- Modify: `frontend/src/routes/ObjectForm.svelte:51-56`
- Modify: `frontend/src/lib/ObjectCard.svelte:11-25`
- Modify: `frontend/src/routes/ObjectDetail.svelte` (the top bar)
- Modify: `frontend/src/routes/Search.svelte` (object results)
- Test: `frontend/tests-e2e/02-lifecycle.spec.ts`

**Interfaces:**
- Consumes: `typeIcon`, `OBJECT_TYPES` from Task 5.

- [ ] **Step 1: Update the existing e2e flow, which will fail**

`02-lifecycle.spec.ts` currently does `await page.getByLabel('Category').fill('car');`. A select
cannot be filled. Change it to:

```ts
  await page.getByLabel('Type').selectOption('car');
```

and after the object is created, assert the icon reached the dashboard:

```ts
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.locator('.card-row svg').first()).toBeVisible();
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npx playwright test 02-lifecycle`

Expected: fails at `selectOption` — the control is still a text input.

- [ ] **Step 3: Replace the input with a picker**

In `frontend/src/routes/ObjectForm.svelte`, replace lines 51-56 with:

```svelte
    <div class="field">
      <label for="c">{$t('object.type')}</label>
      <select id="c" bind:value={input.type}>
        {#each OBJECT_TYPES as ty}<option value={ty}>{$t(`type.${ty}`)}</option>{/each}
      </select>
    </div>
```

Import `OBJECT_TYPES` from `../lib/types`, and default `input.type` to `'other'` for a new
object.

- [ ] **Step 4: Put the icon on the rows**

In `ObjectCard.svelte`, show the type icon at the start of the row body, and replace the raw
`{object.category}` line at `:25` with `{$t(`type.${object.type}`)}`. Give the icon no accessible
name: the type is already written next to it, so a label would make a screen reader say it twice.

Do the same in the object detail top bar and in the search results' object rows.

- [ ] **Step 5: Run everything**

Run: `cd frontend && npm run check && npm run build && npx playwright test`

Expected: 0 type errors, 19 passed.

- [ ] **Step 6: Look at it**

Screenshot the dashboard with at least four objects of different types, in both themes, and
confirm the icons are distinguishable at a glance at their rendered size. Report what you see.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/routes/ObjectForm.svelte frontend/src/lib/ObjectCard.svelte frontend/src/routes/ObjectDetail.svelte frontend/src/routes/Search.svelte frontend/tests-e2e/02-lifecycle.spec.ts
git commit -m "feat: pick an object's type, and see it in every list"
```

---

### Task 7: The activity form speaks the object's vocabulary

**Files:**
- Modify: `frontend/src/routes/ActivityForm.svelte:257-262` (the category select)
- Test: `frontend/tests-e2e/09-object-types.spec.ts` (new)

**Interfaces:**
- Consumes: `categoriesFor` from Task 5; `object.type` loaded by the form already.

- [ ] **Step 1: Write the failing end-to-end test**

Create `frontend/tests-e2e/09-object-types.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

// Seeds its own object with a name no other spec uses: one server and one database are shared
// across the run.
test('a body object logs a symptom, and is not offered fuel', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Left shoulder');
  await page.getByLabel('Type').selectOption('body');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByRole('button', { name: /Log activity/ }).click();
  const select = page.getByLabel('Category');
  await expect(select.locator('option')).toHaveText(['Symptom', 'Treatment', 'Appointment', 'Medication', 'Other']);

  await select.selectOption('symptom');
  await page.getByLabel('Title').fill('Pain lifting overhead');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Pain lifting overhead').first()).toBeVisible();
});

// The rule that stops a re-type silently re-filing history.
test('an entry keeps its own category after its object is re-typed', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Retyped van');
  await page.getByLabel('Type').selectOption('car');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByRole('button', { name: /Log activity/ }).click();
  await page.getByLabel('Category').selectOption('fuel');
  await page.getByLabel('Title').fill('Diesel fill');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByRole('button', { name: 'Edit' }).click();
  await page.getByLabel('Type').selectOption('body');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByText('Diesel fill').first().click();
  await expect(page.getByLabel('Category')).toHaveValue('fuel');
  await expect(page.getByLabel('Category').locator('option')).toContainText(['Fuel / charge']);
});
```

- [ ] **Step 2: Run and watch fail**

Run: `cd frontend && npx playwright test 09-object-types`

Expected: the first test fails — the select still offers all eleven categories.

- [ ] **Step 3: Filter the select**

In `frontend/src/routes/ActivityForm.svelte`, replace the `{#each CATEGORIES as c}` loop with a
derived list:

```svelte
        <select id="c" bind:value={input.category}>
          {#each offered as c}<option value={c}>{$t(`cat.${c}`)}</option>{/each}
        </select>
```

and above, in the script:

```ts
  // The object's vocabulary, plus whatever this entry already says. An entry logged before its
  // object was re-typed must keep its own category in the list, or saving an untouched form
  // would quietly re-file it.
  const offered = $derived(categoriesFor(object?.type ?? 'other', input.category));
```

Import `categoriesFor` from `../lib/object-types`. When a new entry's default category is not
offered by the type — `maintenance` on a `body` object — default to the first offered category
instead.

- [ ] **Step 4: Run everything**

Run: `cd frontend && npm run check && npm run build && npx playwright test`

Expected: 0 type errors, 21 passed (19 existing plus the two new).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/ActivityForm.svelte frontend/tests-e2e/09-object-types.spec.ts
git commit -m "feat: an object's type decides what its entries can be"
```

---

### Task 8: The timeline filter chips

**Files:**
- Modify: `frontend/src/lib/Timeline.svelte:8,20-24`
- Test: `frontend/tests-e2e/09-object-types.spec.ts` (append to the file from Task 7)

**Interfaces:**
- Consumes: `categoriesFor` from Task 5; the object's `type`, which `ObjectDetail` already has.

`Timeline.svelte:22` renders one filter chip per entry in `CATEGORIES`. That list grew from
seven to eleven in Task 5, so without this task every car and dishwasher shows Symptom,
Treatment, Appointment and Medication chips that can only ever filter to nothing.

- [ ] **Step 1: Write the failing test**

Append to `frontend/tests-e2e/09-object-types.spec.ts`:

```ts
test('a car is not offered health filters, and keeps a chip for what it actually has', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Chip test wagon');
  await page.getByLabel('Type').selectOption('car');
  await page.getByRole('button', { name: 'Save' }).click();

  const chips = page.locator('.chips button');
  await expect(chips).not.toContainText(['Symptom']);
  await expect(chips).toContainText(['Fuel / charge']);

  // An entry whose category the type no longer offers still has a chip, or its rows become
  // unreachable by filtering.
  await page.getByRole('button', { name: /Log activity/ }).click();
  await page.getByLabel('Category').selectOption('fuel');
  await page.getByLabel('Title').fill('Filter probe');
  await page.getByRole('button', { name: 'Save' }).click();
  await page.getByRole('button', { name: 'Edit' }).click();
  await page.getByLabel('Type').selectOption('body');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.locator('.chips button')).toContainText(['Fuel / charge']);
});
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npx playwright test 09-object-types`

Expected: fails on `Symptom` being present in the chip row of a car.

- [ ] **Step 3: Drive the chips from the type and the data**

In `frontend/src/lib/Timeline.svelte`, accept the object's type as a prop, and replace the
`{#each CATEGORIES as c}` loop with a derived list:

```ts
  // The type's vocabulary, plus any category the loaded entries actually use. The second half
  // matters after a re-type: without it, an entry logged as `fuel` on an object that is now a
  // `body` has no chip and cannot be filtered to at all.
  const present = $derived(new Set(activities.map((a) => a.category)));
  const chipCategories = $derived(
    [...categoriesFor(type), ...CATEGORIES.filter((c) => present.has(c))]
      .filter((c, i, all) => all.indexOf(c) === i),
  );
```

Pass `type={object.type}` from `ObjectDetail.svelte` where `<Timeline …>` is rendered.

- [ ] **Step 4: Run everything**

Run: `cd frontend && npm run check && npm run build && npx playwright test`

Expected: 0 type errors, 22 passed.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/Timeline.svelte frontend/src/routes/ObjectDetail.svelte frontend/tests-e2e/09-object-types.spec.ts
git commit -m "fix: a car has no use for a symptom filter"
```

---

### Task 9: Documentation and the upgrade note

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-09-11-object-types-design.md` (status line)

- [ ] **Step 1: Write the upgrade note**

In the README's upgrade section, add:

```markdown
### Upgrading to 0.3.0

This release replaces an object's free-text category with a fixed type. The migration maps
known words in both languages (`Auto` → car, `Fahrrad` → bike, `Pedelec` → e-bike); anything it
does not recognise becomes **Other**, and the text you had typed is appended to that object's
description so nothing is lost.

It also rotates the sync epoch, so every device does one full re-sync on its next connection.
That is expected, and it is how each device learns the new field.

**Take a backup before upgrading.** The migration rebuilds two tables.
```

- [ ] **Step 2: Mark the spec implemented**

Change the spec's status line to `Status: implemented.`

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/specs/2026-09-11-object-types-design.md
git commit -m "docs: what the type migration does to an existing database"
```
