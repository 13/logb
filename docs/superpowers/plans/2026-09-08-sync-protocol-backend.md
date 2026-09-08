# Sync Protocol Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the server a sync protocol — a per-user change log, field-level last-write-wins conflict resolution, soft deletes, and three endpoints — so an offline client can push and pull edits.

**Architecture:** Every syncable row gains a client-minted `client_uuid` that ops address it by, and a `deleted_at` tombstone. A `changes` table is an append-only log with an autoincrement `seq` that clients use as a pull cursor; a `field_clock` table holds the winning `edited_at` per (entity, uuid, field) so an arriving op can be compared field by field. HTTP handlers stay thin — all decision logic lives in `src/sync/` and is unit-testable without a server.

**Tech Stack:** Rust, axum 0.8, sqlx 0.9 (SQLite), tokio. Tests are `#[tokio::test]` integration tests driving real HTTP via reqwest, following `tests/common/mod.rs`.

## Global Constraints

- This phase is **server-only**. No file under `frontend/` changes.
- Retention for tombstones and `changes` rows is **90 days**, expressed as a parameter on the purge function so tests can drive it, with the constant applied by the caller in `src/tasks.rs`. This mirrors how `notify::tick(&state, hour)` is already made testable.
- `cargo clippy --all-targets --locked -- -D warnings` must pass; CI treats warnings as errors.
- `tests/openapi.rs` compares routes declared in `src/api/**` against `docs/openapi.json` **in both directions**. Any task that adds a route MUST update `docs/openapi.json` in the same task or the suite fails.
- A losing op is **accepted, not rejected** — recorded, and reported as `superseded`. Only malformed or unauthorized ops are rejected.
- Ties in last-write-wins break on `device_id` string comparison, greater wins, so every participant resolves identically.
- All timestamps are RFC3339 UTC strings produced by `db::now()`, matching every existing table.
- Ops are scoped to the authenticated user. A user may never read or write another user's rows through sync.

---

### Task 1: Schema — client_uuid, tombstones, and the log tables

**Files:**
- Create: `migrations/0007_sync.sql`
- Modify: `Cargo.toml` (add `uuid`)
- Modify: `src/api/objects.rs` (insert path), `src/api/activities.rs` (insert path), `src/api/reminders.rs` (insert path), `src/api/attachments.rs` (insert paths for attachments and files)
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: `client_uuid TEXT NOT NULL UNIQUE` and `deleted_at TEXT` on `objects`, `activities`, `reminders`, `attachments`, `files`. Tables `changes` and `field_clock` as declared below. Every existing create handler mints a UUID via `uuid::Uuid::new_v4().to_string()`.

- [ ] **Step 1: Add the uuid dependency**

In `Cargo.toml`, under `[dependencies]`, after the `hex = "0.4"` line:

```toml
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 2: Write the migration**

Create `migrations/0007_sync.sql`:

```sql
-- Identity a client can mint before the server has seen the row, so a create made offline can
-- be referenced by the activities and attachments created alongside it in the same session.
-- Backfilled for existing rows with SQLite's own randomness: the format only has to be unique
-- and stable, and these rows predate any client that could have named them.
ALTER TABLE objects     ADD COLUMN client_uuid TEXT;
ALTER TABLE activities  ADD COLUMN client_uuid TEXT;
ALTER TABLE reminders   ADD COLUMN client_uuid TEXT;
ALTER TABLE attachments ADD COLUMN client_uuid TEXT;
ALTER TABLE files       ADD COLUMN client_uuid TEXT;

UPDATE objects     SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE activities  SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE reminders   SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE attachments SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE files       SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;

CREATE UNIQUE INDEX idx_objects_uuid     ON objects(client_uuid);
CREATE UNIQUE INDEX idx_activities_uuid  ON activities(client_uuid);
CREATE UNIQUE INDEX idx_reminders_uuid   ON reminders(client_uuid);
CREATE UNIQUE INDEX idx_attachments_uuid ON attachments(client_uuid);
CREATE UNIQUE INDEX idx_files_uuid       ON files(client_uuid);

-- Tombstones. A hard delete is invisible to a client that was offline when it happened, so
-- deletes are recorded instead of applied. `tasks.rs` purges these past the retention window.
ALTER TABLE objects     ADD COLUMN deleted_at TEXT;
ALTER TABLE activities  ADD COLUMN deleted_at TEXT;
ALTER TABLE reminders   ADD COLUMN deleted_at TEXT;
ALTER TABLE attachments ADD COLUMN deleted_at TEXT;
ALTER TABLE files       ADD COLUMN deleted_at TEXT;

-- The append-only log. `seq` is the pull cursor: monotonic, gapless per database, and ordered
-- by the order the server accepted work rather than by any device's clock.
CREATE TABLE changes (
    seq          INTEGER PRIMARY KEY AUTOINCREMENT,
    entity       TEXT NOT NULL CHECK (entity IN ('object','activity','reminder','attachment','file')),
    entity_uuid  TEXT NOT NULL,
    op           TEXT NOT NULL CHECK (op IN ('create','set','delete')),
    field        TEXT,
    value        TEXT,
    edited_at    TEXT NOT NULL,
    applied_at   TEXT NOT NULL,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id    TEXT NOT NULL,
    client_op_id TEXT NOT NULL
);
CREATE INDEX idx_changes_user_seq ON changes(user_id, seq);
-- Scoped to the user, not global. A globally unique client_op_id lets one account's op id
-- collide with another's: the idempotency lookup finds the stranger's row, reports the op
-- accepted, and never applies it -- a lost write reported as success.
CREATE UNIQUE INDEX idx_changes_user_op ON changes(user_id, client_op_id);

-- The winning edit time per field, which is what an arriving op is compared against. Separate
-- from `changes` because that table holds losers too, and a scan of it per field would grow
-- with history rather than with the data.
CREATE TABLE field_clock (
    entity      TEXT NOT NULL,
    entity_uuid TEXT NOT NULL,
    field       TEXT NOT NULL,
    edited_at   TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    PRIMARY KEY (entity, entity_uuid, field)
);
```

- [ ] **Step 3: Write the failing test**

Create `tests/sync.rs`:

```rust
mod common;

#[tokio::test]
async fn every_created_row_gets_a_client_uuid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(uuid.len(), 36, "a v4 uuid in hyphenated form: {uuid}");

    let deleted: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(deleted.is_none(), "a fresh row is not a tombstone");
}

#[tokio::test]
async fn the_log_tables_exist_and_start_empty() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let changes: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    let clocks: i64 = sqlx::query_scalar("SELECT count(*) FROM field_clock")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!((changes, clocks), (0, 0));
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test --test sync`
Expected: FAIL — `every_created_row_gets_a_client_uuid` panics because `client_uuid` is `NULL` for a row the handler inserted without one, so `fetch_one` into `String` errors on an unexpected null.

- [ ] **Step 5: Mint a UUID in every create path**

In `src/api/objects.rs`, the `INSERT INTO objects (...)` statement at the create handler: add `client_uuid` to the column list and one more `?` to the values list, then bind `uuid::Uuid::new_v4().to_string()` in the matching position.

Apply the identical change to the `INSERT INTO activities` in `src/api/activities.rs`, the `INSERT INTO reminders` in `src/api/reminders.rs`, and both the `INSERT INTO attachments` and `INSERT INTO files` statements in `src/api/attachments.rs`.

Also extend the import path in `src/api/export.rs` if it inserts rows directly — grep for `INSERT INTO` across `src/` and make sure every statement targeting these five tables supplies a `client_uuid`:

```bash
grep -rn "INSERT INTO \(objects\|activities\|reminders\|attachments\|files\)" src/
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test`
Expected: PASS — the whole suite, not just `--test sync`. Every existing test creates rows through these same handlers, so this step proves the column addition broke nothing.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock migrations/0007_sync.sql src/api tests/sync.rs
git commit -m "feat: give syncable rows a client uuid and a tombstone column"
```

