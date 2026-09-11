# PostgreSQL part 1: one code path, two databases — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make LogB able to run on either SQLite or PostgreSQL, chosen by a URL, with the whole
test suite passing on both.

**Architecture:** One code path over `sqlx::any::AnyPool`. Statements are written once, in a
portable subset, using `$1` placeholders — which SQLite accepts natively, so no rewriting is
needed. SQLite's pragmas, which cannot be expressed in a URL, are restored by an after-connect
hook. PostgreSQL gets one consolidated schema rather than a replay of nine migrations.

**Tech Stack:** Rust, axum, sqlx 0.9 (`sqlite` + `postgres` + `any` + `migrate`), PostgreSQL 16
in CI and for local test runs.

## What was proven before this plan was written

Do not re-litigate these; they were tested in this repository, not assumed:

- **`$1` binds on SQLite through sqlx.** `INSERT INTO t (id, name) VALUES ($1, $2)` with two
  `.bind()` calls works. So one placeholder style serves both backends and there is no rewrite
  layer to build.
- **`AnyPool` carries this app's row shapes**: `#[derive(sqlx::FromRow)]` structs of `i64`,
  `String`, `Option<i64>`, `INSERT … RETURNING`, `query_scalar` counts, `AUTOINCREMENT`.
- **sqlx 0.9's SQLite URL parser accepts only `mode`, `cache`, `immutable` and `vfs`**
  (`sqlx-sqlite-0.9.0/src/options/parse.rs:44-103`). `foreign_keys`, `journal_mode` and
  `busy_timeout` **cannot** be set in a URL, and `AnyPool` connects by URL only.
- **An after-connect hook restores them**, and a cascade then fires — verified by creating
  parent and child tables, deleting the parent, and finding the child gone.

That last pair is the danger in this task: losing `foreign_keys` is silent. Nothing fails, no
test complains, and `ON DELETE CASCADE` simply stops happening — which in this schema means
orphaned attachments and reminders pointing at deleted objects.

## Global Constraints

- No behaviour change on SQLite. All 290 backend tests, 168 vitest and 22 Playwright tests pass
  unchanged at every task boundary.
- No new dependency beyond enabling sqlx features already in the tree.
- Timestamps stay TEXT holding ISO-8601; money stays INTEGER cents; booleans stay 0/1. This is a
  port, not a redesign.
- Every statement uses `$1`-style placeholders on both backends.
- `cargo clippy --all-targets -- -D warnings` stays clean.

## File structure

| File | Responsibility |
|---|---|
| `src/db.rs` | Connecting, the after-connect pragmas, choosing the migrator |
| `src/dialect.rs` (new) | The handful of statements that cannot be written once |
| `migrations/sqlite/` | The nine existing files, moved unchanged |
| `migrations/postgres/0001_schema.sql` (new) | One consolidated schema |
| `tests/common/mod.rs` | Spawns a test app against whichever backend is configured |
| `.github/workflows/ci.yml` | A second backend job |

---

### Task 1: The pool becomes `AnyPool`

**Files:**
- Modify: `Cargo.toml:13`, `src/db.rs:1-45`, `src/state.rs:2,9`, `src/main.rs`
- Modify: `src/backup.rs`, `src/restore.rs`, `src/sync/{apply,epoch,feed,record}.rs`,
  `src/api/{export,reminders}.rs` — these name `SqlitePool` in signatures
- Test: `tests/pool.rs` (new)

**Interfaces:**
- Produces: `db::connect(url: &str) -> Result<AnyPool, BoxError>` and
  `db::connect_existing(url: &str) -> Result<AnyPool, BoxError>`; `AppState.db: AnyPool`.

- [ ] **Step 1: Write the failing test**

Create `tests/pool.rs`:

