# PostgreSQL part 3: moving the data across — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `logb --copy-to <url>` copies a LogB database into an empty one, preserving every
identifier, and proves the copy arrived before reporting success.

**Architecture:** One new module, `src/copy.rs`, driven by a clap flag like `--backup` and
`--restore` already are. It reads the configured database and writes the destination inside a
single write transaction, table by table in dependency order, then resets identity sequences,
verifies counts and checksums, and rotates the destination's sync epoch.

**Tech Stack:** Rust, sqlx 0.9 over `AnyPool`, SQLite and PostgreSQL 16.

## Global Constraints

- No behaviour change to the running server: this is a command that exits, like `--backup`.
- SQLite 318 backend tests and PostgreSQL 318 pass unchanged at every task boundary. No
  `--skip`, no `#[ignore]`.
- No new dependency.
- `cargo clippy --all-targets --locked -- -D warnings` clean.
- Every test must be shown to fail before its fix. This project has shipped six assertions that
  could not fail; each was found by running them, never by reading them.
- Run verification in the FOREGROUND with long timeouts. Background runs die when a subagent's
  turn ends.

## Running PostgreSQL

```bash
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16
until docker exec logb-pg pg_isready -q; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test
docker rm -f logb-pg
```

A fixed `sleep` is not enough. The harness opens a pool of 2 per test app, so a stock container
suffices.

## The tables, in dependency order

`users`, `settings`, `api_tokens`, `sessions`, `objects`, `activities`, `files`, `attachments`,
`reminders`, `changes`, `field_clock`.

`attachments` references `objects`, `activities` and `files`; `reminders` references `objects`
and `activities`; `changes` and `api_tokens` and `sessions` reference `users`. Copying in this
order means every foreign key has its target before it is written.

## What this does not do

It does not move blobs. `files` rows are copied; the bytes under `LOGB_DATA_DIR/files` are
content-addressed and never rewritten, so the directory is copied with `cp -a` or moved with the
volume. The command says so when it finishes, because a database without its blobs looks fine
until someone opens a photo.

---

### Task 1: The copy, in dependency order

**Files:**
- Create: `src/copy.rs`
- Modify: `src/lib.rs` (add `pub mod copy;`), `src/config.rs` (the flag), `src/main.rs` (dispatch)
- Test: `tests/copy.rs` (new)

**Interfaces:**
- Produces: `copy::run(source_url: &str, dest_url: &str, force: bool) -> Result<Report, BoxError>`
  where `pub struct Report { pub tables: Vec<(String, i64)>, pub epoch: String }` — `tables` is
  each table name and the number of rows copied, in the order they were copied.

- [ ] **Step 1: Write the failing test**

Create `tests/copy.rs`:

```rust
//! Moving a database to another backend. The thing that matters is that identifiers survive:
//! every foreign key still points where it did, and every device's stored ids still resolve.

mod common;

/// Seeds a database through the real API, so the copy is exercised against rows the application
/// actually produces rather than rows a test invented.
async fn seeded() -> common::TestApp {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_activity(&object["id"], "Ölwechsel").await;
    app.create_activity(&object["id"], "Winter tyres").await;
    app
}

#[tokio::test]
async fn every_row_and_identifier_survives_the_copy() {
    let app = seeded().await;
    let dest = common::scratch_database().await;

    let report = logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap();

    // Row counts, per table, in the order they were copied.
    let copied: Vec<(String, i64)> = report.tables.clone();
    assert!(copied.iter().any(|(t, n)| t == "objects" && *n == 1), "{copied:?}");
    assert!(copied.iter().any(|(t, n)| t == "activities" && *n == 2), "{copied:?}");

    // Identifiers, not just counts: the activities must still hang off the same object id.
    let src_rows = app.all_activity_ids().await;
    let dest_rows = dest.all_activity_ids().await;
    assert_eq!(src_rows, dest_rows, "activity ids or their object_id changed in the copy");
}

#[tokio::test]
async fn a_destination_that_already_holds_data_is_refused() {
    let app = seeded().await;
    let dest = common::scratch_database().await;
    logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap();

    // Copying again without --force would merge two histories into one database.
    let err = logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap_err().to_string();
    assert!(err.contains("--force"), "the refusal must name the way round it: {err}");
}
```