---

### Task 2: Soft deletes

**Files:**
- Modify: `src/api/objects.rs` (delete handler, list and read queries), `src/api/activities.rs`, `src/api/reminders.rs`, `src/api/attachments.rs`
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: `deleted_at` columns from Task 1.
- Produces: `DELETE /api/objects/{id}` and its siblings set `deleted_at` instead of removing the row; every read path filters `deleted_at IS NULL`. Deleting an object cascades in application code to its activities, reminders and attachments.

Rationale for the reviewer: SQLite's `ON DELETE CASCADE` cannot fire for an `UPDATE`, so the cascade the schema used to provide has to be written out. Without it, deleting an object would leave its activities alive and syncing.

- [ ] **Step 1: Write the failing test**

Append to `tests/sync.rs`:

```rust
#[tokio::test]
async fn deleting_an_object_tombstones_it_and_its_children() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();

    let res = app.client.post(app.url("/activities")).json(&serde_json::json!({
        "object_id": object_id, "date": "2026-01-01", "category": "fuel",
        "title": "Fill-up", "notes": "", "counter_value": 1000, "cost_cents": 5000
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "create activity: {}", res.text().await.unwrap());

    assert_eq!(
        app.client.delete(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(),
        204
    );

    let object_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(object_rows, 1, "the row survives; only deleted_at is set");

    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM objects WHERE id = ? AND deleted_at IS NULL")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live, 0, "the object is tombstoned");

    let live_children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activities WHERE object_id = ? AND deleted_at IS NULL")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live_children, 0, "children are tombstoned with the parent");

    assert_eq!(
        app.client.get(app.url(&format!("/objects/{object_id}"))).send().await.unwrap().status(),
        404,
        "a tombstoned object reads as absent"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test sync deleting_an_object_tombstones`
Expected: FAIL on `assert_eq!(object_rows, 1)` — the current handler hard-deletes, so the count is 0.

- [ ] **Step 3: Convert the delete handlers**

In `src/api/objects.rs`, replace the `DELETE FROM objects WHERE id = ? AND user_id = ?` statement in the delete handler with a tombstone plus an application-level cascade, run in one transaction so a partial cascade cannot survive a failure:

```rust
let now = db::now();
let mut tx = state.db.begin().await?;
let affected = sqlx::query(
    "UPDATE objects SET deleted_at = ?, updated_at = ? \
     WHERE id = ? AND user_id = ? AND deleted_at IS NULL")
    .bind(&now).bind(&now).bind(id).bind(user.id)
    .execute(&mut *tx).await?.rows_affected();
if affected == 0 {
    return Err(AppError::NotFound);
}
sqlx::query("UPDATE activities SET deleted_at = ? WHERE object_id = ? AND deleted_at IS NULL")
    .bind(&now).bind(id).execute(&mut *tx).await?;
sqlx::query("UPDATE reminders SET deleted_at = ? WHERE object_id = ? AND deleted_at IS NULL")
    .bind(&now).bind(id).execute(&mut *tx).await?;
sqlx::query("UPDATE attachments SET deleted_at = ? WHERE object_id = ? AND deleted_at IS NULL")
    .bind(&now).bind(id).execute(&mut *tx).await?;
tx.commit().await?;
Ok(StatusCode::NO_CONTENT)
```

