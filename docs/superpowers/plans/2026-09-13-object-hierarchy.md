# Object Hierarchy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An object can have another object as its parent, so a garage can hold a main light
and a secondary light, each with its own independent history — and deleting the garage takes
both with it, correctly, on both backends.

**Architecture:** One new column, `objects.parent_id`, self-referential and nullable. One
shared validation function used by both the REST handler and the sync path, because this is
the first field in the app that can create a cycle if the two doors ever disagree. The existing
delete cascade becomes recursive rather than gaining a parallel mechanism.

**Tech Stack:** Rust, axum, sqlx over `AnyPool` (SQLite and PostgreSQL), Svelte 5, vitest,
Playwright.

## Global Constraints

- SQLite and PostgreSQL backend suites, vitest, and Playwright must all pass, unchanged where
  this plan does not touch them, at every task boundary. Record the counts you see at the start
  of Task 1 and treat any drop as a regression to explain, not to shrug past.
- No new dependency, runtime or dev.
- The palette does not change; every user-facing string exists in both `frontend/src/i18n/en.ts`
  and `de.ts`.
- No new object types, no activity-category changes, no rollup of a child's stats into its
  parent. These are explicitly out of scope in the spec; do not add them "while you're in there".
- A field that references another row gets checked identically wherever it can be written.
  `parent_id`'s check additionally has to refuse a cycle — the one property nothing in this app
  has needed to guard before.
- Run verification in the FOREGROUND, with long timeouts. Many agents on this project have lost
  a turn waiting on a background command that died when their turn ended.
- The debug binary serves `frontend/dist` from disk: `npm run build` is enough after a frontend
  change; a Rust change needs `cargo build`.

## Running PostgreSQL for a task's verification

```bash
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16
until docker exec logb-pg pg_isready -q; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test
docker rm -f logb-pg
```

A fixed `sleep` after starting the container is not enough — wait for `pg_isready`.

## File structure

| File | Responsibility |
|---|---|
| `migrations/sqlite/0010_object_hierarchy.sql` (new) | Adds the column and index to SQLite |
| `migrations/postgres/0001_schema.sql` | Adds the same column and index to the consolidated PostgreSQL schema |
| `src/sync/record.rs` | `id_of`, `parent_is_valid`, and the now-recursive `cascade_object` |
| `src/sync/mod.rs` | Whitelist entry and its type-parity test list |
| `src/sync/apply.rs` | The `Set` arm that validates a `parent_id` write |
| `src/api/objects.rs` | `ObjectRow`/`ObjectInput` gain `parent_id`; create/update validate it; `list` gains the root/child/all query shape; single-object read gains `ancestors` |
| `src/sync/feed.rs` | The purge guard gains a fourth relationship to check before hard-deleting an object |
| `src/api/export.rs` | `parent_id` is not written to `data.json`; import never sets it |
| `src/api/search.rs` | Object hits carry their parent's name |
| `frontend/src/lib/types.ts` | `MemObject`/`ObjectInput` gain `parent_id`; `ObjectOut`-equivalent gains `ancestors` |
| `frontend/src/lib/object-tree.ts` (new) | The pure function that keeps the parent picker from offering a doomed choice |
| `frontend/src/routes/ObjectForm.svelte` | The parent picker |
| `frontend/src/routes/ObjectDetail.svelte` | Breadcrumb and the Contents section |
| `frontend/src/lib/i18n/{en,de}.ts` | New strings |

`frontend/src/routes/Dashboard.svelte` needs **no change**. It already calls
`GET /objects?archived=${archived}` with no `parent_id`, which Task 4 makes mean "roots only" —
so the dashboard automatically stops listing nested objects the moment that task lands, with
zero frontend code touched. Do not add a task for it; do not add one by accident either, inside
some other task.

---

### Task 1: The column, on both backends

**Files:**
- Create: `migrations/sqlite/0010_object_hierarchy.sql`
- Modify: `migrations/postgres/0001_schema.sql`
- Test: `tests/schema_parity.rs` (already exists — no change needed, only a run)

**Interfaces:**
- Produces: `objects.parent_id`, nullable, referencing `objects(id)`, no `ON DELETE` clause
  (defaults to restrict on both backends — the database itself must refuse a hard delete while
  any row still points at the parent via `parent_id`, as a second layer under the application
  guard Task 6 adds).

- [ ] **Step 1: Record the baseline**

Run: `cargo test --quiet 2>&1 | grep -E "test result"` and, with a PostgreSQL container up,
the same command with `LOGB_TEST_DATABASE_URL` set. Record both pass counts — later tasks in
this plan will report against them.

- [ ] **Step 2: Write the SQLite migration**

Create `migrations/sqlite/0010_object_hierarchy.sql`:

```sql
-- An object may belong inside another one: a garage inside a house, a light inside the garage.
-- Nullable, so every existing object -- which has no parent today -- needs no backfill.
--
-- No `ON DELETE` clause: deletion in this app is a soft tombstone (`UPDATE ... SET deleted_at`),
-- not a real `DELETE`, so `ON DELETE CASCADE` would only ever fire during the retention purge's
-- hard delete, days after a user-facing delete already happened via `record::cascade_object`.
-- Leaving it unspecified means SQLite (and PostgreSQL) refuse a hard delete of a parent while
-- any row still references it -- a second, defence-in-depth layer under the purge guard in
-- `sync/feed.rs`, which must already have tombstoned every descendant before this could matter.
ALTER TABLE objects ADD COLUMN parent_id INTEGER REFERENCES objects(id);
CREATE INDEX idx_objects_parent ON objects(parent_id);
```

- [ ] **Step 3: Add the same column to the PostgreSQL schema**

In `migrations/postgres/0001_schema.sql`, inside the `CREATE TABLE objects (...)` block, add a
line for `parent_id` (matching the SQLite column's nullability and lack of an `ON DELETE`
clause — PostgreSQL's default is `NO ACTION`, which behaves the same as SQLite's default here):

```sql
    parent_id            BIGINT REFERENCES objects(id),
```

Immediately after the `objects` table's other `CREATE INDEX` lines in that file, add:

```sql
CREATE INDEX idx_objects_parent ON objects(parent_id);
```

- [ ] **Step 4: Run the schema parity test on both backends**

Run: `cargo test --test schema_parity` on SQLite, then again with `LOGB_TEST_DATABASE_URL` set.

Expected: passes on both. If it does not, the two files disagree on the column's type,
nullability, or the index — fix whichever is wrong; the two must describe the same shape.

- [ ] **Step 5: Run the whole suite on both backends**

Expected: unchanged from the baseline in Step 1. Nothing yet reads or writes `parent_id`, so
nothing should behave differently.

- [ ] **Step 6: Commit**

```bash
git add migrations/sqlite/0010_object_hierarchy.sql migrations/postgres/0001_schema.sql
git commit -m "feat: an object may belong inside another one"
```

---

### Task 2: The shared check — existence, ownership, and no cycle

**Files:**
- Modify: `src/sync/record.rs`
- Test: `tests/objects.rs` (or wherever this project's object-level Rust integration tests
  live — check the file list under `tests/` and follow its existing style)