`common::scratch_database()` returns a handle with a `url` and helpers, creating an empty
database of whichever backend the suite is running against — a temporary file for SQLite, a
freshly created database for PostgreSQL. The harness already creates per-test PostgreSQL
databases; reuse that machinery rather than writing a second copy of it. `TestApp` needs
`database_url()` and `all_activity_ids()`; follow the file's existing style.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test --test copy`

Expected: compilation failure — `logb::copy` does not exist.

- [ ] **Step 3: Write the copy**

Create `src/copy.rs`. It opens both databases with `db::connect_existing` for the source (no
migrations: the source is whatever it is) and `db::connect` for the destination (which applies
the schema if it is empty), then copies inside one `db::begin_write` transaction on the
destination.

```rust
//! `logb --copy-to <url>`: move a database to another backend, once.
//!
//! Identifiers are preserved rather than reassigned. Every foreign key in this schema is an
//! `id`, and every device that has ever synced holds `client_uuid`s that resolve to them, so a
//! copy that renumbered anything would silently detach half the data from the other half.

/// The tables, in an order where every foreign key's target is written before it is.
const TABLES: [&str; 11] = [
    "users", "settings", "api_tokens", "sessions", "objects", "activities", "files",
    "attachments", "reminders", "changes", "field_clock",
];
```