Apply the same tombstone-instead-of-delete change to the delete handlers in `src/api/activities.rs` (also tombstoning that activity's attachments), `src/api/reminders.rs`, and `src/api/attachments.rs`.

- [ ] **Step 4: Filter every read path**

Add `AND deleted_at IS NULL` (or `AND o.deleted_at IS NULL` where the query aliases the table) to every `SELECT` and `UPDATE ... WHERE` targeting these five tables. Find them with:

```bash
grep -rn "FROM \(objects\|activities\|reminders\|attachments\|files\)" src/
```

Include the aggregate subqueries in `src/api/objects.rs` that compute `ObjectStats`, the timeline query in `src/api/activities.rs`, the search query in `src/api/search.rs`, the digest collector in `src/notify.rs`, and the export walk in `src/api/export.rs` — a tombstoned row must not appear in stats, search results, reminder digests, or an export.

- [ ] **Step 5: Run the whole suite**

Run: `cargo test`
Expected: PASS. The existing `tests/objects.rs::crud_and_stats` already asserts a deleted object reads as 404, and `tests/users.rs::deleting_a_user_takes_their_objects_files_and_blobs` covers the user-deletion path — both must still pass. If the user-deletion test fails, note that deleting a *user* remains a hard delete via `ON DELETE CASCADE`; only the per-row endpoints become soft.

- [ ] **Step 6: Commit**

```bash
git add src tests/sync.rs
git commit -m "feat: turn row deletes into tombstones so an offline client can learn of them"
```

---

### Task 3: Sync types and the syncable-field whitelist

**Files:**
- Create: `src/sync/mod.rs`
- Modify: `src/lib.rs` (add `pub mod sync;`)
- Test: unit tests inside `src/sync/mod.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks at the type level.
- Produces:
  - `pub enum Entity { Object, Activity, Reminder, Attachment, File }` with `Entity::from_str(&str) -> Option<Entity>`, `Entity::as_str(&self) -> &'static str`, `Entity::table(&self) -> &'static str`.
  - `pub fn is_syncable_field(entity: Entity, field: &str) -> bool`.
  - `pub struct Op { pub client_op_id: String, pub entity: Entity, pub entity_uuid: String, pub op: OpKind, pub field: Option<String>, pub value: Option<serde_json::Value>, pub edited_at: String, pub device_id: String }`
  - `pub enum OpKind { Create, Set, Delete }` with the same `from_str`/`as_str` pair.
  - `pub enum Outcome { Accepted, Superseded, Rejected(String) }`, serialized in lowercase with a `reason` on `Rejected`.

The whitelist is the security boundary: it is what stops a `set` op naming `user_id` or `id` and reparenting a row into another account.

- [ ] **Step 1: Write the failing test**

Create `src/sync/mod.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_round_trip() {
        assert_eq!(Entity::from_str("activity"), Some(Entity::Activity));
        assert_eq!(Entity::Activity.as_str(), "activity");
        assert_eq!(Entity::Activity.table(), "activities");
        assert_eq!(Entity::from_str("users"), None);
    }

    #[test]
    fn the_whitelist_admits_real_columns() {
        assert!(is_syncable_field(Entity::Object, "name"));
        assert!(is_syncable_field(Entity::Activity, "quantity_milli"));
        assert!(is_syncable_field(Entity::Reminder, "snoozed_until"));
        assert!(is_syncable_field(Entity::Attachment, "caption"));
    }

    #[test]
    fn the_whitelist_refuses_identity_and_ownership() {
        for field in ["id", "user_id", "object_id", "client_uuid", "created_at", "deleted_at"] {
            assert!(!is_syncable_field(Entity::Object, field), "{field} must not be settable");
            assert!(!is_syncable_field(Entity::Activity, field), "{field} must not be settable");
        }
    }

    #[test]
    fn files_are_immutable() {
        for field in ["sha256", "mime", "size", "original_name"] {
            assert!(!is_syncable_field(Entity::File, field), "files are create/delete only");
        }
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib sync`
Expected: FAIL to compile — `Entity` and `is_syncable_field` are not defined.

- [ ] **Step 3: Write the implementation**

Above the test module in `src/sync/mod.rs`:

```rust
//! The sync protocol's vocabulary: what an op can name, and what it may change.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Entity { Object, Activity, Reminder, Attachment, File }

impl Entity {
    pub fn from_str(s: &str) -> Option<Entity> {
        match s {
            "object" => Some(Entity::Object),
            "activity" => Some(Entity::Activity),
            "reminder" => Some(Entity::Reminder),
            "attachment" => Some(Entity::Attachment),
            "file" => Some(Entity::File),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Entity::Object => "object",
            Entity::Activity => "activity",
            Entity::Reminder => "reminder",
            Entity::Attachment => "attachment",
            Entity::File => "file",
        }
    }

    /// The table the entity lives in. Kept here rather than interpolated at each call site so
    /// that the only strings ever spliced into SQL come from this closed set.
    pub fn table(&self) -> &'static str {
        match self {
            Entity::Object => "objects",
            Entity::Activity => "activities",
            Entity::Reminder => "reminders",
            Entity::Attachment => "attachments",
            Entity::File => "files",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind { Create, Set, Delete }

impl OpKind {
    pub fn from_str(s: &str) -> Option<OpKind> {
        match s {
            "create" => Some(OpKind::Create),
            "set" => Some(OpKind::Set),
            "delete" => Some(OpKind::Delete),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            OpKind::Create => "create",
            OpKind::Set => "set",
            OpKind::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Op {
    pub client_op_id: String,
    pub entity: Entity,
    pub entity_uuid: String,
    pub op: OpKind,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    pub edited_at: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum Outcome {
    Accepted,
    Superseded,
    Rejected { reason: String },
}

/// Whether an op may write `field` on `entity`.
///
/// This is the protocol's security boundary, and the reason it is a whitelist rather than a
/// blacklist: `id`, `user_id` and the parent foreign keys are all ordinary columns, so a
/// blacklist that forgot one would let a `set` op move somebody else's row into the caller's
/// account. Anything absent here is simply not settable over sync.
pub fn is_syncable_field(entity: Entity, field: &str) -> bool {
    let allowed: &[&str] = match entity {
        Entity::Object => &[
            "name", "category", "counter_unit", "fuel_unit", "description",
            "purchase_date", "purchase_price_cents", "archived_at", "cover_attachment_id",
        ],
        Entity::Activity => &[
            "date", "category", "title", "notes", "counter_value", "cost_cents",
            "quantity_milli",
        ],
        Entity::Reminder => &[
            "title", "notes", "due_date", "due_counter", "repeat_months", "repeat_counter",
            "done_at", "done_activity_id", "snoozed_until",
        ],
        Entity::Attachment => &["kind", "caption"],
        // Content-addressed and written once. A file changes by being replaced, never edited.
        Entity::File => &[],
    };
    allowed.contains(&field)
}
```

In `src/lib.rs`, add `pub mod sync;` to the module list, keeping alphabetical order (after `pub mod state;`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib sync`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```bash
git add src/sync/mod.rs src/lib.rs
git commit -m "feat: define the sync vocabulary and the settable-field whitelist"
```

---

### Task 4: Last-write-wins resolution

**Files:**
- Create: `src/sync/apply.rs`
- Modify: `src/sync/mod.rs` (add `pub mod apply;`)
- Test: unit tests inside `src/sync/apply.rs`

**Interfaces:**
- Consumes: `Entity`, `OpKind`, `Op`, `Outcome`, `is_syncable_field` from Task 3.
- Produces: `pub fn wins(incoming_edited_at: &str, incoming_device: &str, stored_edited_at: &str, stored_device: &str) -> bool`.

Isolated from the database on purpose: this is the one rule the whole protocol turns on, and it is worth being able to test exhaustively without a pool.

- [ ] **Step 1: Write the failing test**

Create `src/sync/apply.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_edit_wins() {
        assert!(wins("2026-01-02T00:00:00Z", "phone", "2026-01-01T00:00:00Z", "desktop"));
    }

    #[test]
    fn an_older_edit_loses() {
        assert!(!wins("2026-01-01T00:00:00Z", "phone", "2026-01-02T00:00:00Z", "desktop"));
    }

    #[test]
    fn a_tie_breaks_on_device_id_with_the_greater_winning() {
        let t = "2026-01-01T00:00:00Z";
        assert!(wins(t, "phone", t, "desktop"), "phone > desktop");
        assert!(!wins(t, "desktop", t, "phone"), "desktop < phone");
    }

    #[test]
    fn a_replay_of_the_same_op_does_not_win() {
        let t = "2026-01-01T00:00:00Z";
        assert!(!wins(t, "phone", t, "phone"), "identical edit is not newer than itself");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib sync::apply`
Expected: FAIL to compile — `wins` is not defined.

- [ ] **Step 3: Write the implementation**

Above the test module in `src/sync/apply.rs`:

```rust
//! Applying an incoming op, and the rule that decides whether it may.

/// Whether an incoming edit supersedes the stored one for a field.
///
/// Timestamps are RFC3339 UTC with a fixed number of digits, so a lexical comparison is also
/// a chronological one and no parsing is needed. Equal timestamps break on `device_id`, which
/// is arbitrary but *consistent*: every device applying the same pair of ops reaches the same
/// answer without talking to any other device, which is what keeps two phones from converging
/// on different values. An op identical to the stored one does not win, so a replay is a
/// no-op rather than a rewrite.
pub fn wins(
    incoming_edited_at: &str,
    incoming_device: &str,
    stored_edited_at: &str,
    stored_device: &str,
) -> bool {
    match incoming_edited_at.cmp(stored_edited_at) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => incoming_device > stored_device,
    }
}
```

In `src/sync/mod.rs`, add at the top: `pub mod apply;`

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib sync::apply`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```bash
git add src/sync
git commit -m "feat: add the last-write-wins rule with a deterministic tie-break"
```

---

### Task 5: POST /api/sync/push

**Files:**
- Create: `src/api/sync.rs`
- Modify: `src/api/mod.rs` (declare and merge the router), `src/sync/apply.rs` (add `apply_op`)
- Modify: `docs/openapi.json`
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: `wins` (Task 4), `Entity`/`OpKind`/`Op`/`Outcome`/`is_syncable_field` (Task 3), `AuthUser` extractor from `src/auth.rs`, `db::now()`.
- Produces:
  - `pub async fn apply_op(tx: &mut sqlx::SqliteConnection, user_id: i64, op: &crate::sync::Op) -> Result<crate::sync::Outcome, crate::error::AppError>` in `src/sync/apply.rs`.
  - `pub fn canonical_edited_at(raw: &str) -> Option<String>` in `src/sync/apply.rs`.
  - `POST /api/sync/push` taking `{ "ops": [Op, ...] }` and returning `{ "results": [{ "client_op_id": String, "outcome": "accepted"|"superseded"|"rejected", "reason"?: String }], "server_time": String, "ids": { "<uuid>": <i64> } }`.

- [ ] **Step 1: Write the failing test**

Append to `tests/sync.rs`:

```rust
use serde_json::json;

/// An op batch as the wire format expects it.
fn push_body(ops: serde_json::Value) -> serde_json::Value {
    json!({ "ops": ops })
}

#[tokio::test]
async fn a_set_op_updates_the_row_and_is_logged() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-1", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Golf VII",
        "edited_at": "2026-02-01T10:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "accepted");
    assert!(body["server_time"].is_string());

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Golf VII");

    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-1'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1);
}

#[tokio::test]
async fn an_older_edit_is_superseded_but_still_recorded() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let newer = json!([{
        "client_op_id": "op-new", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Newer",
        "edited_at": "2026-02-02T00:00:00Z", "device_id": "phone"
    }]);
    let older = json!([{
        "client_op_id": "op-old", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Older",
        "edited_at": "2026-02-01T00:00:00Z", "device_id": "phone"
    }]);
    app.client.post(app.url("/sync/push")).json(&push_body(newer)).send().await.unwrap();
    let res = app.client.post(app.url("/sync/push")).json(&push_body(older)).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();

    assert_eq!(body["results"][0]["outcome"], "superseded");
    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Newer", "the loser must not overwrite the winner");
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-old'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "a superseded op is still part of the log");
}

#[tokio::test]
async fn a_replayed_push_is_idempotent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let batch = push_body(json!([{
        "client_op_id": "op-same", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Once",
        "edited_at": "2026-02-01T00:00:00Z", "device_id": "phone"
    }]));
    for _ in 0..2 {
        let res = app.client.post(app.url("/sync/push")).json(&batch).send().await.unwrap();
        assert_eq!(res.status(), 200);
    }
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM changes WHERE client_op_id = 'op-same'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "the same op id lands exactly once");
}

#[tokio::test]
async fn timestamps_are_compared_chronologically_not_lexically() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    // Whole seconds first, then a value half a second LATER written with a fraction. Compared
    // as raw strings the fractional one loses ('.' < 'Z'), so a lexical rule would keep "Early".
    for (id, value, at) in [
        ("op-whole", "Early", "2026-08-01T00:00:00Z"),
        ("op-frac", "Later", "2026-08-01T00:00:00.500Z"),
    ] {
        let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": id, "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": value,
            "edited_at": at, "device_id": "phone"
        }]))).send().await.unwrap();
        assert_eq!(res.status(), 200, "push failed: {}", res.text().await.unwrap());
    }

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Later", "the chronologically later edit must win");

    // An offset-form timestamp is the same instant as its Z form and must not re-win.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-offset", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Earlier still",
        "edited_at": "2026-08-01T00:00:00+00:00", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "superseded");

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-junk", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Nonsense",
        "edited_at": "last thursday", "device_id": "phone"
    }]))).send().await.unwrap();
    assert_eq!(res.json::<serde_json::Value>().await.unwrap()["results"][0]["outcome"], "rejected");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Later");
}