```rust
//! The pragmas SQLite needs cannot be expressed in a connection URL, and `AnyPool` connects by
//! URL only. They are restored by an after-connect hook instead -- and losing them is silent:
//! nothing errors, `ON DELETE CASCADE` simply stops happening, and attachments outlive the
//! objects they belong to.

#[tokio::test]
async fn every_pooled_connection_enforces_foreign_keys() {
    let dir = tempfile::tempdir().unwrap();
    let pool = logb::db::connect(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display()))
        .await
        .unwrap();

    // Ask several times: the hook has to run for every connection the pool opens, not just the
    // first one.
    for _ in 0..4 {
        let on: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&pool).await.unwrap();
        assert_eq!(on, 1, "foreign keys are off on a pooled connection");
    }
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode").fetch_one(&pool).await.unwrap();
    assert_eq!(mode, "wal");
}

/// The assertion that matters, because the pragma above is only a means to it.
#[tokio::test]
async fn deleting_an_object_still_takes_its_attachments() {
    let dir = tempfile::tempdir().unwrap();
    let pool = logb::db::connect(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display()))
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (id, username, password_hash, created_at) VALUES (1, 'ben', 'x', '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO objects (id, user_id, name, type, description, created_at, updated_at) \
                 VALUES (1, 1, 'Golf', 'car', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO files (id, user_id, sha256, original_name, mime, size, created_at) \
                 VALUES (1, 1, 'abc', 'a.png', 'image/png', 1, '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO attachments (id, object_id, file_id, kind, created_at) \
                 VALUES (1, 1, 1, 'photo', '2026-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();

    sqlx::query("DELETE FROM objects WHERE id = 1").execute(&pool).await.unwrap();

    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM attachments").fetch_one(&pool).await.unwrap();
    assert_eq!(left, 0, "the attachment outlived its object: foreign keys are not enforced");
}
```

`tempfile` is already a dev-dependency — check `Cargo.toml` and add it under
`[dev-dependencies]` only if it is missing.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test --test pool`

Expected: compilation failure — `db::connect` still takes a `&Path` and returns `SqlitePool`.

- [ ] **Step 3: Enable the drivers**

`Cargo.toml:13`:

```toml
sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite", "postgres", "any", "migrate"] }
```

- [ ] **Step 4: Rewrite `src/db.rs`'s connection functions**

```rust
use sqlx::any::{AnyPool, AnyPoolOptions};
use sqlx::Executor;

/// SQLite needs three settings that a connection URL cannot carry: sqlx 0.9's URL parser accepts
/// only `mode`, `cache`, `immutable` and `vfs`. `AnyPool` connects by URL, so they are applied
/// to every connection as it is opened instead.
///
/// `foreign_keys` is the one that matters. Without it nothing fails -- `ON DELETE CASCADE`
/// simply stops happening, and the first sign is an attachment that outlived its object.
/// PostgreSQL enforces foreign keys always and has no equivalent to set.
fn after_connect(url: &str) -> Option<&'static str> {
    url.starts_with("sqlite:")
        .then_some("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000")
}

pub async fn connect(url: &str) -> Result<AnyPool, BoxError> {
    sqlx::any::install_default_drivers();
    let pragmas = after_connect(url);
    let pool = AnyPoolOptions::new()
        .max_connections(if url.starts_with("sqlite:") { 4 } else { 16 })
        .after_connect(move |conn, _meta| {
            Box::pin(async move {
                if let Some(sql) = pragmas {
                    conn.execute(sql).await?;
                }
                Ok(())
            })
        })
        .connect(url)
        .await?;
    migrator(url).run(&pool).await?;
    Ok(pool)
}
```

`migrator(url)` is introduced in Task 3; until then, keep `sqlx::migrate!("./migrations")` here
and leave a comment saying Task 3 replaces it.

`connect_existing` takes the same URL, applies the same hook, and does not run migrations.

The callers in `src/main.rs` build the URL: `LOGB_DATABASE_URL` if set, otherwise
`sqlite://<data_dir>/logb.db?mode=rwc`. Keep `LOGB_DATA_DIR` working exactly as it does — it
still decides where blobs live, and it is still the default database location.