For each table: read every row from the source, and insert it into the destination. Read the
column names from the source at run time (`SELECT *` and the row's own column list) rather than
hard-coding eleven column lists that will drift from the schema — the schema parity test already
guarantees both backends agree on those names.

The destination is refused unless empty: check `users` — a LogB database with no users is one
that has never been set up. With `force`, proceed anyway.

**The source must not be in use.** Copying from a database a server is still writing to captures
a moving target: the later tables would be read after the earlier ones changed, and the copy
would be internally inconsistent while reporting success. On SQLite, take the write lock
briefly and fail if it is held — `BEGIN IMMEDIATE` against the source returns `SQLITE_BUSY`
when a server has it. On PostgreSQL, count other connections to the same database
(`pg_stat_activity`, excluding this one) and refuse if there are any. Both refusals say what to
do: stop the server first.

**Rotate the destination's epoch, last, inside the transaction.** `changes` and `field_clock`
are copied verbatim, so the cursor numbers survive — but a device resuming mid-stream against a
freshly copied database is not a risk worth taking for the one re-bootstrap it saves.
`sync::epoch::rotate` is the existing function; the new epoch goes in the `Report` so the
command can print it.

- [ ] **Step 4: Add the flag and the dispatch**

In `src/config.rs`, beside `--backup` and `--restore`:

```rust
    /// Copy this database into another one and exit. The destination must be empty. Blobs are
    /// not moved: they are content-addressed files under the data directory, and a copy of that
    /// directory pairs with any database.
    #[arg(long, value_name = "URL")]
    pub copy_to: Option<String>,

    /// Copy into a destination that already holds data. Two histories in one database.
    #[arg(long, requires = "copy_to")]
    pub force: bool,
```

In `src/main.rs`, beside the `--backup` arm:

```rust
    if let Some(dest) = config.copy_to.clone() {
        let report = logb::copy::run(&config.database_url()?, &dest, config.force).await?;
        for (table, rows) in &report.tables {
            println!("{rows:>7} {table}");
        }
        println!("sync epoch is now {} -- every device will re-bootstrap", report.epoch);
        println!("blobs are NOT copied: copy the files/ directory alongside this database");
        return Ok(());
    }
```

- [ ] **Step 5: Add a test for the in-use refusal**

Append to `tests/copy.rs`:

```rust
/// Copying out of a database a server is still writing to would capture a moving target: later
/// tables read after earlier ones changed, internally inconsistent, reported as success.
#[tokio::test]
async fn copying_from_a_database_still_in_use_is_refused() {
    let app = seeded().await;              // its server is running and holds the database
    let dest = common::scratch_database().await;
    let err = logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap_err().to_string();
    assert!(err.contains("stop the server"), "the refusal must say what to do: {err}");
}
```

Watch it fail before implementing the check, and report what you saw.

- [ ] **Step 6: Run both backends**

Expected: 321 on SQLite (318 plus the three new tests), 321 on PostgreSQL.

Note that the SQLite-to-SQLite and PostgreSQL-to-PostgreSQL cases are what the suite exercises
automatically. The cross-backend case is what the command is *for*, and Task 4 tests it
explicitly.

- [ ] **Step 7: Commit**

```bash
git add src/copy.rs src/lib.rs src/config.rs src/main.rs tests/copy.rs tests/common/mod.rs
git commit -m "feat: copy a database into another one, preserving every identifier"
```

---

### Task 2: Sequences, so the next insert does not collide

**Files:**
- Modify: `src/copy.rs`
- Test: `tests/copy.rs`

**Interfaces:**
- Consumes: `copy::run` from Task 1.

- [ ] **Step 1: Write the failing test**

Append to `tests/copy.rs`:

```rust
/// Copying rows with their ids does not move the counter that hands out the next one. On
/// PostgreSQL an identity sequence still sits at 1 after a copy, so the first insert collides
/// with the row that already has id 1 -- and the symptom arrives days later, as a duplicate key
/// on an ordinary save, long after anyone would connect it to the copy.
#[tokio::test]
async fn the_destination_can_still_insert_after_a_copy() {
    let app = seeded().await;
    let dest = common::scratch_database().await;
    logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap();

    // The same call the API makes when someone adds an object.
    let inserted = dest.insert_object("Second car").await;
    assert!(inserted.is_ok(), "inserting after the copy failed: {inserted:?}");
}
```

- [ ] **Step 2: Run it and watch it fail on PostgreSQL**

Expected: a duplicate key violation on `objects_pkey`. On SQLite it passes already —
`AUTOINCREMENT` derives the next id from the table, so a copy carries it. Report both.

- [ ] **Step 3: Reset the sequences**

After the rows are copied, on PostgreSQL only, set each identity sequence to the highest id
copied:

```rust
    // SQLite needs nothing here: `AUTOINCREMENT` reads the next id from the table itself (and
    // from `sqlite_sequence`, which the row copy carries). PostgreSQL's identity sequence is a
    // separate object that the copy does not touch, so without this the first insert after a
    // copy collides with a row that is already there.
    if crate::dialect::Backend::of(dest_url) == crate::dialect::Backend::Postgres {
        for table in ["users", "objects", "activities", "files", "attachments", "reminders", "api_tokens", "changes"] {
            let column = if *table == "changes" { "seq" } else { "id" };
            let sql = format!(
                "SELECT setval(pg_get_serial_sequence('{table}', '{column}'), \
                 COALESCE((SELECT MAX({column}) FROM {table}), 1))"
            );
            sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *tx).await?;
        }
    }
```

The table and column names come from that fixed list, never from input — which is the audit
`AssertSqlSafe` asks the author to have made.

- [ ] **Step 4: Run both backends**

Expected: the new test passes on both.

- [ ] **Step 5: Prove the fix is what fixed it**

Comment out the `setval` loop, run the test against PostgreSQL, watch the duplicate key, restore
it. Report both outputs.

- [ ] **Step 6: Commit**

```bash
git add src/copy.rs tests/copy.rs tests/common/mod.rs
git commit -m "fix: move the id counters, not just the ids"
```

---

### Task 3: Verify, then report

**Files:**
- Modify: `src/copy.rs`
- Test: `tests/copy.rs`

**Interfaces:**
- Consumes: `copy::run` from Tasks 1 and 2.

- [ ] **Step 1: Write the failing test**

Append to `tests/copy.rs`:

```rust
/// A copy that reports success while having dropped rows is worse than one that fails: the
/// operator deletes the source. So the command counts both sides and compares, and a mismatch
/// is an error naming the table.
#[tokio::test]
async fn a_copy_that_loses_rows_fails_and_names_the_table() {
    let app = seeded().await;
    let dest = common::scratch_database().await;

    // Delete a row from the destination mid-copy by racing is unreliable; instead copy, then
    // remove a row and re-run the verification directly. That is the same check the command
    // performs, against a destination that is genuinely wrong.
    logb::copy::run(&app.database_url(), &dest.url, false).await.unwrap();
    dest.delete_one_activity().await;

    let err = logb::copy::verify(&app.database_url(), &dest.url).await.unwrap_err().to_string();
    assert!(err.contains("activities"), "the error must name the table that differs: {err}");
}
```

- [ ] **Step 2: Run it and watch it fail**

Expected: `copy::verify` does not exist.

- [ ] **Step 3: Write the verification**

Add `pub async fn verify(source_url: &str, dest_url: &str) -> Result<(), BoxError>` and call it
from `run` before committing. For each table: compare the row count, and compare a checksum over
the primary keys — `SELECT COUNT(*), COALESCE(SUM(id), 0), COALESCE(MAX(updated_at), '')` where
the table has those columns, and count alone where it does not.

A mismatch returns an error naming the table and both values. Because this runs before the
commit, a failed verification rolls the whole copy back rather than leaving a half-copied
database that looks finished.

- [ ] **Step 4: Run both backends**

Expected: the new test passes on both; the earlier copy tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/copy.rs tests/copy.rs tests/common/mod.rs
git commit -m "feat: prove the copy arrived before reporting success"
```

---

### Task 4: The cross-backend copy, which is the point of the command

**Files:**
- Test: `tests/copy.rs`
- Modify: `src/copy.rs` only if the test finds something

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the test**

This is the case the command exists for, and no suite run exercises it: the suite runs entirely
on one backend at a time.

```rust
/// SQLite to PostgreSQL, which is the migration this command exists to perform. Skipped unless
/// a PostgreSQL server is configured, because there is nothing to copy into without one.
#[tokio::test]
async fn a_sqlite_database_copies_into_postgresql() {
    let Ok(server) = std::env::var("LOGB_TEST_DATABASE_URL") else {
        eprintln!("skipped: set LOGB_TEST_DATABASE_URL to a PostgreSQL server to run this");
        return;
    };
    // Seed a SQLite database specifically, whatever the suite is running against.
    let dir = tempfile::tempdir().unwrap();
    let source = logb::db::sqlite_url(dir.path()).unwrap();
    let app = common::spawn_on(&source).await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.create_activity(&object["id"], "Ölwechsel").await;

    let dest = common::scratch_database_on(&server).await;
    let report = logb::copy::run(&source, &dest.url, false).await.unwrap();
    assert!(report.tables.iter().any(|(t, n)| t == "activities" && *n == 1), "{:?}", report.tables);

    // And the copy is usable: the app starts against it and serves the data.
    let moved = common::spawn_on(&dest.url).await;
    let objects: serde_json::Value = moved.get_json("/objects").await;
    assert_eq!(objects["items"][0]["name"], "Golf");
    assert_eq!(objects["items"][0]["type"], "car");
}
```

`common::spawn_on(url)` starts a test app against a specific database URL; `scratch_database_on`
creates an empty database on a named server. Add both if missing.

- [ ] **Step 2: Run it**

Run the suite on SQLite with `LOGB_TEST_DATABASE_URL` **also** set, so this test has a server to
copy into while the rest of the suite runs on SQLite.

Expected: it passes. If it does not, that is the most valuable failure in this plan — report
what broke rather than adjusting the test. A type that survives a same-backend copy and not a
cross-backend one is exactly what this task exists to find.

- [ ] **Step 3: Commit**

```bash
git add tests/copy.rs tests/common/mod.rs src/copy.rs
git commit -m "test: copy a SQLite database into PostgreSQL and serve it"
```

---

### Task 5: Say how to use it

**Files:**
- Modify: `README.md`, `src/lib.rs` (the startup warning), `docs/superpowers/specs/2026-09-11-postgres-p3-copy-design.md` (status)

- [ ] **Step 1: Document the migration**

Add a short README section: stop the server, copy the blobs, run `--copy-to`, point
`LOGB_DATABASE_URL` at the new database, start. Say plainly that blobs are a separate `cp -a`,
that every device re-bootstraps afterwards because the epoch rotates, and that the source
database is left untouched so the move is reversible until it is deleted.

- [ ] **Step 2: Narrow the startup warning**

It currently says PostgreSQL is unsupported because there is no automatic backup **and** no way
to bring an existing database across. The second half is now false. Rewrite it to name only what
is still true — no automatic backup — and keep it a warning: parts four and five are unbuilt.

- [ ] **Step 3: Verify the warning**

Start the binary against a PostgreSQL URL and paste the line it logs.

- [ ] **Step 4: Mark the spec implemented and commit**

```bash
git add README.md src/lib.rs docs/superpowers/specs/2026-09-11-postgres-p3-copy-design.md
git commit -m "docs: how to move an existing database to PostgreSQL"
```