#[tokio::test]
async fn a_field_outside_the_whitelist_is_rejected() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-evil", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "user_id", "value": 2,
        "edited_at": "2026-02-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");
}

#[tokio::test]
async fn a_foreign_key_field_cannot_point_at_another_users_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let victim_object = car["id"].as_i64().unwrap();
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"%PDF-1.4 fake".to_vec())
            .file_name("invoice.pdf")
            .mime_str("application/pdf")
            .unwrap(),
    );
    let res = app.client.post(app.url(&format!("/objects/{victim_object}/attachments")))
        .multipart(form).send().await.unwrap();
    assert_eq!(res.status(), 201, "upload failed: {}", res.text().await.unwrap());
    let victim_attachment = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    // A second account, with an object of its own, tries to adopt the first account's
    // attachment as its cover image.
    let mallory = app.create_user_client("mallory", "another password").await;
    let theirs = app.create_object(&mallory, "Bike", None).await;
    let their_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(theirs["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let res = mallory.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-steal", "entity": "object", "entity_uuid": their_uuid,
        "op": "set", "field": "cover_attachment_id", "value": victim_attachment,
        "edited_at": "2026-07-01T00:00:00Z", "device_id": "mallory-phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");

    let cover: Option<i64> = sqlx::query_scalar(
        "SELECT cover_attachment_id FROM objects WHERE client_uuid = ?")
        .bind(&their_uuid).fetch_one(&app.state.db).await.unwrap();
    assert!(cover.is_none(), "the cross-account reference must not have landed");
}