**Interfaces:**
- Produces: `pub(crate) async fn id_of(tx: &mut sqlx::AnyConnection, entity: Entity, uuid: &str) -> Result<i64, AppError>` —
  the reverse of the existing `uuid_of`.
- Produces: `pub(crate) async fn parent_is_valid(tx: &mut sqlx::AnyConnection, user_id: i64, object_id: Option<i64>, candidate_parent_id: i64) -> Result<bool, AppError>`.
  `object_id` is `None` on create (a brand-new object cannot yet be anyone's ancestor, so the
  cycle half of the check is skipped) and `Some(id)` on update.

This task builds the check in isolation, against the database directly, before anything in the
REST or sync layer calls it. That is deliberate: the recursive query is the riskiest new SQL in
this feature, and it should be proven correct on its own before two call sites depend on it.

- [ ] **Step 1: Write the failing tests**

Find this project's Rust integration test file for objects (likely `tests/objects.rs`) and
follow its existing harness style (`common::spawn()`, `app.create_object(...)`, direct SQL
against `app.state.db` where a test needs to reach past the API). Add:

```rust
/// A brand-new object cannot be anyone's ancestor, so creating one only needs to check that
/// the given parent exists, belongs to the caller, and is not deleted -- no cycle is possible
/// yet.
#[tokio::test]
async fn a_nonexistent_parent_is_invalid_on_create() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let ok = logb::sync::record::parent_is_valid(&mut app.state.db.acquire().await.unwrap(), 1, None, 999_999)
        .await
        .unwrap();
    assert!(!ok, "a parent id that does not exist must be invalid");
}

/// The core property this task exists for: reparenting an ancestor underneath its own
/// descendant must be refused, at every depth, including the trivial one-hop case of an object
/// naming itself.
#[tokio::test]
async fn a_cycle_is_refused_at_every_depth() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let house_id = house["id"].as_i64().unwrap();
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();

    let mut conn = app.state.db.acquire().await.unwrap();
    // Build House -> Garage -> Light directly, so this test is not entangled with the REST
    // handler this task's function does not yet feed into.
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(house_id).bind(garage_id)
        .execute(&mut *conn).await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(garage_id).bind(light_id)
        .execute(&mut *conn).await.unwrap();

    // Self-parent.
    assert!(!logb::sync::record::parent_is_valid(&mut conn, 1, Some(house_id), house_id).await.unwrap());
    // One hop: House under its own child.
    assert!(!logb::sync::record::parent_is_valid(&mut conn, 1, Some(house_id), garage_id).await.unwrap());
    // Two hops: House under its grandchild.
    assert!(!logb::sync::record::parent_is_valid(&mut conn, 1, Some(house_id), light_id).await.unwrap());
    // The legitimate direction must still work: Light may be reparented onto House directly.
    assert!(logb::sync::record::parent_is_valid(&mut conn, 1, Some(light_id), house_id).await.unwrap());
}

/// Ownership is not optional: a parent id that exists and has no ancestor relationship to the
/// object is still invalid if it belongs to someone else.
#[tokio::test]
async fn a_parent_belonging_to_another_user_is_invalid() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let mine = app.create_object(&app.client, "Mine", None).await;
    let anna = app.create_user_client("anna", "password123").await;
    let theirs_res = anna.post(app.url("/objects"))
        .json(&serde_json::json!({ "name": "Theirs", "type": "car", "counter_unit": null, "description": "", "purchase_date": null, "purchase_price_cents": null }))
        .send().await.unwrap();
    let theirs: serde_json::Value = theirs_res.json().await.unwrap();

    let mut conn = app.state.db.acquire().await.unwrap();
    let ok = logb::sync::record::parent_is_valid(
        &mut conn, 1, Some(mine["id"].as_i64().unwrap()), theirs["id"].as_i64().unwrap(),
    ).await.unwrap();
    assert!(!ok, "a parent owned by another account must be invalid regardless of ancestry");
}
```