- [ ] **Step 5: Change the types that name the driver**

`src/state.rs:2,9` and the nine other files listed under **Files**: `SqlitePool` becomes
`AnyPool`. These are signature changes only; no query text changes in this task.

- [ ] **Step 6: Run everything**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`

Expected: `tests/pool.rs` passes, and **every other test still passes on SQLite**. If a
`query_as` now fails to compile, the row contains a type `AnyRow` does not carry — report which
one rather than casting around it; that is a finding this plan needs to hear.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src tests/pool.rs
git commit -m "refactor: hold the database through AnyPool, keeping SQLite's pragmas"
```

---

### Task 2: One placeholder style

**Files:**
- Modify: every `sqlx::query*` call site under `src/` (135 of them) and any in `tests/`
- Test: the existing suite is the test

**Interfaces:** none changed.

- [ ] **Step 1: Convert the placeholders**

SQLite accepts `$1`, `$2` … natively (proven above), PostgreSQL requires them. Every `?` in a
statement becomes `$n`, numbered in bind order.

Work file by file, not with a blanket regex: a `?` can appear inside a string literal, and
`LIKE ?` and `ESCAPE '\'` sit next to each other in `src/api/search.rs`. After each file, run
that file's tests.

- [ ] **Step 2: Prove none were missed**

Run: `grep -rn "sqlx::query" -A3 src/ | grep -c "?"`

Expected: `0`. Any remaining `?` inside a statement is either a missed placeholder or a literal
question mark — check each by hand and say which in the task report.