#[tokio::test]
async fn one_user_cannot_push_at_another_users_row() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let res = other.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-cross", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Stolen",
        "edited_at": "2030-01-01T00:00:00Z", "device_id": "mallory-phone"
    }]))).send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected");

    let name: String = sqlx::query_scalar("SELECT name FROM objects WHERE client_uuid = ?")
        .bind(&uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(name, "Golf", "an unrelated user changed nothing");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test sync`
Expected: FAIL — every push test gets 404, because `/sync/push` is not routed.

- [ ] **Step 3: Write `apply_op`**

Append to `src/sync/apply.rs`, above the test module:

```rust
use crate::error::AppError;
use crate::sync::{is_syncable_field, Entity, Op, OpKind, Outcome};

/// Rewrites a client-supplied timestamp into the one canonical form `wins` can compare.
///
/// `wins` compares `edited_at` lexically, which is only chronological when every value has the
/// same width and the same zone spelling. `db::now()` guarantees that for values the server
/// writes, but `edited_at` arrives from a device and nothing constrains what it sends:
/// `2026-01-01T00:00:00Z` sorts AFTER `2026-01-01T00:00:00.500Z` (`Z` is 0x5A, `.` is 0x2E)
/// while being half a second earlier, and an offset form like `+00:00` does not order against
/// `Z` at all. Either would hand the wrong edit the win, silently and unreproducibly. So every
/// timestamp is parsed and re-emitted as UTC with fixed millisecond precision before it is
/// compared with, or stored beside, any other.
pub fn canonical_edited_at(raw: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Applies one op inside the caller's transaction and returns how it landed.
///
/// Ownership is resolved by joining back to `objects.user_id` rather than trusting anything in
/// the op, so a uuid belonging to another account cannot be written through.
pub async fn apply_op(
    tx: &mut sqlx::SqliteConnection,
    user_id: i64,
    op: &Op,
) -> Result<Outcome, AppError> {
    if op.entity_uuid.is_empty() || op.device_id.is_empty() {
        return Ok(Outcome::Rejected { reason: "entity_uuid and device_id are required".into() });
    }

    // Does this uuid exist, and does it belong to the caller?
    let owner: Option<i64> = match op.entity {
        Entity::Object => sqlx::query_scalar(
            "SELECT user_id FROM objects WHERE client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Activity => sqlx::query_scalar(
            "SELECT o.user_id FROM activities a JOIN objects o ON o.id = a.object_id \
             WHERE a.client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Reminder => sqlx::query_scalar(
            "SELECT o.user_id FROM reminders r JOIN objects o ON o.id = r.object_id \
             WHERE r.client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::Attachment => sqlx::query_scalar(
            "SELECT o.user_id FROM attachments t JOIN objects o ON o.id = t.object_id \
             WHERE t.client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
        Entity::File => sqlx::query_scalar(
            "SELECT user_id FROM files WHERE client_uuid = ?")
            .bind(&op.entity_uuid).fetch_optional(&mut *tx).await?,
    };
    match owner {
        None => return Ok(Outcome::Rejected { reason: "unknown entity_uuid".into() }),
        Some(owner) if owner != user_id => {
            return Ok(Outcome::Rejected { reason: "unknown entity_uuid".into() })
        }
        Some(_) => {}
    }

    match op.op {
        // A create arriving through sync is a client announcing a row it already made; the row
        // itself is inserted by the ordinary REST create, which the client still calls. Here it
        // only has to be logged, so the pull feed carries it to other devices.
        OpKind::Create => Ok(Outcome::Accepted),

        OpKind::Delete => {
            let sql = format!(
                "UPDATE {} SET deleted_at = ? WHERE client_uuid = ? AND deleted_at IS NULL",
                op.entity.table()
            );
            sqlx::query(&sql).bind(crate::db::now()).bind(&op.entity_uuid)
                .execute(&mut *tx).await?;
            Ok(Outcome::Accepted)
        }

        OpKind::Set => {
            let Some(field) = op.field.as_deref() else {
                return Ok(Outcome::Rejected { reason: "set requires a field".into() });
            };
            if !is_syncable_field(op.entity, field) {
                return Ok(Outcome::Rejected { reason: format!("{field} is not settable") });
            }

            // Two whitelisted fields are foreign keys, and a check on the field NAME says
            // nothing about the VALUE. Without this, `set object.cover_attachment_id` could
            // point a row at another account's attachment -- which the object read then hands
            // back as `cover_file_id`. The REST handlers already refuse a cross-object
            // reference; sync has to refuse it identically, or it is just a second, unguarded
            // door onto the same write.
            if let Some(referenced) = op.value.as_ref().and_then(serde_json::Value::as_i64) {
                let permitted: Option<i64> = match (op.entity, field) {
                    (Entity::Object, "cover_attachment_id") => sqlx::query_scalar(
                        "SELECT a.id FROM attachments a JOIN objects o ON o.id = a.object_id \
                         WHERE a.id = ? AND o.client_uuid = ? AND a.deleted_at IS NULL")
                        .bind(referenced).bind(&op.entity_uuid)
                        .fetch_optional(&mut *tx).await?,
                    (Entity::Reminder, "done_activity_id") => sqlx::query_scalar(
                        "SELECT act.id FROM activities act \
                         JOIN reminders r ON r.object_id = act.object_id \
                         WHERE act.id = ? AND r.client_uuid = ? AND act.deleted_at IS NULL")
                        .bind(referenced).bind(&op.entity_uuid)
                        .fetch_optional(&mut *tx).await?,
                    _ => Some(referenced),
                };
                if permitted.is_none() {
                    return Ok(Outcome::Rejected {
                        reason: format!("{field} must reference a row on the same object"),
                    });
                }
            }

            let stored: Option<(String, String)> = sqlx::query_as(
                "SELECT edited_at, device_id FROM field_clock \
                 WHERE entity = ? AND entity_uuid = ? AND field = ?")
                .bind(op.entity.as_str()).bind(&op.entity_uuid).bind(field)
                .fetch_optional(&mut *tx).await?;

            if let Some((stored_at, stored_device)) = &stored {
                if !wins(&op.edited_at, &op.device_id, stored_at, stored_device) {
                    return Ok(Outcome::Superseded);
                }
            }

            // `field` and the table name are both from closed sets (the whitelist and
            // `Entity::table`), never from the request, so this interpolation cannot be
            // steered by a caller. The value stays a bind parameter.
            let sql = format!(
                "UPDATE {} SET {field} = ? WHERE client_uuid = ?",
                op.entity.table()
            );
            let query = sqlx::query(&sql);
            let query = match op.value.as_ref() {
                None | Some(serde_json::Value::Null) => query.bind(None::<String>),
                Some(serde_json::Value::String(s)) => query.bind(s.clone()),
                Some(serde_json::Value::Number(n)) => query.bind(n.as_i64()),
                Some(serde_json::Value::Bool(b)) => query.bind(Some(i64::from(*b))),
                Some(other) => {
                    return Ok(Outcome::Rejected {
                        reason: format!("unsupported value type: {other}"),
                    })
                }
            };
            query.bind(&op.entity_uuid).execute(&mut *tx).await?;

            sqlx::query(
                "INSERT INTO field_clock (entity, entity_uuid, field, edited_at, device_id) \
                 VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(entity, entity_uuid, field) \
                 DO UPDATE SET edited_at = excluded.edited_at, device_id = excluded.device_id")
                .bind(op.entity.as_str()).bind(&op.entity_uuid).bind(field)
                .bind(&op.edited_at).bind(&op.device_id)
                .execute(&mut *tx).await?;

            Ok(Outcome::Accepted)
        }
    }
}
```

- [ ] **Step 4: Write the handler**

Create `src/api/sync.rs`:

```rust
//! The sync endpoints. Thin: every decision lives in `crate::sync`.

use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use crate::sync::{apply::apply_op, Op, Outcome};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn router() -> Router<App> {
    Router::new().route("/sync/push", post(push))
}

#[derive(Deserialize)]
pub struct PushBody {
    pub ops: Vec<Op>,
}

#[derive(Serialize)]
pub struct OpResult {
    pub client_op_id: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Serialize)]
pub struct PushOut {
    pub results: Vec<OpResult>,
    pub server_time: String,
    /// uuid to integer id, so a client can resolve references it only knows by uuid.
    pub ids: HashMap<String, i64>,
}

/// The whole batch lands in one transaction: a client that retries after a lost response must
/// find either all of its ops recorded or none, never a prefix it cannot identify.
async fn push(
    State(state): State<App>,
    user: AuthUser,
    Json(mut body): Json<PushBody>,
) -> Result<Json<PushOut>, AppError> {
    let mut tx = state.db.begin().await?;
    let mut results = Vec::with_capacity(body.ops.len());
    let mut ids = HashMap::new();

    // Canonicalise before anything reads the value: the ordering rule, the `field_clock` row
    // it is compared against, and the copy written into `changes` must all be the same shape,
    // or a later comparison is against a string that never went through here.
    for op in &mut body.ops {
        match crate::sync::apply::canonical_edited_at(&op.edited_at) {
            Some(canonical) => op.edited_at = canonical,
            None => {
                results.push(OpResult {
                    client_op_id: op.client_op_id.clone(),
                    outcome: Outcome::Rejected { reason: "edited_at must be RFC3339".into() },
                });
                op.edited_at = String::new();
            }
        }
    }

    for op in &body.ops {
        // Rejected above by canonicalisation; its result is already recorded.
        if op.edited_at.is_empty() {
            continue;
        }
        // Idempotency: an op id already in the log was applied by an earlier attempt whose
        // response the client never saw. Report it as accepted without applying it twice.
        let seen: Option<i64> = sqlx::query_scalar(
            "SELECT seq FROM changes WHERE user_id = ? AND client_op_id = ?")
            .bind(user.id).bind(&op.client_op_id)
            .fetch_optional(&mut *tx)
            .await?;
        if seen.is_some() {
            results.push(OpResult {
                client_op_id: op.client_op_id.clone(),
                outcome: Outcome::Accepted,
            });
            continue;
        }

        let outcome = apply_op(&mut tx, user.id, op).await?;
        if !matches!(outcome, Outcome::Rejected { .. }) {
            sqlx::query(
                "INSERT INTO changes \
                 (entity, entity_uuid, op, field, value, edited_at, applied_at, user_id, \
                  device_id, client_op_id) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(op.entity.as_str())
                .bind(&op.entity_uuid)
                .bind(op.op.as_str())
                .bind(op.field.as_deref())
                .bind(op.value.as_ref().map(|v| v.to_string()))
                .bind(&op.edited_at)
                .bind(db::now())
                .bind(user.id)
                .bind(&op.device_id)
                .bind(&op.client_op_id)
                .execute(&mut *tx)
                .await?;

            let sql = format!("SELECT id FROM {} WHERE client_uuid = ?", op.entity.table());
            if let Some(id) = sqlx::query_scalar::<_, i64>(&sql)
                .bind(&op.entity_uuid)
                .fetch_optional(&mut *tx)
                .await?
            {
                ids.insert(op.entity_uuid.clone(), id);
            }
        }
        results.push(OpResult { client_op_id: op.client_op_id.clone(), outcome });
    }

    tx.commit().await?;
    Ok(Json(PushOut { results, server_time: db::now(), ids }))
}
```

In `src/api/mod.rs`: add `pub mod sync;` to the module list (alphabetically, after `pub mod settings;`) and `.merge(sync::router())` to the router chain after `.merge(search::router())`.

- [ ] **Step 5: Document the route**

In `docs/openapi.json`, add a `/sync/push` entry under `paths` following the shape the neighbouring entries use — a `post` with a `requestBody` carrying `ops`, and a `200` response describing `results`, `server_time` and `ids`. `tests/openapi.rs` compares path and method names only, so the schema detail is for human readers, but the path and `post` method must both be present.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test`
Expected: PASS, including `tests/openapi.rs`, which fails loudly if the route was added without the doc entry.

- [ ] **Step 7: Commit**

```bash
git add src tests/sync.rs docs/openapi.json
git commit -m "feat: accept pushed ops with field-level last-write-wins"
```

---

### Task 6: GET /api/sync/pull

**Files:**
- Modify: `src/api/sync.rs`, `docs/openapi.json`
- Create: `src/sync/feed.rs`
- Modify: `src/sync/mod.rs` (add `pub mod feed;`)
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: the `changes` table and `apply_op` logging from Task 5.
- Produces:
  - `pub async fn pull(db: &sqlx::SqlitePool, user_id: i64, since: i64, limit: i64) -> Result<Vec<ChangeRow>, AppError>` in `src/sync/feed.rs`, with `pub struct ChangeRow { pub seq: i64, pub entity: String, pub entity_uuid: String, pub op: String, pub field: Option<String>, pub value: Option<String>, pub edited_at: String, pub device_id: String }` deriving `Serialize` and `sqlx::FromRow`.
  - `pub async fn horizon(db: &sqlx::SqlitePool, user_id: i64) -> Result<i64, AppError>` returning the lowest `seq` still retained for that user, or 0 when the log is empty.
  - `GET /api/sync/pull?since=&limit=` returning `{ "changes": [...], "next_seq": i64, "complete": bool, "server_time": String }`, or `410` when `since` predates the horizon.

- [ ] **Step 1: Write the failing test**

Append to `tests/sync.rs`:

```rust
#[tokio::test]
async fn pull_returns_ops_after_the_cursor_and_advances_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    for (n, name) in [("op-a", "First"), ("op-b", "Second")] {
        app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": n, "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "name", "value": name,
            "edited_at": format!("2026-03-0{}T00:00:00Z", if n == "op-a" { 1 } else { 2 }),
            "device_id": "phone"
        }]))).send().await.unwrap();
    }

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 2);
    assert_eq!(body["complete"], true);
    assert!(body["server_time"].is_string());
    let next = body["next_seq"].as_i64().unwrap();

    let body: serde_json::Value = app.client.get(app.url(&format!("/sync/pull?since={next}")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 0, "the cursor is exhausted");
}

#[tokio::test]
async fn pull_pages_and_reports_incompleteness() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();

    for i in 0..3 {
        app.client.post(app.url("/sync/push")).json(&push_body(json!([{
            "client_op_id": format!("op-{i}"), "entity": "object", "entity_uuid": uuid,
            "op": "set", "field": "description", "value": format!("note {i}"),
            "edited_at": format!("2026-04-0{}T00:00:00Z", i + 1), "device_id": "phone"
        }]))).send().await.unwrap();
    }

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0&limit=2"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 2);
    assert_eq!(body["complete"], false, "more remains behind the page");
}

#[tokio::test]
async fn pull_never_leaks_another_users_changes() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-ben", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Ben's",
        "edited_at": "2026-05-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();

    let other = app.create_user_client("mallory", "another password").await;
    let body: serde_json::Value = other.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["changes"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_cursor_before_the_horizon_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-kept", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Kept",
        "edited_at": "2026-06-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();

    // Simulate a purge having removed everything before this row.
    sqlx::query("UPDATE changes SET seq = 500 WHERE client_op_id = 'op-kept'")
        .execute(&app.state.db).await.unwrap();

    let res = app.client.get(app.url("/sync/pull?since=1")).send().await.unwrap();
    assert_eq!(res.status(), 410, "a stale cursor must be told to re-bootstrap");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test sync pull`
Expected: FAIL — 404, the route does not exist.

- [ ] **Step 3: Add the `Gone` error variant**

In `src/error.rs`, add to the `AppError` enum after `Conflict`:

```rust
    #[error("cursor is older than the retained history")]
    Gone,
```

and to `status_and_code`:

```rust
            AppError::Gone => (StatusCode::GONE, "gone"),
```

- [ ] **Step 4: Write the feed queries**

Create `src/sync/feed.rs`:

```rust
//! Reading the change log: the pull cursor and the retention horizon.

use crate::error::AppError;
use serde::Serialize;

#[derive(Serialize, sqlx::FromRow, Debug, Clone)]
pub struct ChangeRow {
    pub seq: i64,
    pub entity: String,
    pub entity_uuid: String,
    pub op: String,
    pub field: Option<String>,
    pub value: Option<String>,
    pub edited_at: String,
    pub device_id: String,
}

pub async fn pull(
    db: &sqlx::SqlitePool,
    user_id: i64,
    since: i64,
    limit: i64,
) -> Result<Vec<ChangeRow>, AppError> {
    Ok(sqlx::query_as::<_, ChangeRow>(
        "SELECT seq, entity, entity_uuid, op, field, value, edited_at, device_id \
         FROM changes WHERE user_id = ? AND seq > ? ORDER BY seq LIMIT ?")
        .bind(user_id)
        .bind(since)
        .bind(limit)
        .fetch_all(db)
        .await?)
}

/// The oldest `seq` still retained for this user, or 0 when the log is empty.
///
/// A client whose cursor sits below this has missed ops that were purged, so an incremental
/// pull would silently skip them -- it has to re-bootstrap instead.
pub async fn horizon(db: &sqlx::SqlitePool, user_id: i64) -> Result<i64, AppError> {
    let lowest: Option<i64> = sqlx::query_scalar("SELECT min(seq) FROM changes WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(db)
        .await?;
    Ok(lowest.unwrap_or(0))
}
```

In `src/sync/mod.rs`, add `pub mod feed;` beside `pub mod apply;`.

- [ ] **Step 5: Write the handler**

In `src/api/sync.rs`, add the imports `use axum::extract::Query;`, `use axum::routing::get;`, and `use crate::sync::feed;`, then extend the router:

```rust
pub fn router() -> Router<App> {
    Router::new()
        .route("/sync/push", post(push))
        .route("/sync/pull", get(pull))
}
```

and add:

```rust
/// A page big enough that a normal catch-up is one round trip, small enough that a phone on a
/// bad connection is not asked to hold a huge response in memory.
const DEFAULT_LIMIT: i64 = 500;
const MAX_LIMIT: i64 = 1000;

#[derive(Deserialize)]
pub struct PullParams {
    #[serde(default)]
    pub since: i64,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct PullOut {
    pub changes: Vec<feed::ChangeRow>,
    pub next_seq: i64,
    /// False when the page filled exactly, meaning the client should pull again immediately.
    pub complete: bool,
    pub server_time: String,
}

async fn pull(
    State(state): State<App>,
    user: AuthUser,
    Query(params): Query<PullParams>,
) -> Result<Json<PullOut>, AppError> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let horizon = feed::horizon(&state.db, user.id).await?;
    // `since` of 0 is a first pull and always legal; anything below the horizon has missed
    // purged ops, and resuming from it would skip them without either side noticing.
    if params.since > 0 && params.since < horizon - 1 {
        return Err(AppError::Gone);
    }
    let changes = feed::pull(&state.db, user.id, params.since, limit).await?;
    let complete = (changes.len() as i64) < limit;
    let next_seq = changes.last().map(|c| c.seq).unwrap_or(params.since);
    Ok(Json(PullOut { changes, next_seq, complete, server_time: db::now() }))
}
```

- [ ] **Step 6: Document the route**

Add `/sync/pull` with a `get` to `docs/openapi.json`, describing `since` and `limit` query parameters and the `200` and `410` responses.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src tests/sync.rs docs/openapi.json
git commit -m "feat: serve the change feed with a cursor and a retention horizon"
```

---

### Task 7: GET /api/sync/bootstrap

**Files:**
- Modify: `src/api/sync.rs`, `src/sync/feed.rs`, `docs/openapi.json`
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: `feed::pull` and the tombstone columns.
- Produces: `GET /api/sync/bootstrap` returning `{ "objects": [...], "activities": [...], "reminders": [...], "attachments": [...], "files": [...], "seq": i64, "server_time": String }`, where `seq` is the cursor the client resumes incremental pulls from, and every array holds only live (non-tombstoned) rows belonging to the caller.

- [ ] **Step 1: Write the failing test**

Append to `tests/sync.rs`:

```rust
#[tokio::test]
async fn bootstrap_returns_live_rows_and_a_resumable_cursor() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let keep = app.create_object(&app.client, "Golf", Some("km")).await;
    let drop = app.create_object(&app.client, "Old Bike", None).await;
    let drop_id = drop["id"].as_i64().unwrap();
    assert_eq!(
        app.client.delete(app.url(&format!("/objects/{drop_id}"))).send().await.unwrap().status(),
        204
    );

    let body: serde_json::Value = app.client.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();

    let objects = body["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 1, "the tombstoned object is absent");
    assert_eq!(objects[0]["name"], keep["name"]);
    assert!(objects[0]["client_uuid"].is_string(), "rows are addressable by uuid");
    assert!(body["seq"].is_i64());
    assert!(body["server_time"].is_string());

    // The cursor is immediately usable.
    let seq = body["seq"].as_i64().unwrap();
    let res = app.client.get(app.url(&format!("/sync/pull?since={seq}"))).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn bootstrap_is_scoped_to_the_caller() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let other = app.create_user_client("mallory", "another password").await;
    let body: serde_json::Value = other.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test sync bootstrap`
Expected: FAIL — 404.

- [ ] **Step 3: Write the snapshot query**

Append to `src/sync/feed.rs`:

```rust
/// Every live row the user owns, plus the cursor to resume incremental pulls from.
///
/// The cursor is read BEFORE the rows. Taken afterwards, an op landing between the two reads
/// would be baked into the snapshot and then delivered again by the first pull -- harmless for
/// an idempotent apply, but it would also let an op that landed between them be skipped if the
/// order were reversed. Reading it first can only ever repeat work, never lose it.
pub async fn snapshot(
    db: &sqlx::SqlitePool,
    user_id: i64,
) -> Result<(i64, serde_json::Value), AppError> {
    let seq: i64 = sqlx::query_scalar("SELECT coalesce(max(seq), 0) FROM changes WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(db)
        .await?;

    let objects = rows(db, "SELECT * FROM objects WHERE user_id = ? AND deleted_at IS NULL", user_id).await?;
    let activities = rows(db,
        "SELECT a.* FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = ? AND a.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let reminders = rows(db,
        "SELECT r.* FROM reminders r JOIN objects o ON o.id = r.object_id \
         WHERE o.user_id = ? AND r.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let attachments = rows(db,
        "SELECT t.* FROM attachments t JOIN objects o ON o.id = t.object_id \
         WHERE o.user_id = ? AND t.deleted_at IS NULL AND o.deleted_at IS NULL", user_id).await?;
    let files = rows(db, "SELECT * FROM files WHERE user_id = ? AND deleted_at IS NULL", user_id).await?;

    Ok((seq, serde_json::json!({
        "objects": objects,
        "activities": activities,
        "reminders": reminders,
        "attachments": attachments,
        "files": files,
    })))
}

/// Reads a whole table as JSON objects, column names taken from the result set.
///
/// Generic in the shape it returns because the snapshot ships rows verbatim -- a typed struct
/// per table would have to be kept in step with five schemas for no gain to any caller.
async fn rows(
    db: &sqlx::SqlitePool,
    sql: &str,
    user_id: i64,
) -> Result<Vec<serde_json::Value>, AppError> {
    use sqlx::{Column, Row, TypeInfo, ValueRef};
    let fetched = sqlx::query(sql).bind(user_id).fetch_all(db).await?;
    let mut out = Vec::with_capacity(fetched.len());
    for row in fetched {
        let mut map = serde_json::Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            let raw = row.try_get_raw(i)?;
            let value = if raw.is_null() {
                serde_json::Value::Null
            } else {
                match raw.type_info().name() {
                    "INTEGER" => serde_json::json!(row.try_get::<i64, _>(i)?),
                    "REAL" => serde_json::json!(row.try_get::<f64, _>(i)?),
                    _ => serde_json::json!(row.try_get::<String, _>(i)?),
                }
            };
            map.insert(col.name().to_string(), value);
        }
        out.push(serde_json::Value::Object(map));
    }
    Ok(out)
}
```

- [ ] **Step 4: Write the handler**

In `src/api/sync.rs`, extend the router with `.route("/sync/bootstrap", get(bootstrap))` and add:

```rust
async fn bootstrap(
    State(state): State<App>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let (seq, mut snapshot) = feed::snapshot(&state.db, user.id).await?;
    let map = snapshot.as_object_mut().expect("snapshot builds a JSON object");
    map.insert("seq".into(), serde_json::json!(seq));
    map.insert("server_time".into(), serde_json::json!(db::now()));
    Ok(Json(snapshot))
}
```

- [ ] **Step 5: Document the route**

Add `/sync/bootstrap` with a `get` to `docs/openapi.json`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src tests/sync.rs docs/openapi.json
git commit -m "feat: serve a full snapshot for a new or expired device"
```

---

### Task 8: Retention purge

**Files:**
- Modify: `src/sync/feed.rs`, `src/tasks.rs`
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: `changes` and the tombstone columns.
- Produces: `pub async fn purge(state: &crate::state::App, retention_days: i64) -> Result<u64, AppError>` in `src/sync/feed.rs`, returning the number of `changes` rows removed. `src/tasks.rs` calls it hourly with `RETENTION_DAYS`.

It takes `&App` rather than a bare pool because reclaiming an expired attachment's blob needs `state.storage`. Task 2 made this necessary: a tombstoned attachment still pins its file through `attachments.file_id ... ON DELETE RESTRICT`, so `purge_orphan_files` cannot free anything until the tombstone itself expires. If this purge dropped the rows without that call, every deleted photo would leave its bytes on disk permanently.

Taking the window as a parameter rather than reading a constant is what makes this testable in a second instead of ninety days — the same shape `notify::tick(&state, hour)` already uses.

- [ ] **Step 1: Write the failing test**

Append to `tests/sync.rs`:

```rust
#[tokio::test]
async fn purge_drops_old_log_rows_and_old_tombstones() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();

    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-ancient", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Ancient",
        "edited_at": "2026-01-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();

    // Backdate both the log row and a tombstone well past any sane window.
    sqlx::query("UPDATE changes SET applied_at = '2000-01-01T00:00:00Z'")
        .execute(&app.state.db).await.unwrap();
    sqlx::query("UPDATE objects SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = ?")
        .bind(object_id).execute(&app.state.db).await.unwrap();

    let removed = logby::sync::feed::purge(&app.state, 90).await.unwrap();
    assert_eq!(removed, 1, "the ancient log row went");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(rows, 0);

    let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = ?")
        .bind(object_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(objects, 0, "an expired tombstone is finally a real delete");
}

#[tokio::test]
async fn purge_keeps_recent_history() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = ?")
        .bind(car["id"].as_i64().unwrap())
        .fetch_one(&app.state.db).await.unwrap();
    app.client.post(app.url("/sync/push")).json(&push_body(json!([{
        "client_op_id": "op-fresh", "entity": "object", "entity_uuid": uuid,
        "op": "set", "field": "name", "value": "Fresh",
        "edited_at": "2026-01-01T00:00:00Z", "device_id": "phone"
    }]))).send().await.unwrap();

    assert_eq!(logby::sync::feed::purge(&app.state, 90).await.unwrap(), 0);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM changes")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(rows, 1, "today's history is not history yet");
}

#[tokio::test]
async fn purge_reclaims_the_blob_of_an_expired_attachment() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = car["id"].as_i64().unwrap();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"%PDF-1.4 fake".to_vec())
            .file_name("invoice.pdf")
            .mime_str("application/pdf")
            .unwrap(),
    );
    let res = app.client.post(app.url(&format!("/objects/{object_id}/attachments")))
        .multipart(form).send().await.unwrap();
    assert_eq!(res.status(), 201, "upload failed: {}", res.text().await.unwrap());
    let attachment_id = res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap();

    let sha: String = sqlx::query_scalar(
        "SELECT f.sha256 FROM files f JOIN attachments a ON a.file_id = f.id WHERE a.id = ?")
        .bind(attachment_id).fetch_one(&app.state.db).await.unwrap();
    let blob = app.state.storage.blob_path(&sha);
    assert!(blob.exists(), "the upload landed on disk");

    assert_eq!(
        app.client.delete(app.url(&format!("/attachments/{attachment_id}")))
            .send().await.unwrap().status(),
        204
    );
    assert!(blob.exists(), "a tombstoned attachment still pins its blob");

    sqlx::query("UPDATE attachments SET deleted_at = '2000-01-01T00:00:00Z'")
        .execute(&app.state.db).await.unwrap();
    logby::sync::feed::purge(&app.state, 90).await.unwrap();

    assert!(!blob.exists(), "an expired tombstone finally frees the bytes");
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM files")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(files, 0, "the files row goes with its last attachment");
}
```

`purge_orphan_files` is `pub` on `crate::api::attachments` already, so no visibility change is needed. If it is not reachable from `src/sync/feed.rs`, widen it rather than duplicating its logic — the reference re-check it performs is the whole point of calling it.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test sync purge`
Expected: FAIL to compile — `logby::sync::feed::purge` does not exist.

- [ ] **Step 3: Write the purge**

Append to `src/sync/feed.rs`:

```rust
/// Drops log rows and tombstones older than the retention window.
///
/// Tombstones go last and only once their own `deleted_at` has aged out, so a device that is
/// still inside the window always finds the delete in the log before the row itself vanishes.
/// Beyond the window a device has to re-bootstrap anyway, which is precisely when it no longer
/// needs the tombstone to learn the row is gone.
pub async fn purge(
    state: &crate::state::App,
    retention_days: i64,
) -> Result<u64, AppError> {
    let db = &state.db;
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let removed = sqlx::query("DELETE FROM changes WHERE applied_at < ?")
        .bind(&cutoff)
        .execute(db)
        .await?
        .rows_affected();

    // Which files the expiring attachments were pinning, read BEFORE those rows go: once the
    // attachment is deleted the link is gone, and with it any way to find the blob to reclaim.
    let pinned: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT file_id FROM attachments \
         WHERE deleted_at IS NOT NULL AND deleted_at < ?")
        .bind(&cutoff)
        .fetch_all(db)
        .await?;

    // `files` is absent deliberately: nothing sets `files.deleted_at`, because a file is
    // content-addressed and shared between attachments. A file dies when its last attachment
    // does, which is what `purge_orphan_files` decides below.
    for table in ["attachments", "activities", "reminders", "objects"] {
        let sql = format!("DELETE FROM {table} WHERE deleted_at IS NOT NULL AND deleted_at < ?");
        sqlx::query(&sql).bind(&cutoff).execute(db).await?;
    }

    // Now that no attachment references them, the unreferenced ones can give up their blob and
    // thumbnail. `purge_orphan_files` re-checks each candidate against the live attachments, so
    // a file still used by another object is left alone.
    crate::api::attachments::purge_orphan_files(state, &pinned).await?;

    // A field clock for a row nobody can name any more is dead weight.
    sqlx::query(
        "DELETE FROM field_clock WHERE entity_uuid NOT IN (\
           SELECT client_uuid FROM objects UNION ALL SELECT client_uuid FROM activities \
           UNION ALL SELECT client_uuid FROM reminders UNION ALL SELECT client_uuid FROM attachments \
           UNION ALL SELECT client_uuid FROM files)")
        .execute(db)
        .await?;

    Ok(removed)
}
```

- [ ] **Step 4: Wire it into the background loop**

In `src/tasks.rs`, add the constant beside `PRUNE_EVERY`:

```rust
/// How long a device may stay offline before an incremental pull is no longer possible and it
/// must re-bootstrap. Also how long a tombstone survives, since the two are the same guarantee
/// seen from either end.
const RETENTION_DAYS: i64 = 90;
```

and inside the `if since_prune >= PRUNE_EVERY` block, after the existing `prune_sessions` match:

```rust
                match crate::sync::feed::purge(&state, RETENTION_DAYS).await {
                    Ok(n) if n > 0 => tracing::debug!(changes = n, "purged expired sync history"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "sync purge failed"),
                }
```

- [ ] **Step 5: Run the whole suite**

Run: `cargo test && cargo clippy --all-targets --locked -- -D warnings`
Expected: PASS for both.

- [ ] **Step 6: Commit**

```bash
git add src tests/sync.rs
git commit -m "feat: purge sync history and tombstones past the retention window"
```

---

## Self-Review

**Spec coverage.** Every Phase 1 item in the spec maps to a task: `client_uuid` on all five tables and the two log tables (Task 1); tombstones (Tasks 1–2); the LWW rule with a `device_id` tie-break (Task 4); `POST /sync/push` with per-op outcomes, idempotency and UUID-to-id mapping (Task 5); `GET /sync/pull` with `server_time`, `next_seq`, `complete` and `410` (Task 6); `GET /sync/bootstrap` (Task 7); the 90-day purge (Task 8).

Two spec items are deliberately **not** here, because they belong to later phases: the client's clock-offset correction (the server publishes `server_time` in every response, which is the whole of its half of that contract), and blob sync, which Phase 5 covers.

One spec sentence needed a decision the spec left implicit: it says ops address entities by UUID and the server maps UUID to id, but not who inserts a row born offline. Task 5 resolves it — a `create` op is logged, not applied, because the client still calls the ordinary REST create. That keeps validation, file handling and the `client_op_id` idempotency already in `activities.rs` and `attachments.rs` as the single insert path rather than growing a second one. Flagging it because it is a real design choice made during planning rather than during brainstorming.

**Placeholder scan.** No TBDs, no "add error handling", no "similar to Task N". Task 1 Step 5, Task 2 Step 4 and Task 6 Step 6 direct the engineer to a `grep` rather than listing call sites — deliberate, since the exact line numbers will have moved by the time they run it, and the greps are exact.

**Type consistency.** `Entity`, `OpKind`, `Op`, `Outcome` and `is_syncable_field` are defined in Task 3 and used unchanged in Tasks 4–5. `wins` has one signature throughout. `feed::pull`, `feed::horizon`, `feed::snapshot` and `feed::purge` are each defined once and referenced with matching arguments. `AppError::Gone` is added in Task 6 Step 3 before its first use in Task 6 Step 5.