`create_object`'s signature in `tests/common/mod.rs:776` is
`create_object(&self, client: &reqwest::Client, name: &str, unit: Option<&str>)` — match it
exactly; do not invent a different one. If `create_user_client` does not already exist in the
harness, check `tests/database_api.rs` or similar, which used exactly this shape for a
non-admin user, and follow it rather than writing a second way to make one.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test --quiet a_cycle_is_refused_at_every_depth a_nonexistent_parent_is_invalid_on_create a_parent_belonging_to_another_user_is_invalid`

Expected: compilation failure — `id_of` and `parent_is_valid` do not exist yet.

- [ ] **Step 3: Write `id_of`**

In `src/sync/record.rs`, beside the existing `uuid_of`:

```rust
/// The reverse of `uuid_of`: the internal id a `client_uuid` names, for entities where a
/// caller needs to reason about the row as an integer -- as `parent_is_valid` does, since the
/// tree it walks is built entirely out of integer `parent_id`s.
pub(crate) async fn id_of(
    tx: &mut sqlx::AnyConnection,
    entity: Entity,
    uuid: &str,
) -> Result<i64, AppError> {
    let sql = format!("SELECT id FROM {} WHERE client_uuid = $1", entity.table());
    let id: Option<i64> =
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql)).bind(uuid).fetch_one(&mut *tx).await?;
    Ok(id.expect("every row has carried a client_uuid since migration 0007_sync.sql"))
}
```

- [ ] **Step 4: Write `parent_is_valid`**

```rust
/// Whether `candidate_parent_id` may become `object_id`'s parent: it must exist, belong to
/// `user_id`, and not be deleted; and it must not be `object_id` itself or a descendant of it,
/// which would leave the object as its own ancestor once the write took effect.
///
/// `object_id` is `None` on create, where the object being created has no id yet and therefore
/// cannot possibly be anyone's ancestor -- only existence and ownership are checked there.
///
/// Shared by the REST handler and sync's `Set` handling. A recursive check written twice is a
/// recursive check that can drift into disagreeing twice, which is worse here than for a plain
/// existence check -- this is the one field in the app where the two doors disagreeing could
/// corrupt data rather than merely let a bad value through.
pub(crate) async fn parent_is_valid(
    tx: &mut sqlx::AnyConnection,
    user_id: i64,
    object_id: Option<i64>,
    candidate_parent_id: i64,
) -> Result<bool, AppError> {
    let exists: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM objects WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL")
        .bind(candidate_parent_id).bind(user_id)
        .fetch_optional(&mut *tx).await?;
    if exists.is_none() {
        return Ok(false);
    }
    let Some(object_id) = object_id else { return Ok(true) };

    // Walk the candidate's own ancestor chain (itself, then its parent, then its parent's
    // parent, ...). If `object_id` ever appears in it, making the candidate `object_id`'s
    // parent would close a loop: the candidate is already inside the subtree rooted at
    // `object_id`, including the trivial case where the candidate IS `object_id`.
    //
    // `crate::db::Bool`, not a bare `bool`: `EXISTS(...)` decodes as an INTEGER 0/1 on SQLite
    // and a real BOOLEAN on PostgreSQL, and `Decode<Any> for bool` only accepts the latter --
    // the same defect that once broke every `/users` read, here on a query this project has
    // never run before rather than a column it has always had.
    let would_cycle: (crate::db::Bool,) = sqlx::query_as(
        "WITH RECURSIVE ancestors(id) AS ( \
           SELECT id FROM objects WHERE id = $1 \
           UNION ALL \
           SELECT o.parent_id FROM objects o JOIN ancestors a ON o.id = a.id WHERE o.parent_id IS NOT NULL \
         ) SELECT EXISTS (SELECT 1 FROM ancestors WHERE id = $2)")
        .bind(candidate_parent_id).bind(object_id)
        .fetch_one(&mut *tx).await?;
    Ok(!bool::from(would_cycle.0))
}
```

- [ ] **Step 5: Run the new tests on both backends**

Run the three tests from Step 1 against SQLite, then against PostgreSQL with
`LOGB_TEST_DATABASE_URL` set. Expected: all pass on both. The cycle test is the one to look at
closely on PostgreSQL — report the exact output, since `WITH RECURSIVE` and `db::Bool` are
both new combinations this project has not exercised together before.

- [ ] **Step 6: Prove the cycle check can fail**

Comment out the `would_cycle` block and unconditionally `return Ok(true)` after the existence
check. Run `a_cycle_is_refused_at_every_depth` again and confirm it fails, naming which
assertion. Restore the real body and confirm it passes again. Report both outputs — this is the
single most important guard in this feature, and it must be seen catching the defect it exists
to catch.

- [ ] **Step 7: Run the whole suite on both backends**

Expected: unchanged from Task 1's baseline plus the three new tests.

- [ ] **Step 8: Commit**

```bash
git add src/sync/record.rs tests/objects.rs
git commit -m "feat: check a proposed parent for existence, ownership, and cycles"
```

---

### Task 3: Deleting an object deletes what is inside it

**Files:**
- Modify: `src/sync/record.rs` (`cascade_object`)
- Test: `tests/objects.rs`

**Interfaces:**
- Consumes: nothing new from Task 2 directly, but relies on the same `parent_id` column.
- Modifies: `cascade_object`'s signature changes from `async fn` to a plain `fn` returning a
  boxed future, because it now calls itself. Its callers (`src/api/objects.rs`'s `delete`, and
  `src/sync/apply.rs`'s delete arm) are unaffected — both already `.await` its result, which
  works identically on a boxed future.

- [ ] **Step 1: Write the failing test**

Append to `tests/objects.rs`:

```rust
/// Deleting an object deletes everything inside it, at every depth, and each descendant's own
/// activities, attachments and reminders go with it -- not just the descendant itself.
#[tokio::test]
async fn deleting_an_object_tombstones_every_descendant_and_their_own_children() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let house_id = house["id"].as_i64().unwrap();
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();
    app.create_activity(&light, "Changed the bulb").await;

    let mut conn = app.state.db.acquire().await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(house_id).bind(garage_id)
        .execute(&mut *conn).await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(garage_id).bind(light_id)
        .execute(&mut *conn).await.unwrap();
    drop(conn);

    let res = app.client.delete(app.url(&format!("/objects/{house_id}"))).send().await.unwrap();
    assert_eq!(res.status(), 204);

    let mut conn = app.state.db.acquire().await.unwrap();
    for id in [garage_id, light_id] {
        let deleted_at: Option<String> = sqlx::query_scalar("SELECT deleted_at FROM objects WHERE id = $1")
            .bind(id).fetch_one(&mut *conn).await.unwrap();
        assert!(deleted_at.is_some(), "object {id} must be tombstoned when its ancestor is deleted");
    }
    let live_activities: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM activities WHERE object_id = $1 AND deleted_at IS NULL")
        .bind(light_id).fetch_one(&mut *conn).await.unwrap();
    assert_eq!(live_activities, 0, "the light's own activity must be tombstoned too, not just the light");
}