- [ ] **Step 3: Run everything**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`

Expected: all green on SQLite. This task changes no behaviour whatsoever; a failure here is a
mis-numbered bind, and the failing test names the statement.

- [ ] **Step 4: Commit**

```bash
git add src tests
git commit -m "refactor: write every statement with numbered placeholders"
```

---

### Task 3: Two schemas, one migrator

**Files:**
- Move: `migrations/*.sql` → `migrations/sqlite/`
- Create: `migrations/postgres/0001_schema.sql`
- Modify: `src/db.rs` (the `migrator` function referenced in Task 1)
- Test: `tests/schema_parity.rs` (new)

**Interfaces:**
- Produces: `db::migrator(url: &str) -> sqlx::migrate::Migrator`.

- [ ] **Step 1: Write the failing parity test**

Create `tests/schema_parity.rs`:

```rust
//! The two schemas are written by hand, in different dialects, and nothing but this test makes
//! them agree. A column added to one and forgotten in the other does not fail at compile time:
//! it fails at runtime, on whichever instance happens to be running the other backend.

use std::collections::BTreeSet;

async fn columns(url: &str) -> BTreeSet<String> {
    let pool = logb::db::connect(url).await.unwrap();
    let sql = if url.starts_with("sqlite:") {
        "SELECT m.name || '.' || p.name FROM sqlite_master m \
         JOIN pragma_table_info(m.name) p WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' \
         AND m.name <> '_sqlx_migrations'"
    } else {
        "SELECT table_name || '.' || column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'"
    };
    sqlx::query_scalar::<_, String>(sql).fetch_all(&pool).await.unwrap().into_iter().collect()
}

#[tokio::test]
async fn the_two_schemas_describe_the_same_tables_and_columns() {
    let Ok(pg) = std::env::var("LOGB_TEST_DATABASE_URL") else {
        eprintln!("skipped: set LOGB_TEST_DATABASE_URL to a PostgreSQL server to run this");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sqlite = columns(&format!("sqlite://{}/logb.db?mode=rwc", dir.path().display())).await;
    let postgres = columns(&pg).await;

    let only_sqlite: Vec<_> = sqlite.difference(&postgres).collect();
    let only_postgres: Vec<_> = postgres.difference(&sqlite).collect();
    assert!(only_sqlite.is_empty() && only_postgres.is_empty(),
        "schemas disagree.\n  only in SQLite:   {only_sqlite:?}\n  only in PostgreSQL: {only_postgres:?}");
}
```

A skipped test is not a passing test: Task 6 makes CI always provide the URL, so this cannot be
quietly skipped forever.

- [ ] **Step 2: Move the SQLite migrations**

```bash
mkdir -p migrations/sqlite && git mv migrations/*.sql migrations/sqlite/
```

- [ ] **Step 3: Write the PostgreSQL schema**

Create `migrations/postgres/0001_schema.sql`. It describes the schema **as it is today**, not
nine historical steps: a fresh database has no history to replay, and `0009`'s rebuild with
foreign keys disabled has no PostgreSQL equivalent.

```sql
-- The current schema, in one file. The SQLite side keeps its nine migrations because existing
-- databases have to be moved forward step by step; a new PostgreSQL database does not.
--
-- Types are deliberately the same shapes the SQLite side uses: timestamps are TEXT holding
-- ISO-8601, money is integer cents, booleans are 0/1. Converting them to timestamptz and
-- numeric would touch every comparison, every serialisation and every test in the app, and
-- belongs in its own slice once the port is proven.

CREATE TABLE users (
    id            BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    username      TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_admin      SMALLINT NOT NULL DEFAULT 0,
    lang          TEXT NOT NULL DEFAULT 'en',
    created_at    TEXT NOT NULL
);
-- SQLite spells this `UNIQUE COLLATE NOCASE` on the column. PostgreSQL has no per-column
-- collation of that kind without citext, so the case-insensitive uniqueness is an index --
-- and every lookup of a username must compare `lower(username)` to match it (see src/dialect.rs).
CREATE UNIQUE INDEX idx_users_username_lower ON users (lower(username));

CREATE TABLE sessions (
    token      TEXT PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_sessions_expiry ON sessions(expires_at);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE objects (
    id                   BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    user_id              BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    type                 TEXT NOT NULL CHECK (type IN
                           ('car','e_bike','bike','motorcycle','home','appliance','tool','body','other')),
    counter_unit         TEXT CHECK (counter_unit IN ('km', 'mi', 'h')),
    description          TEXT NOT NULL DEFAULT '',
    purchase_date        TEXT,
    purchase_price_cents BIGINT,
    archived_at          TEXT,
    cover_attachment_id  BIGINT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    fuel_unit            TEXT CHECK (fuel_unit IN ('l', 'gal', 'kwh')),
    client_uuid          TEXT,
    deleted_at           TEXT
);
CREATE INDEX idx_objects_user ON objects(user_id);
CREATE UNIQUE INDEX idx_objects_uuid ON objects(client_uuid);

CREATE TABLE activities (
    id             BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    object_id      BIGINT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date           TEXT NOT NULL,
    category       TEXT NOT NULL CHECK (category IN
                     ('maintenance','repair','purchase','inspection','modification','fuel','other',
                      'symptom','treatment','appointment','medication')),
    title          TEXT NOT NULL,
    notes          TEXT NOT NULL DEFAULT '',
    counter_value  BIGINT,
    cost_cents     BIGINT,
    quantity_milli BIGINT,
    client_op_id   TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    client_uuid    TEXT,
    deleted_at     TEXT
);
CREATE INDEX idx_activities_object_date ON activities(object_id, date);
CREATE UNIQUE INDEX idx_activities_client_op ON activities(client_op_id) WHERE client_op_id IS NOT NULL;
CREATE UNIQUE INDEX idx_activities_uuid ON activities(client_uuid);

CREATE TABLE files (
    id            BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    user_id       BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sha256        TEXT NOT NULL,
    original_name TEXT NOT NULL,
    mime          TEXT NOT NULL,
    size          BIGINT NOT NULL,
    width         BIGINT,
    height        BIGINT,
    taken_at      TEXT,
    created_at    TEXT NOT NULL,
    client_uuid   TEXT,
    deleted_at    TEXT,
    UNIQUE (user_id, sha256)
);
CREATE UNIQUE INDEX idx_files_uuid ON files(client_uuid);

CREATE TABLE attachments (
    id           BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    object_id    BIGINT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    activity_id  BIGINT REFERENCES activities(id) ON DELETE CASCADE,
    file_id      BIGINT NOT NULL REFERENCES files(id) ON DELETE RESTRICT,
    kind         TEXT NOT NULL CHECK (kind IN ('photo', 'document')),
    caption      TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL,
    client_op_id TEXT,
    client_uuid  TEXT,
    deleted_at   TEXT
);
CREATE INDEX idx_attachments_object ON attachments(object_id);
CREATE INDEX idx_attachments_activity ON attachments(activity_id);
CREATE INDEX idx_attachments_file ON attachments(file_id);
CREATE UNIQUE INDEX idx_attachments_client_op ON attachments(client_op_id) WHERE client_op_id IS NOT NULL;
CREATE UNIQUE INDEX idx_attachments_uuid ON attachments(client_uuid);

CREATE TABLE reminders (
    id               BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    object_id        BIGINT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    title            TEXT NOT NULL,
    notes            TEXT NOT NULL DEFAULT '',
    due_date         TEXT,
    due_counter      BIGINT,
    repeat_months    BIGINT,
    repeat_counter   BIGINT,
    done_at          TEXT,
    done_activity_id BIGINT REFERENCES activities(id) ON DELETE SET NULL,
    created_at       TEXT NOT NULL,
    snoozed_until    TEXT,
    client_uuid      TEXT,
    deleted_at       TEXT,
    CHECK (due_date IS NOT NULL OR due_counter IS NOT NULL)
);
CREATE INDEX idx_reminders_object ON reminders(object_id);
CREATE UNIQUE INDEX idx_reminders_uuid ON reminders(client_uuid);

CREATE TABLE api_tokens (
    id           BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    user_id      BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    prefix       TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
);

-- `seq` is the pull cursor. On SQLite it is monotonic, gapless and in commit order because
-- SQLite has one writer. PostgreSQL does not give that for free, and an identity column here
-- would be wrong twice over: it hands out numbers that survive a rollback, and concurrent
-- transactions can commit out of order, so a device can read past a number that has not
-- committed and never see it again. Part two of this project assigns `seq` under an advisory
-- lock. Until then this column is plain BIGINT and nothing may write to it concurrently.
CREATE TABLE changes (
    seq          BIGINT PRIMARY KEY,
    entity       TEXT NOT NULL CHECK (entity IN ('object','activity','reminder','attachment','file')),
    entity_uuid  TEXT NOT NULL,
    op           TEXT NOT NULL CHECK (op IN ('create','set','delete')),
    field        TEXT,
    value        TEXT,
    edited_at    TEXT NOT NULL,
    applied_at   TEXT NOT NULL,
    user_id      BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id    TEXT NOT NULL,
    client_op_id TEXT NOT NULL
);
CREATE INDEX idx_changes_user_seq ON changes(user_id, seq);
CREATE UNIQUE INDEX idx_changes_user_op ON changes(user_id, client_op_id);

CREATE TABLE field_clock (
    entity      TEXT NOT NULL,
    entity_uuid TEXT NOT NULL,
    field       TEXT NOT NULL,
    edited_at   TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    PRIMARY KEY (entity, entity_uuid, field)
);
```

The currency setting and the sync epoch are **not** seeded here. SQLite seeds them with
`INSERT … randomblob()`; the app writes both on first start instead (Task 4), so one code path
produces them on either backend.

- [ ] **Step 4: Select the migrator at runtime**

In `src/db.rs`:

```rust
/// Which set of migrations this URL needs. `migrate!` embeds the files at compile time, so both
/// directories ship in the binary and the choice is which embedded set to run.
pub fn migrator(url: &str) -> sqlx::migrate::Migrator {
    if url.starts_with("sqlite:") {
        sqlx::migrate!("./migrations/sqlite")
    } else {
        sqlx::migrate!("./migrations/postgres")
    }
}
```

- [ ] **Step 5: Run both**

```bash
cargo test
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16
sleep 5
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test --test schema_parity
docker rm -f logb-pg
```

Expected: SQLite suite green; the parity test passes. If it reports a difference, fix the schema
that is wrong — do not narrow the test to make the difference disappear.

- [ ] **Step 6: Commit**

```bash
git add migrations src/db.rs tests/schema_parity.rs
git commit -m "feat: a PostgreSQL schema, and a test that keeps it level with SQLite's"
```

---

### Task 4: The statements that cannot be written once

**Files:**
- Create: `src/dialect.rs`
- Modify: `src/api/search.rs`, `src/api/users.rs` and `src/auth.rs` (username lookups),
  `src/sync/epoch.rs`, `src/db.rs` (first-start seeding)
- Test: `tests/dialect.rs` (new)

**Interfaces:**
- Produces: `dialect::Backend::{Sqlite, Postgres}`, `Backend::of(url)`,
  `Backend::case_insensitive_like(&self) -> &'static str`,
  `Backend::name_order(&self, column: &str) -> String`.

- [ ] **Step 1: Write the failing tests**

Create `tests/dialect.rs`. These run against whichever backend is configured, so they are the
tests that catch a dialect difference rather than describing one:

```rust
//! Searching and sorting are where the two databases disagree most quietly. SQLite's `LIKE` is
//! case-insensitive for ASCII only; PostgreSQL's `ILIKE` follows the server's collation. A
//! German user searching "ölwechsel" for an entry titled "Ölwechsel" is the case that decides
//! whether this app behaves the same on both.

mod common;

#[tokio::test]
async fn search_is_case_insensitive_including_accents() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_activity(&object["id"], "Ölwechsel").await;

    for term in ["Ölwechsel", "ölwechsel", "ÖLWECHSEL", "ölwech"] {
        let hits = app.search(term).await;
        assert_eq!(hits["activities"].as_array().unwrap().len(), 1, "searching {term} found nothing");
    }
}

#[tokio::test]
async fn usernames_are_case_insensitive_for_uniqueness_and_sign_in() {
    let app = common::spawn().await;
    app.setup("Ben", "correct horse").await;
    let taken = app.create_user("BEN", "another one").await;
    assert_eq!(taken.status(), 409, "username uniqueness must ignore case on both backends");
    assert_eq!(app.sign_in("bEn", "correct horse").await.status(), 200);
}

#[tokio::test]
async fn objects_sort_without_regard_to_case() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for name in ["apple", "Banana", "cherry"] {
        app.create_object(&app.client, name, None).await;
    }
    let names = app.object_names().await;
    assert_eq!(names, vec!["apple", "Banana", "cherry"], "case must not push Banana to the front");
}
```

Add whichever small helpers (`create_activity`, `search`, `create_user`, `sign_in`,
`object_names`) the harness is missing, following the shape of the ones already in
`tests/common/mod.rs`.

- [ ] **Step 2: Run them on SQLite and watch them pass or fail**

Run: `cargo test --test dialect`

Expected: they pass on SQLite today except where the app already differs — report exactly which
pass and which fail before changing anything, because that is the baseline the PostgreSQL side
has to match.

- [ ] **Step 3: Write the adapter**

Create `src/dialect.rs`:

```rust
//! The three places the two databases cannot be written the same way.
//!
//! Everything else in this app is portable SQL. Keeping the exceptions in one file means the
//! next person can read the whole surface of the difference in under a minute, rather than
//! discovering it one failing query at a time.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

impl Backend {
    pub fn of(url: &str) -> Self {
        if url.starts_with("sqlite:") { Self::Sqlite } else { Self::Postgres }
    }

    /// SQLite's `LIKE` already ignores case for ASCII; PostgreSQL needs `ILIKE`, which also
    /// covers the accented characters SQLite's does not.
    pub fn case_insensitive_like(&self) -> &'static str {
        match self { Self::Sqlite => "LIKE", Self::Postgres => "ILIKE" }
    }

    /// SQLite sorts with `COLLATE NOCASE`; PostgreSQL sorts by `lower(...)`.
    pub fn name_order(&self, column: &str) -> String {
        match self {
            Self::Sqlite => format!("{column} COLLATE NOCASE"),
            Self::Postgres => format!("lower({column})"),
        }
    }
}
```

Username lookups compare `lower(username) = lower($1)` on both backends, which matches the
unique index the PostgreSQL schema declares and behaves identically on SQLite.

`src/sync/epoch.rs` stops using `hex(randomblob(16))` and generates the value in Rust with the
`rand` crate that is already a dependency. `db::connect` seeds the epoch and the default
currency on first start if the `settings` rows are absent, replacing the seeds that used to live
in the SQLite migrations.

- [ ] **Step 4: Run both backends**

```bash
cargo test
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16 && sleep 5
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test
docker rm -f logb-pg
```

Expected: identical results on both. Report any test that passes on one and fails on the other —
that is the finding this whole task exists to surface.

- [ ] **Step 5: Commit**

```bash
git add src/dialect.rs src tests/dialect.rs
git commit -m "feat: name the three places the two databases disagree"
```

---

### Task 5: The test harness runs against either backend

**Files:**
- Modify: `tests/common/mod.rs`
- Test: the whole suite is the test

**Interfaces:**
- Produces: `common::spawn()` unchanged in signature, backed by SQLite or PostgreSQL depending
  on `LOGB_TEST_DATABASE_URL`.

- [ ] **Step 1: Give each test its own database**

Today every test gets its own temporary SQLite file, so tests cannot see each other's rows. A
single shared PostgreSQL database would break that immediately — the suite runs in parallel.

In `tests/common/mod.rs`, when `LOGB_TEST_DATABASE_URL` is set:

```rust
/// Each test gets its own PostgreSQL database, created from the server URL and dropped when the
/// test finishes. The suite runs in parallel and every test assumes it is alone -- sharing one
/// database would make failures depend on which tests happened to run together, which is the
/// worst kind of flake to debug.
async fn fresh_postgres(server_url: &str) -> (String, String) {
    let name = format!("logb_test_{}", uuid_like_suffix());
    let admin = sqlx::any::AnyPoolOptions::new().max_connections(1).connect(server_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
    (name, replace_database_in_url(server_url, &name))
}
```

`uuid_like_suffix()` is the process id plus an atomic counter -- unique within a run, which is
all that is needed, and no new dependency. `replace_database_in_url()` swaps the path segment
after the host: `postgres://user:pw@host:5432/postgres` becomes
`postgres://user:pw@host:5432/logb_test_1234_7`. Both are private helpers in the harness with a
unit test each.

Drop the database in the harness's teardown. If teardown cannot be guaranteed, name databases
with a common prefix and drop leftovers at the start of a run — a CI container is discarded
anyway, and a developer's server should not slowly fill with them.

- [ ] **Step 2: Run the whole suite against PostgreSQL**

```bash
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16 && sleep 5
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test
docker rm -f logb-pg
```

Expected: **this will not be green the first time.** Report the failures grouped by cause rather
than fixing them blindly — each group is either a real portability defect or a test that assumed
SQLite. Say which is which; a test that assumed SQLite may legitimately need a backend guard,
but "it only runs on SQLite" must be a deliberate, stated decision per test, not a shrug.

Two known ones to expect: `tests/backup.rs` and `tests/migration_object_types.rs` are about
SQLite mechanisms (`VACUUM INTO`, a SQLite table rebuild) and should be skipped on PostgreSQL
with an explicit `if backend is postgres { return }` and a comment saying why. Part five of this
project deals with backups properly.

- [ ] **Step 3: Commit**

```bash
git add tests
git commit -m "test: run the suite against whichever database is configured"
```

---

### Task 6: CI runs both

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add the PostgreSQL job**

Add a second backend job beside the existing `backend` one. It runs the same commands with a
service container and the URL set:

```yaml
  backend-postgres:
    name: backend on postgresql
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: logb
        ports: ['5432:5432']
        options: >-
          --health-cmd pg_isready --health-interval 10s --health-timeout 5s --health-retries 5
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      # rust-embed needs frontend/dist to exist to compile; the tracked .gitkeep keeps the
      # directory, and the icon is what the export test looks for.
      - run: mkdir -p frontend/dist && cp frontend/public/icon.svg frontend/dist/
      - run: cargo test --all-targets
        env:
          LOGB_TEST_DATABASE_URL: postgres://postgres:logb@127.0.0.1:5432/postgres
```

Copy the existing `backend` job's steps rather than inventing new ones — it already handles the
`frontend/dist` seeding that rust-embed needs, and diverging from it is how one job silently
stops testing what the other does.

- [ ] **Step 2: Make it required, not advisory**

Both backend jobs must be able to fail the workflow. A job that is allowed to fail is a job
nobody reads.

- [ ] **Step 3: Verify on a branch**

Push the branch and watch the run. Report both jobs' outcomes and the PostgreSQL job's duration —
if it is far slower than the SQLite one, say so; that is worth knowing before it is on every
commit.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: run the backend suite against PostgreSQL as well"
```

---

### Task 7: Refuse clearly where SQLite is assumed

**Files:**
- Modify: `src/backup.rs`, `src/restore.rs`, `src/main.rs`
- Test: `tests/backup.rs`

**Interfaces:** none changed.

Part five decides what backups mean on PostgreSQL. This task only makes the SQLite-only paths
fail honestly in the meantime, because they are reachable the moment someone sets a PostgreSQL
URL.

- [ ] **Step 1: Write the failing test**

In `tests/backup.rs`:

```rust
/// `VACUUM INTO` and replacing the database file are SQLite mechanisms. On PostgreSQL they must
/// say so and stop, rather than failing somewhere inside sqlx with a syntax error that names
/// nothing the operator can act on.
#[tokio::test]
async fn backup_and_restore_refuse_on_postgresql() {
    let Ok(url) = std::env::var("LOGB_TEST_DATABASE_URL") else { return };
    let err = logb::backup::run_once(&url).await.unwrap_err().to_string();
    assert!(err.contains("PostgreSQL"), "the message must name the reason: {err}");
    let err = logb::restore::run(&url, std::path::Path::new("/tmp/whatever.db")).await.unwrap_err().to_string();
    assert!(err.contains("PostgreSQL"), "the message must name the reason: {err}");
}
```

Match the real function names and signatures in `src/backup.rs` and `src/restore.rs` — the names
above are indicative, the ones in the source are authoritative.

- [ ] **Step 2: Implement the guards**

Each entry point checks the backend and returns an error naming it: that backup and restore are
SQLite mechanisms, that on PostgreSQL the database is backed up with PostgreSQL's own tooling,
and — for restore — that `logb --copy-to` exists for moving data between databases.

The nightly job must not run at all on PostgreSQL, rather than running and failing every night.

- [ ] **Step 3: Run both backends**

Run the suite on SQLite and with `LOGB_TEST_DATABASE_URL` set.

Expected: SQLite's backup and restore tests are untouched and still pass — that is the important
half, because it is the path actually in use today.

- [ ] **Step 4: Commit**

```bash
git add src tests/backup.rs
git commit -m "fix: say that backup and restore are SQLite mechanisms"
```

---

## When this is done

The app runs on either database, chosen by `LOGB_DATABASE_URL`, and CI proves it on both. The
sync cursor's ordering on PostgreSQL is still **not** safe — that is part two, and no phone
should sync against a PostgreSQL instance until it lands. Say so in the branch's final report;
do not add the setting or document the URL as supported until then.