/// Each cascaded descendant must appear in the change log as its own delete, or a device that
/// only pulls Garage's delete would never learn the light inside it was removed.
#[tokio::test]
async fn a_cascaded_descendant_is_logged_as_its_own_delete() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let garage_id = garage["id"].as_i64().unwrap();
    let light_id = light["id"].as_i64().unwrap();
    let mut conn = app.state.db.acquire().await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(garage_id).bind(light_id)
        .execute(&mut *conn).await.unwrap();
    drop(conn);

    app.client.delete(app.url(&format!("/objects/{garage_id}"))).send().await.unwrap();

    let light_uuid: String = sqlx::query_scalar("SELECT client_uuid FROM objects WHERE id = $1")
        .bind(light_id).fetch_one(&app.state.db).await.unwrap();
    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM changes WHERE entity = 'object' AND entity_uuid = $1 AND op = 'delete'")
        .bind(&light_uuid).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(logged, 1, "the light's own delete must reach the change log, or a device never learns it is gone");
}
```

Match `create_activity`'s real signature (`tests/common/mod.rs:807`) exactly.

- [ ] **Step 2: Run them and watch them fail**

Expected: `deleting_an_object_tombstones_every_descendant_and_their_own_children` fails —
`garage_id` and `light_id` remain live after "House" is deleted, since nothing yet cascades to
children.

- [ ] **Step 3: Make `cascade_object` recursive**

In `src/sync/record.rs`, change the function's signature and add a fourth cascaded
relationship. Rust cannot compile an `async fn` that calls itself (the resulting future would
have infinite size), so this becomes a plain `fn` returning an explicitly boxed future:

```rust
pub(crate) fn cascade_object<'a>(
    tx: &'a mut sqlx::AnyConnection,
    object_uuid: &'a str,
    now: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<(Entity, String)>, AppError>> + Send + 'a>> {
    Box::pin(async move {
        let mut cascaded = Vec::new();
        // Of the three cascaded tables, only `activities` carries `updated_at`
        // (migrations/sqlite/0001_init.sql) -- `reminders` and `attachments` don't, so there is nothing to
        // bump on those two.
        for (entity, has_updated_at) in
            [(Entity::Activity, true), (Entity::Reminder, false), (Entity::Attachment, false)]
        {
            let table = entity.table();
            fn mine(placeholder: u8) -> String {
                format!(
                    "deleted_at IS NULL AND object_id = (SELECT id FROM objects WHERE client_uuid = ${placeholder})"
                )
            }
            let select = format!("SELECT client_uuid FROM {table} WHERE {}", mine(1));
            let uuids: Vec<Option<String>> = sqlx::query_scalar(sqlx::AssertSqlSafe(select))
                .bind(object_uuid).fetch_all(&mut *tx).await?;
            let update = if has_updated_at {
                format!("UPDATE {table} SET deleted_at = $1, updated_at = $2 WHERE {}", mine(3))
            } else {
                format!("UPDATE {table} SET deleted_at = $1 WHERE {}", mine(2))
            };
            let query = sqlx::query(sqlx::AssertSqlSafe(update)).bind(now);
            let query = if has_updated_at { query.bind(now) } else { query };
            query.bind(object_uuid).execute(&mut *tx).await?;
            cascaded.extend(nameable(uuids).map(|uuid| (entity, uuid)));
        }

        // The fourth cascaded relationship, and the only recursive one: an object's direct
        // children. Each child tombstoned here may itself have children, so this calls itself
        // once per child. It terminates because `parent_is_valid`'s cycle check refuses every
        // write that could make this tree infinite -- there is no way to reach this point with
        // a loop in the ancestry.
        let child_uuids: Vec<String> = sqlx::query_scalar(
            "SELECT client_uuid FROM objects \
             WHERE deleted_at IS NULL AND parent_id = (SELECT id FROM objects WHERE client_uuid = $1)")
            .bind(object_uuid).fetch_all(&mut *tx).await?;
        sqlx::query(
            "UPDATE objects SET deleted_at = $1, updated_at = $1 \
             WHERE deleted_at IS NULL AND parent_id = (SELECT id FROM objects WHERE client_uuid = $2)")
            .bind(now).bind(object_uuid).execute(&mut *tx).await?;
        for child_uuid in &child_uuids {
            cascaded.push((Entity::Object, child_uuid.clone()));
            cascaded.extend(cascade_object(&mut *tx, child_uuid, now).await?);
        }
        Ok(cascaded)
    })
}
```

`nameable` is a helper already defined elsewhere in this same file (`record.rs`) that turns a
`Vec<Option<String>>` into an iterator of real uuids — read the current, un-modified
`cascade_object` first to confirm its exact name and signature before relying on it here; do
not introduce a second helper with a different name.

- [ ] **Step 4: Run the new tests on both backends**

Expected: both pass. Report the SQL PostgreSQL actually ran without complaint — a correlated
subquery inside an `UPDATE ... WHERE` naming the same table being updated is legal SQL but
worth confirming rather than assuming, on a backend this exact shape has not run on before.

- [ ] **Step 5: Run the whole suite on both backends**

Expected: unchanged from Task 2's counts plus the two new tests. Every existing cascade test
(activities/attachments/reminders on an object with no children) must still pass exactly as
before — this task must not change behaviour for the overwhelmingly common case of an object
with no descendants.

- [ ] **Step 6: Commit**

```bash
git add src/sync/record.rs tests/objects.rs
git commit -m "fix: deleting an object deletes what is inside it, recursively"
```

---

### Task 4: The two doors — REST and sync — refuse a bad parent identically

**Files:**
- Modify: `src/sync/mod.rs` (whitelist, `INTEGER_COLUMNS`)
- Modify: `src/sync/apply.rs` (the `Set` arm's foreign-key validation block)
- Modify: `src/api/objects.rs` (`ObjectRow`, `ObjectInput`, `load_owned_object`, `list`,
  `create`, `update`, `read`, `with_stats`/`ancestors`)
- Test: `tests/objects.rs`, `tests/sync.rs`

**Interfaces:**
- Consumes: `record::parent_is_valid`, `record::id_of` from Task 2.
- Produces: `ObjectRow.parent_id: Option<i64>`; `ObjectInput.parent_id: Option<Option<i64>>`
  (the same three-state shape `cover_attachment_id` already uses); `ListQuery.parent_id: Option<i64>`
  and `ListQuery.all: bool`; `ObjectOut` (or wherever the single-object read assembles its
  response) gains `ancestors: Vec<(i64, String)>` — id and name pairs, root first, excluding the
  object itself.

This is the largest task in the plan. Do it in the order below; each half is independently
testable before the next begins.

- [ ] **Step 1: Write the failing sync test**

In `tests/sync.rs`, follow the file's existing style for a rejected `Set` op (there are already
tests rejecting a bad `cover_attachment_id` or `done_activity_id` — copy that shape exactly):

```rust
/// The sync door must refuse a cycle exactly as the REST door does -- proving the two doors
/// agree, not just that each one independently rejects something.
#[tokio::test]
async fn a_sync_push_cannot_create_a_cycle() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let house_uuid = client_uuid(&app.state.db, "objects", house["id"].as_i64().unwrap()).await;
    let garage_uuid = client_uuid(&app.state.db, "objects", garage["id"].as_i64().unwrap()).await;

    // Garage becomes House's child through the ordinary REST path first.
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    // Now a device tries to push the reverse: House becomes Garage's child.
    let res = app.client.post(app.url("/sync/push")).json(&push_body(json!([
        { "client_op_id": "op-cycle", "entity": "object", "entity_uuid": house_uuid,
          "op": "set", "field": "parent_id", "value": garage_json_id(&app, &garage_uuid).await,
          "edited_at": after_now(60), "device_id": "phone" }
    ]))).await;
    assert_eq!(res.status(), 200, "the batch must not 500");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["results"][0]["outcome"], "rejected", "a cycle must be rejected, not applied");
}
```

Check `client_uuid`'s exact helper signature and the `push_body`/`after_now`/`json` helpers
already used elsewhere in `tests/sync.rs`, and match them precisely — do not invent new
signatures. If there is no existing helper to look up an object's real integer id by uuid for
binding into a sync op's `value`, add a small one following the file's existing style (a sync op
value referencing another object is always the real integer id, the same convention
`cover_attachment_id` already uses — see `src/sync/apply.rs`'s existing FK-validation block for
why).

- [ ] **Step 2: Write the failing REST tests**

Append to `tests/objects.rs`:

```rust
#[tokio::test]
async fn creating_an_object_with_a_valid_parent_succeeds() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let res = app.client.post(app.url("/objects"))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201);
    let garage: serde_json::Value = res.json().await.unwrap();
    assert_eq!(garage["parent_id"], house["id"]);
}

#[tokio::test]
async fn reparenting_onto_a_descendant_is_refused_with_a_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    let res = app.client.patch(app.url(&format!("/objects/{}", house["id"])))
        .json(&serde_json::json!({ "name": "House", "type": "home", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": garage["id"] }))
        .send().await.unwrap();
    assert_eq!(res.status(), 400);
}

/// Roots-only by default is the dashboard's contract, and it must hold for the overwhelmingly
/// common case of an installation with no hierarchy at all.
#[tokio::test]
async fn listing_objects_with_no_parent_id_query_returns_only_roots() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    let list: Vec<serde_json::Value> = app.client.get(app.url("/objects?archived=false"))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<String> = list.iter().map(|o| o["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, vec!["House"], "the default list must exclude Garage, which has a parent");
}

#[tokio::test]
async fn listing_a_specific_parents_children_returns_only_those() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let bike = app.create_object(&app.client, "Bike", None).await; // stays a root
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    let list: Vec<serde_json::Value> = app.client
        .get(app.url(&format!("/objects?archived=false&parent_id={}", house["id"])))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<String> = list.iter().map(|o| o["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, vec!["Garage"]);
    let _ = bike; // present in the account, absent from this query -- the point being tested
}

#[tokio::test]
async fn reading_an_object_reports_its_ancestor_chain() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();
    app.client.patch(app.url(&format!("/objects/{}", light["id"])))
        .json(&serde_json::json!({ "name": "Main light", "type": "other", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": garage["id"] }))
        .send().await.unwrap();

    let read: serde_json::Value = app.client.get(app.url(&format!("/objects/{}", light["id"])))
        .send().await.unwrap().json().await.unwrap();
    let names: Vec<&str> = read["ancestors"].as_array().unwrap().iter()
        .map(|a| a["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["House", "Garage"], "root first, nearest ancestor last, self excluded");
}
```

- [ ] **Step 3: Run everything from Steps 1-2 and watch it fail**

Expected: compilation failures (the fields do not exist yet) or 400s/wrong-shape responses.
Record what you actually see.

- [ ] **Step 4: Wire the whitelist**

In `src/sync/mod.rs`, add to `Entity::Object`'s whitelist entry:

```rust
("parent_id", Integer),
```

and to `INTEGER_COLUMNS`:

```rust
(Entity::Object, "parent_id"),
```

Run `cargo test --test mod` (or wherever `sync::mod`'s unit tests live — they are `#[cfg(test)]`
inside `src/sync/mod.rs` itself, so `cargo test the_whitelist` will find them) and confirm
`every_whitelisted_field_carries_its_schema_type` and the other two whitelist tests pass.

- [ ] **Step 5: Wire sync's `Set` handling**

In `src/sync/apply.rs`, inside the existing FK-validation block (the `match (op.entity, field)`
that already has arms for `cover_attachment_id` and `done_activity_id`), add:

```rust
(Entity::Object, "parent_id") => {
    let object_id = record::id_of(&mut *tx, Entity::Object, &op.entity_uuid).await?;
    if record::parent_is_valid(&mut *tx, user_id, Some(object_id), *referenced).await? {
        Some(*referenced)
    } else {
        None
    }
}
```

This must sit inside the same `if let Binding::Integer(referenced) = &bound { match (op.entity, field) { ... } }`
block that already exists — open `src/sync/apply.rs` and read that block's exact current shape
directly (it already has arms for `(Entity::Object, "cover_attachment_id")` and
`(Entity::Reminder, "done_activity_id")`) and add this arm alongside them, in the same style,
rather than writing a new, separate `if`.

- [ ] **Step 6: Add `parent_id` to `ObjectRow` and `ObjectInput`**

In `src/api/objects.rs`:

```rust
pub struct ObjectRow {
    // ... existing fields ...
    pub parent_id: Option<i64>,
}
```

```rust
pub struct ObjectInput {
    // ... existing fields ...
    #[serde(default, deserialize_with = "double_option")]
    pub parent_id: Option<Option<i64>>,
}
```

Add `parent_id` to every SQL column list that currently selects `ObjectRow`'s columns:
`load_owned_object`, `list`'s `SELECT`, `create`'s `INSERT ... RETURNING`, `update`'s final
reload, and `search.rs`'s object query (Task 8 gives that one its own, larger change — for now
just keep it compiling by adding the column to its `SELECT` list unchanged otherwise).

- [ ] **Step 7: Validate on create**

In `create`, before the `INSERT`:

```rust
let parent_id = body.parent_id.flatten();
if let Some(pid) = parent_id {
    if !record::parent_is_valid(&mut *tx, user.id, None, pid).await? {
        return Err(AppError::BadRequest("parent_id must be your own, undeleted, and not itself".into()));
    }
}
```

`Option<Option<T>>::flatten()` collapses "absent" and "explicit null" to the same `None` here,
which is correct on create: there is no existing value to distinguish "keep" from "clear" the
way there is on update. Add `parent_id` to the `INSERT`'s column list, `VALUES` placeholders,
and `RETURNING` list, binding `parent_id`.

- [ ] **Step 8: Validate on update**

In `update`, alongside the existing `cover_attachment_id` three-state handling:

```rust
let parent_id = match body.parent_id {
    None => existing.parent_id,
    Some(None) => None,
    Some(Some(pid)) => {
        // `parent_is_valid` takes `&mut sqlx::AnyConnection`, matching every other helper in
        // `record.rs` -- there is no open transaction yet at this point in `update` (the
        // existing `cover_attachment_id` check runs before `db::begin_write` too), so a
        // connection is acquired from the pool just for this check. `&mut conn` coerces to
        // `&mut AnyConnection` the same way `&mut app.state.db.acquire().await.unwrap()`
        // already does in this task's own tests.
        let mut conn = state.db.acquire().await?;
        if !record::parent_is_valid(&mut conn, user.id, Some(id), pid).await? {
            return Err(AppError::BadRequest("parent_id must be your own, undeleted, not itself, and not a descendant".into()));
        }
        Some(pid)
    }
};
```

Add `parent_id` to the `changed` diff list
(`if parent_id != existing.parent_id { changed.push(("parent_id", json!(parent_id))); }`) and to
the `UPDATE`'s column list and binds.

- [ ] **Step 9: `list` gains `parent_id` and `all`**

```rust
pub struct ListQuery {
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub all: bool,
}
```

`list`'s current SQL binds exactly one placeholder today (`$1 = user.id`); `{archived}` and
`{order}` are spliced fragments from a closed Rust match, not bound parameters — read the
current function before adding to it, so the new placeholders are numbered correctly rather
than guessed. Change the `WHERE` clause to add, alongside the existing `archived_at {archived}`
fragment:

```sql
AND ($3 OR ($2 IS NULL AND parent_id IS NULL) OR (parent_id = $2))
```

with the full statement's binds becoming `.bind(user.id).bind(q.parent_id).bind(q.all)` in that
order, `$2 = q.parent_id`, `$3 = q.all`. When `all` is true every one of the user's own objects
is returned regardless of nesting (subject to the `archived` filter, as today).

- [ ] **Step 10: `ancestors` on a single-object read**

Add a small helper near `with_stats`:

```rust
/// The object's ancestor chain, root first, excluding the object itself -- empty when it has
/// no parent, which costs nothing extra: the common case (an object with no parent) never
/// reaches the recursive query at all.
async fn ancestors(state: &App, object: &ObjectRow) -> Result<Vec<(i64, String)>, AppError> {
    let Some(_) = object.parent_id else { return Ok(Vec::new()) };
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(
        "WITH RECURSIVE chain(id, name, parent_id, depth) AS ( \
           SELECT id, name, parent_id, 0 FROM objects WHERE id = $1 \
           UNION ALL \
           SELECT o.id, o.name, o.parent_id, c.depth + 1 FROM objects o \
             JOIN chain c ON o.id = c.parent_id \
         ) SELECT id, name, depth FROM chain WHERE id != $1 ORDER BY depth DESC")
        .bind(object.id)
        .fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(|(id, name, _)| (id, name)).collect())
}
```

Wire this into whatever response type the single-object `read` handler and `create`/`update`
return (`ObjectOut` or its equivalent) as an `ancestors` field, serialized as an array of
`{ "id": ..., "name": ... }` objects — check exactly how the existing `#[derive(Serialize)]`
response struct is shaped and add the field the same way `cover_file_id` was added to it.

- [ ] **Step 11: Run everything from Steps 1-2 again**

Expected: all pass, on both backends.

- [ ] **Step 12: Run the whole suite on both backends**

Expected: unchanged from Task 3's counts plus the tests added in this task. Pay particular
attention to any existing test that calls `GET /objects` and asserts on the full list — the
new roots-only default may need an existing test's fixture updated if it relied on every
created object appearing in the plain list. Report any such test you had to touch and why.

- [ ] **Step 13: Commit**

```bash
git add src/sync/mod.rs src/sync/apply.rs src/api/objects.rs tests/objects.rs tests/sync.rs
git commit -m "feat: objects can be nested, validated identically on both doors"
```

---

### Task 5: The purge must not orphan a tree it cannot finish clearing

**Files:**
- Modify: `src/sync/feed.rs`
- Test: wherever this project's purge tests live (check `tests/sync.rs` or a dedicated file —
  the module doc comment on the purge function in `feed.rs` will say)

**Interfaces:**
- Consumes: `objects.parent_id` from Task 1.

- [ ] **Step 1: Read the existing purge guard first**

Read the `NOT EXISTS` guard list in `src/sync/feed.rs` around the `objects` entry (it currently
guards against live activities, reminders, and attachments) before changing anything — this
plan's spec quotes it, but the exact line numbers will have shifted since Task 1's migration.

- [ ] **Step 2: Write the failing test**

```rust
/// The purge must not hard-delete a tombstoned parent while any object -- live or itself
/// tombstoned but not yet purged -- still names it as `parent_id`. Purging the parent first
/// would either violate the foreign key outright, or, if it somehow didn't, leave the child's
/// `parent_id` pointing at a row that no longer exists.
#[tokio::test]
async fn a_tombstoned_parent_is_not_purged_while_a_tombstoned_child_still_references_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light", None).await;
    let garage_id = garage["id"].as_i64().unwrap();

    let mut conn = app.state.db.acquire().await.unwrap();
    sqlx::query("UPDATE objects SET parent_id = $1 WHERE id = $2").bind(garage_id).bind(light["id"])
        .execute(&mut *conn).await.unwrap();
    drop(conn);

    app.client.delete(app.url(&format!("/objects/{garage_id}"))).send().await.unwrap();
    // Backdate both tombstones past the retention window directly, the way this project's
    // existing purge tests already do -- follow that exact pattern rather than waiting real days.
    app.age_out_tombstones().await;

    app.run_purge().await;

    let still_there: i64 = sqlx::query_scalar("SELECT count(*) FROM objects WHERE id = $1")
        .bind(garage_id).fetch_one(&app.state.db).await.unwrap();
    assert_eq!(still_there, 1, "the parent must survive the purge while its child still references it");
}
```

Check `age_out_tombstones` and `run_purge`'s exact names in the harness (they were added for an
earlier purge-race test on this project — search `tests/common/mod.rs` for them) and match
them precisely.

- [ ] **Step 3: Run it and watch it fail**

Expected: fails, or the harness has no such helpers yet — if the exact helper names above do
not exist, find whatever the existing purge tests actually call and use that instead of
inventing new names.

- [ ] **Step 4: Extend the guard**

Add a fourth `NOT EXISTS` clause to the `objects` guard in `feed.rs`, alongside the three that
already check activities, reminders and attachments:

```sql
AND NOT EXISTS (SELECT 1 FROM objects c WHERE c.parent_id = objects.id)
```

Deliberately with no `deleted_at` condition on `c` — a *live* child referencing this row would
already be impossible (the delete cascade in Task 3 tombstones every descendant before this
parent could itself age into purge eligibility), but a tombstoned child not yet old enough to be
purged itself must also hold the parent back, or the parent could be purged first and leave the
child's `parent_id` dangling.

- [ ] **Step 5: Run the new test, then the whole suite, on both backends**

Expected: the new test passes; nothing else regresses from Task 4's counts.

- [ ] **Step 6: Commit**

```bash
git add src/sync/feed.rs tests/
git commit -m "fix: the purge waits for a parent's children to be purged first"
```

---

### Task 6: Export drops it; import never sets it

**Files:**
- Modify: `src/api/export.rs`
- Test: `tests/export.rs`

**Interfaces:** none shared with other tasks.

- [ ] **Step 1: Write the failing test**

```rust
/// `/export` is scoped to one object and its own activities, attachments and reminders -- a
/// parent living outside the exported object is never in the archive. Carrying `parent_id`
/// across an export would point at nothing on the far side, or at whatever unrelated row an
/// id-reassigning import happens to give a completely different object. An imported object
/// must always land with no parent, regardless of what it had.
#[tokio::test]
async fn an_exported_objects_parent_is_dropped_and_reimports_with_none() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = app.create_object(&app.client, "House", None).await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    app.client.patch(app.url(&format!("/objects/{}", garage["id"])))
        .json(&serde_json::json!({ "name": "Garage", "type": "car", "counter_unit": null,
            "description": "", "purchase_date": null, "purchase_price_cents": null,
            "parent_id": house["id"] }))
        .send().await.unwrap();

    let res = app.client.get(app.url(&format!("/export?object_id={}", garage["id"])))
        .send().await.unwrap();
    let zip_bytes = res.bytes().await.unwrap().to_vec();
    let data_json = /* extract data.json from the zip the way the existing export tests already do */;
    assert!(!data_json.to_string().contains("parent_id"), "parent_id must not appear in the archive at all");

    let anna = app.create_user_client("anna", "password123").await;
    let import_res = anna.post(app.url("/import")).header("content-type", "application/zip")
        .body(zip_bytes).send().await.unwrap();
    assert_eq!(import_res.status(), 200);
    let imported: Vec<serde_json::Value> = anna.get(app.url("/objects?all=true")).send().await.unwrap().json().await.unwrap();
    assert_eq!(imported[0]["parent_id"], serde_json::Value::Null);
}
```

Follow the existing export test file's exact pattern for extracting `data.json` from the
returned zip bytes — this project's export tests already do this; copy that approach rather
than writing a new one.

- [ ] **Step 2: Run it and watch it fail**

Expected: fails to compile (parent_id was added to `ObjectRow` in Task 4, so it now
automatically flows into whatever struct `export.rs` reuses for the archive — if `export.rs`
serializes `ObjectRow` directly rather than its own dedicated export type, this is exactly the
leak this task exists to close) or the assertion fails if it does compile.

- [ ] **Step 3: Exclude it**

If `export.rs` has its own dedicated struct for what goes into `data.json` (check first — this
project generally prefers a dedicated export/import shape over reusing the live API's row type
directly, given how export already omits fields like `user_id`), simply do not add `parent_id`
to it. If it currently derives the exported shape from `ObjectRow` in a way that would leak the
new field, add an explicit exclusion the same way any other internal-only field is already kept
out.

- [ ] **Step 4: Run the test, then the whole suite, on both backends**

Expected: passes; nothing else regresses.

- [ ] **Step 5: Commit**

```bash
git add src/api/export.rs tests/export.rs
git commit -m "fix: an object's parent does not survive export and import"
```

---

### Task 7: Search shows where a match lives

**Files:**
- Modify: `src/api/search.rs`
- Test: `tests/search.rs` (or wherever search's Rust tests live)

**Interfaces:**
- Produces: `ObjectHit` (new struct, mirroring the existing `ActivityHit`), replacing
  `SearchResults.objects: Vec<ObjectRow>` with `Vec<ObjectHit>`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn a_search_hit_names_its_parent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let garage = app.create_object(&app.client, "Garage", None).await;
    let light = app.create_object(&app.client, "Main light unique term", None).await;
    app.client.patch(app.url(&format!("/objects/{}", light["id"])))
        .json(&serde_json::json!({ "name": "Main light unique term", "type": "other",
            "counter_unit": null, "description": "", "purchase_date": null,
            "purchase_price_cents": null, "parent_id": garage["id"] }))
        .send().await.unwrap();

    let res: serde_json::Value = app.client.get(app.url("/search?q=unique+term"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(res["objects"][0]["parent_name"], "Garage");
}

#[tokio::test]
async fn a_root_objects_search_hit_has_no_parent_name() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Solo object distinctive", None).await;
    let res: serde_json::Value = app.client.get(app.url("/search?q=distinctive"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(res["objects"][0]["parent_name"], serde_json::Value::Null);
}
```

- [ ] **Step 2: Run and watch it fail**

Expected: `parent_name` does not exist on the response.

- [ ] **Step 3: Add `ObjectHit`**

```rust
#[derive(Serialize, sqlx::FromRow)]
pub struct ObjectHit {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub type_: String,
    pub counter_unit: Option<String>,
    pub fuel_unit: Option<String>,
    pub description: String,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    pub archived_at: Option<String>,
    pub cover_attachment_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub parent_name: Option<String>,
}
```

Change `SearchResults.objects: Vec<ObjectRow>` to `Vec<ObjectHit>`. Change the object query's
`SELECT` to a `LEFT JOIN` against `objects` again for the parent's name:

```sql
SELECT o.id, o.user_id, o.name, o.type, o.counter_unit, o.fuel_unit, o.description, \
       o.purchase_date, o.purchase_price_cents, o.archived_at, o.cover_attachment_id, \
       o.parent_id, o.created_at, o.updated_at, p.name AS parent_name \
FROM objects o LEFT JOIN objects p ON p.id = o.parent_id \
WHERE o.user_id = $1 AND o.deleted_at IS NULL AND ( \
  o.name {like} $2 ESCAPE '\\' OR o.description {like} $2 ESCAPE '\\') \
ORDER BY o.archived_at IS NOT NULL, {order} LIMIT $3
```

Do not filter on `parent_id` here — search must keep finding nested objects exactly as it finds
root ones, which the module's own existing doc comment already establishes as this endpoint's
contract.

- [ ] **Step 4: Update the frontend type**

In `frontend/src/lib/types.ts`, add `parent_name: string | null` to whatever type represents a
search object hit (check whether `SearchResults` currently reuses `MemObject` or has its own —
follow Task 4's frontend change, which is where `MemObject` itself gains `parent_id`).

- [ ] **Step 5: Show it in the search UI**

In `frontend/src/routes/Search.svelte`, wherever an object hit is rendered, show
`{hit.parent_name}` when present — following the existing pattern the file already uses for an
object's type row (`{$t('search.in-parent', { name: hit.parent_name })}` or similar; add the
i18n key in both languages).

- [ ] **Step 6: Run everything on both backends, and the frontend suite**

Expected: backend tests pass on both; `npm run check`, vitest, and Playwright unaffected.

- [ ] **Step 7: Commit**

```bash
git add src/api/search.rs frontend/src/lib/types.ts frontend/src/routes/Search.svelte frontend/src/i18n tests/
git commit -m "feat: a search hit inside a room says which room"
```

---

### Task 8: The parent picker cannot offer a doomed choice

**Files:**
- Create: `frontend/src/lib/object-tree.ts`
- Create: `frontend/tests/object-tree.test.ts`
- Modify: `frontend/src/lib/types.ts`, `frontend/src/lib/object-form.ts`,
  `frontend/src/routes/ObjectForm.svelte`

**Interfaces:**
- Produces: `pub function excludingDescendants(objects: MemObject[], selfId: number | null): MemObject[]` —
  given every object the user owns (fetched with `all=true`) and the id of the object currently
  being edited (`null` when creating), returns the objects that are legal parent choices:
  everything except the object itself and anything reachable by following `parent_id` down from
  it.

- [ ] **Step 1: Write the failing unit tests**

Create `frontend/tests/object-tree.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { excludingDescendants } from '../src/lib/object-tree';
import type { MemObject } from '../src/lib/types';

function obj(id: number, parent_id: number | null): MemObject {
  return { id, user_id: 1, name: `#${id}`, type: 'other', counter_unit: null, fuel_unit: null,
    description: '', purchase_date: null, purchase_price_cents: null, archived_at: null,
    cover_attachment_id: null, cover_file_id: null, parent_id, created_at: '', updated_at: '',
    stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0 } };
}

describe('excludingDescendants', () => {
  it('excludes nothing when creating a brand-new object', () => {
    const all = [obj(1, null), obj(2, 1)];
    expect(excludingDescendants(all, null).map((o) => o.id)).toEqual([1, 2]);
  });

  it('excludes the object itself', () => {
    const all = [obj(1, null), obj(2, 1)];
    expect(excludingDescendants(all, 1).map((o) => o.id)).toEqual([2]);
  });

  it('excludes a direct child, so a room cannot be filed inside its own fixture', () => {
    const all = [obj(1, null), obj(2, 1)];
    expect(excludingDescendants(all, 1).some((o) => o.id === 2)).toBe(false);
  });

  it('excludes a grandchild, three levels deep', () => {
    const all = [obj(1, null), obj(2, 1), obj(3, 2)];
    expect(excludingDescendants(all, 1).map((o) => o.id)).toEqual([]);
  });

  it('does not exclude an unrelated object, even one that is also a leaf', () => {
    const all = [obj(1, null), obj(2, 1), obj(3, null)];
    expect(excludingDescendants(all, 1).map((o) => o.id)).toEqual([3]);
  });
});
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npx vitest run tests/object-tree.test.ts`

Expected: `object-tree` module not found.

- [ ] **Step 3: Write the pure function**

```ts
import type { MemObject } from './types';

/** Every object in `all` that may legally become `selfId`'s parent: not the object itself, and
 *  not anything reachable by following `parent_id` down from it -- which the server would
 *  refuse anyway as a cycle, but offering it at all would be a choice that is always wrong.
 *  `selfId` is `null` when creating a brand-new object, which cannot yet be anyone's ancestor. */
export function excludingDescendants(all: MemObject[], selfId: number | null): MemObject[] {
  if (selfId === null) return all;
  const childrenOf = new Map<number, number[]>();
  for (const o of all) {
    if (o.parent_id === null) continue;
    childrenOf.set(o.parent_id, [...(childrenOf.get(o.parent_id) ?? []), o.id]);
  }
  const excluded = new Set<number>([selfId]);
  const stack = [selfId];
  while (stack.length > 0) {
    const id = stack.pop()!;
    for (const child of childrenOf.get(id) ?? []) {
      if (!excluded.has(child)) { excluded.add(child); stack.push(child); }
    }
  }
  return all.filter((o) => !excluded.has(o.id));
}
```

- [ ] **Step 4: Run the tests**

Expected: all pass.

- [ ] **Step 5: Add `parent_id` to the frontend types**

In `frontend/src/lib/types.ts`, add `parent_id: number | null` to `MemObject` and
`parent_id?: number | null` to `ObjectInput`. Add `ancestors: { id: number; name: string }[]` to
`MemObject` (Task 4's REST change puts it on every single-object read; list responses can leave
it as an empty array, which the type should allow without special-casing).

- [ ] **Step 6: Wire the picker into the form**

In `frontend/src/lib/object-form.ts`, add `parent_id: number | null` to `emptyInput()` (defaulting
to `null`) and to `toInput()` (copying `o.parent_id`).

In `frontend/src/routes/ObjectForm.svelte`, fetch every object the user owns
(`api<MemObject[]>('GET', '/objects?all=true')`) once on mount, filter it with
`excludingDescendants(all, editing ? Number(id) : null)`, and render a `<select>`:

```svelte
<div class="field">
  <label for="p">{$t('object.parent')}</label>
  <select id="p" bind:value={input.parent_id}>
    <option value={null}>{$t('object.parent-none')}</option>
    {#each parentChoices as p}<option value={p.id}>{p.name}</option>{/each}
  </select>
</div>
```

Add `object.parent` / `object.parent-none` to both `en.ts` and `de.ts`.

- [ ] **Step 7: Run the frontend suite**

Run: `cd frontend && npm run check && npm run build && npx vitest run && npx playwright test`

Expected: `npm run check` 0 errors; vitest count up by 5 (the new file); Playwright unaffected —
this task adds a field to the form, not a new required one, so nothing existing should break.

- [ ] **Step 8: Look at it**

Screenshot the object form's new parent field in both themes, for a fresh object and for one
being edited that already has descendants (confirm the descendants do not appear as choices).
Report what you see.

- [ ] **Step 9: Commit**

```bash
git add frontend/src/lib/object-tree.ts frontend/tests/object-tree.test.ts frontend/src/lib/types.ts frontend/src/lib/object-form.ts frontend/src/routes/ObjectForm.svelte frontend/src/i18n
git commit -m "feat: the parent picker never offers a choice the server would refuse"
```

---

### Task 9: The breadcrumb and the Contents section

**Files:**
- Modify: `frontend/src/routes/ObjectDetail.svelte`
- Test: `frontend/tests-e2e/12-object-hierarchy.spec.ts` (new)

**Interfaces:**
- Consumes: `MemObject.ancestors` from Task 8; `GET /objects?parent_id=X` from Task 4.

- [ ] **Step 1: Write the failing end-to-end test**

Create `frontend/tests-e2e/12-object-hierarchy.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('a house shows its rooms, and a room shows its breadcrumb', async ({ page }) => {
  await signIn(page);

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Hierarchy House');
  await page.getByLabel('Type').selectOption('home');
  await page.getByRole('button', { name: 'Save' }).click();

  await page.getByRole('button', { name: /New object/ }).click();
  await page.getByLabel('Name').fill('Hierarchy Garage');
  await page.getByLabel('Type').selectOption('other');
  await page.getByLabel('Parent').selectOption({ label: 'Hierarchy House' });
  await page.getByRole('button', { name: 'Save' }).click();

  // The garage's own page shows the breadcrumb back to the house.
  await expect(page.getByText('Hierarchy House')).toBeVisible();

  // The house's page lists the garage in its contents.
  await page.goto('/');
  await page.getByText('Hierarchy House').click();
  await page.getByRole('button', { name: 'Info' }).click();
  await expect(page.getByText('Hierarchy Garage')).toBeVisible();
});
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd frontend && npm run build && npx playwright test 12-object-hierarchy`

Expected: fails — nothing renders a breadcrumb or a contents list yet.

- [ ] **Step 3: Add the breadcrumb**

In `frontend/src/routes/ObjectDetail.svelte`, immediately after the `<TopBar>` block, before the
cover image:

```svelte
{#if object.ancestors.length > 0}
  <p class="breadcrumb">
    {#each object.ancestors as a, i}
      <a href={`/objects/${a.id}`} onclick={(e) => { e.preventDefault(); go(`/objects/${a.id}`); }}>{a.name}</a>
      {#if i < object.ancestors.length - 1} › {/if}
    {/each}
  </p>
{/if}
```

Add a `.breadcrumb` rule using the existing muted-text and spacing tokens, matching how `.desc`
or `.hint` are already styled in this file's `<style>` block — do not invent a new visual
weight for this.

- [ ] **Step 4: Add the Contents section**

In the `info` tab branch, fetch the object's direct children
(`api<MemObject[]>('GET', `/objects?parent_id=${oid}&archived=false`)`) and render them,
reusing `ObjectCard` — the same component the dashboard already uses, so a child inside a room
looks exactly like an object anywhere else in the app:

```svelte
<h3>{$t('object.contents')}</h3>
{#if children.length === 0}
  <p class="muted">{$t('object.contents-empty')}</p>
{:else}
  <div class="list">
    {#each children as c (c.id)}<ObjectCard object={c} />{/each}
  </div>
{/if}
<button class="ghost" onclick={() => go(`/objects/new?parent_id=${oid}`)}>+ {$t('object.contents-add')}</button>
```

`ObjectForm.svelte` needs to read a `parent_id` query parameter on the `new` route and pre-fill
`input.parent_id` with it. This project has no dedicated query-string helper; `ObjectDetail.svelte`
reads its own `tab` parameter directly (`new URLSearchParams(location.search).get('tab')`) —
follow that exact idiom:

```ts
const presetParentId = new URLSearchParams(location.search).get('parent_id');
```

and, in `emptyInput()`'s call site (only when `!editing`), set
`input.parent_id = presetParentId ? Number(presetParentId) : null` before the form first renders.

Add `object.contents`, `object.contents-empty`, `object.contents-add` to both `en.ts` and
`de.ts`. The German strings should read as German, matching the du-form used throughout the
rest of `de.ts`.

- [ ] **Step 5: Run everything**

Run: `cd frontend && npm run check && npm run build && npx playwright test`, and both backend
suites for good measure even though this task touches no Rust.

Expected: 0 type errors; Playwright count up by 1; nothing else regresses.

- [ ] **Step 6: Look at it**

Screenshot House's Info tab (showing Garage in its contents) and Garage's page (showing the
breadcrumb back to House), in both themes. Report what you see.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/routes/ObjectDetail.svelte frontend/src/routes/ObjectForm.svelte frontend/src/i18n frontend/tests-e2e/12-object-hierarchy.spec.ts
git commit -m "feat: show a house's rooms, and a room's way back up"
```

---

### Task 10: Close the series

**Files:**
- Modify: `docs/superpowers/specs/2026-09-13-object-hierarchy-design.md`
- Modify: `README.md`, if this feature needs any operator-facing note (it should not — nothing
  about deployment, backup, or the database configuration changes)

- [ ] **Step 1: Confirm nothing operator-facing changed**

Read through `README.md`'s existing sections and confirm none of them describe object structure
in a way this feature contradicts. If truly nothing needs to change, say so in the commit
message rather than padding the README with a feature note nobody deploying the app needs.

- [ ] **Step 2: Mark the spec implemented**

Change the spec's status line to `Status: implemented.`

- [ ] **Step 3: Run the whole suite one last time, both backends, plus frontend**

Record the final counts against Task 1's baseline and report the difference plainly.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-09-13-object-hierarchy-design.md
git commit -m "docs: objects can contain other objects"
```
