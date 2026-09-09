# Backup, Restore and Health Check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give logby a nightly verified database snapshot with retention, a `--restore` command that is safe for synced devices, and a health check that actually reads the database.

**Architecture:** Backup runs from the existing background loop in `src/tasks.rs`, taking the hour as a parameter the way `notify::tick` already does. Restore is a CLI mode alongside `--backup`, and stamps a fresh `sync_epoch` so every device re-bootstraps rather than resuming on a cursor whose meaning changed. The health check asserts the applied migration count matches what the binary embeds.

**Tech Stack:** Rust, axum 0.8, sqlx 0.9 (SQLite), clap 4, tokio. Tests are `#[tokio::test]` integration tests over real HTTP, following `tests/common/mod.rs`.

## Global Constraints

- Server-only. No file under `frontend/` changes.
- `cargo clippy --all-targets --locked -- -D warnings` must pass; CI treats warnings as errors.
- `tests/openapi.rs` compares routes declared in `src/api/**` against `docs/openapi.json` **in both directions**. This plan adds no route, but Task 2 changes two existing responses — update their schemas in the same task.
- Automatic backup is **off unless `LOGBY_BACKUP_DIR` is set**, so an existing deployment behaves exactly as it does today until its operator opts in.
- Retention keeps the newest **14** snapshots.
- Any new config field must also be added to `test_config` in `tests/common/mod.rs`, which constructs `Config` literally and will not compile otherwise.
- Timestamps use `db::now()` / `db::today()`, matching every existing table.

---

### Task 1: A health check that reads the database

**Files:**
- Modify: `src/error.rs`, `src/db.rs`, `src/api/mod.rs`
- Test: `tests/health.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `AppError::Unavailable(String)` mapping to HTTP 503 with code `"unavailable"`; `db::expected_migrations() -> usize`.

Rationale for the reviewer: `/api/health` currently returns a static literal, so Docker reported this container healthy for nineteen hours while it ran against a database with no tables. The container's `HEALTHCHECK` already treats non-2xx as failure, so making the endpoint truthful needs no Dockerfile change.

- [ ] **Step 1: Write the failing test**

Append to `tests/health.rs`:

```rust
#[tokio::test]
async fn health_reports_the_database_state() {
    let app = common::spawn().await;
    let res = app.client.get(app.url("/health")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["migrations"].as_i64().unwrap() > 0, "it says how much schema it found");
}

#[tokio::test]
async fn health_turns_503_when_the_schema_is_incomplete() {
    let app = common::spawn().await;
    // Exactly the shape of the real incident: the process is up and serving, but the database
    // under it is not the one the binary was built for.
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = (SELECT max(version) FROM _sqlx_migrations)")
        .execute(&app.state.db).await.unwrap();

    let res = app.client.get(app.url("/health")).send().await.unwrap();
    assert_eq!(res.status(), 503, "a liveness probe must fail when the schema is behind");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"], "unavailable");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test health`
Expected: FAIL — `health_reports_the_database_state` panics because `migrations` is null, and `health_turns_503_when_the_schema_is_incomplete` gets 200.

- [ ] **Step 3: Add the error variant**

In `src/error.rs`, add to the `AppError` enum after `Conflict(String)`:

```rust
    #[error("{0}")]
    Unavailable(String),
```

and to `status_and_code`, beside the other explicit arms:

```rust
            AppError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
```

- [ ] **Step 4: Expose the expected migration count**

In `src/db.rs`, add:

```rust
/// How many migrations this binary carries.
///
/// `sqlx::migrate!` embeds the directory at compile time, so this is what the running code
/// believes the schema should be -- the number the health check compares the database against.
pub fn expected_migrations() -> usize {
    sqlx::migrate!("./migrations").iter().count()
}
```

- [ ] **Step 5: Make the handler read the database**

In `src/api/mod.rs`, replace the `health` function and its route registration. The route becomes `.route("/health", get(health))` as before — unchanged, so `tests/openapi.rs` stays green — but the handler gains state:

```rust
/// Liveness that is worth the name: it answers for the database, not just the process.
///
/// A handler returning a constant cannot distinguish "serving correctly" from "serving an empty
/// file", and an instance in the second state once ran for nineteen hours reporting itself
/// healthy. Counting applied migrations catches both halves of that: a database with no schema,
/// and one whose schema is older than the binary talking to it.
async fn health(State(state): State<App>) -> Result<Json<serde_json::Value>, AppError> {
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&state.db)
        .await
        .map_err(|e| AppError::Unavailable(format!("database unreadable: {e}")))?;
    let expected = crate::db::expected_migrations() as i64;
    if applied < expected {
        return Err(AppError::Unavailable(format!(
            "schema is behind: {applied} of {expected} migrations applied"
        )));
    }
    Ok(Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "migrations": applied,
    })))
}
```

Add `use axum::extract::State;` and `use crate::error::AppError;` to the imports at the top of `src/api/mod.rs` if they are not already there.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets --locked -- -D warnings`
Expected: PASS for both. Run the whole suite, not just `--test health`: every other test binary calls `/health` indirectly through `common::spawn`, so this proves the handler change broke nothing.

- [ ] **Step 7: Commit**

```bash
git add src/error.rs src/db.rs src/api/mod.rs tests/health.rs
git commit -m "feat: make the health check answer for the database, not just the process"
```

---

### Task 2: The sync epoch

**Files:**
- Create: `migrations/0008_sync_epoch.sql`, `src/sync/epoch.rs`
- Modify: `src/sync/mod.rs`, `src/api/sync.rs`, `docs/openapi.json`
- Test: `tests/sync.rs`

**Interfaces:**
- Consumes: `AppError` and the existing `AppError::Gone` (already maps to 410).
- Produces:
  - `sync::epoch::current(db: &sqlx::SqlitePool) -> Result<String, AppError>`
  - `sync::epoch::rotate(db: &sqlx::SqlitePool) -> Result<String, AppError>`
  - `GET /api/sync/pull` accepts an `epoch` query parameter and returns `epoch` in its body.
  - `GET /api/sync/bootstrap` returns `epoch` in its body.

Rationale for the reviewer: `changes.seq` is an autoincrement cursor. A restored database reissues those numbers for different ops, so a device resuming on its old cursor would pull ops that are not the edits it missed and apply the wrong history silently. The epoch is what makes that loud.

- [ ] **Step 1: Write the failing test**

Append to `tests/sync.rs`:

```rust
#[tokio::test]
async fn a_pull_carrying_a_stale_epoch_is_gone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let _ = car;

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().expect("pull states the epoch").to_string();
    let next = body["next_seq"].as_i64().unwrap();
    assert!(next > 0, "the REST create is already in the feed");

    // The cursor is current and the epoch matches: ordinary catch-up.
    let res = app.client.get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    // Same cursor, an epoch from a different database: the numbers no longer mean what the
    // device thinks they mean.
    let res = app.client.get(app.url(&format!("/sync/pull?since={next}&epoch=not-this-database")))
        .send().await.unwrap();
    assert_eq!(res.status(), 410);

    // A non-zero cursor with no epoch at all is the same failure: a client that cannot say
    // which database it is resuming against cannot safely resume.
    let res = app.client.get(app.url(&format!("/sync/pull?since={next}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 410);

    // A first pull carries no cursor, so it needs no epoch.
    assert_eq!(app.client.get(app.url("/sync/pull?since=0")).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn bootstrap_states_the_epoch_it_belongs_to() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let body: serde_json::Value = app.client.get(app.url("/sync/bootstrap"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().expect("bootstrap states the epoch");

    // The pair is usable together: the seq and epoch a bootstrap hands out are accepted by pull.
    let seq = body["seq"].as_i64().unwrap();
    let res = app.client.get(app.url(&format!("/sync/pull?since={seq}&epoch={epoch}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn rotating_the_epoch_forces_every_device_to_re_bootstrap() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let body: serde_json::Value = app.client.get(app.url("/sync/pull?since=0"))
        .send().await.unwrap().json().await.unwrap();
    let epoch = body["epoch"].as_str().unwrap().to_string();
    let next = body["next_seq"].as_i64().unwrap();

    let fresh = logby::sync::epoch::rotate(&app.state.db).await.unwrap();
    assert_ne!(fresh, epoch, "rotation produces a different epoch");

    let res = app.client.get(app.url(&format!("/sync/pull?since={next}&epoch={epoch}")))
        .send().await.unwrap();
    assert_eq!(res.status(), 410, "the device's epoch is now the old database's");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test sync epoch`
Expected: FAIL — `body["epoch"]` is null, so the `.expect(...)` panics.

- [ ] **Step 3: Write the migration**

Create `migrations/0008_sync_epoch.sql`:

```sql
-- Which database a cursor belongs to.
--
-- `changes.seq` is an autoincrement, so a restored snapshot reissues the same numbers for
-- entirely different ops. A device resuming on its old cursor would pull ops that are not the
-- edits it missed and apply them as though they were -- wrong history, silently, with nothing
-- reporting an error. Devices carry this value alongside their cursor; `--restore` changes it,
-- and a mismatch sends them back to a full bootstrap.
INSERT INTO settings (key, value) VALUES ('sync_epoch', lower(hex(randomblob(16))));
```

- [ ] **Step 4: Write the epoch helpers**

Create `src/sync/epoch.rs`:

```rust
//! The identity of the database a cursor refers to. See `migrations/0008_sync_epoch.sql`.

use crate::error::AppError;

pub async fn current(db: &sqlx::SqlitePool) -> Result<String, AppError> {
    let (v,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'sync_epoch'")
        .fetch_one(db)
        .await?;
    Ok(v)
}

/// Gives the database a new identity, invalidating every cursor held anywhere.
///
/// Called by `--restore`. It is deliberately not reachable over HTTP: rotating without
/// replacing the data would send every device to a bootstrap for no reason.
pub async fn rotate(db: &sqlx::SqlitePool) -> Result<String, AppError> {
    let fresh = uuid::Uuid::new_v4().to_string();
    sqlx::query("UPDATE settings SET value = ? WHERE key = 'sync_epoch'")
        .bind(&fresh)
        .execute(db)
        .await?;
    Ok(fresh)
}
```

In `src/sync/mod.rs`, add `pub mod epoch;` beside the other module declarations.

- [ ] **Step 5: Enforce it in the pull handler**

In `src/api/sync.rs`, add `epoch` to the query parameters and to both response bodies.

`PullParams` gains:

```rust
    pub epoch: Option<String>,
```

`PullOut` gains:

```rust
    /// Which database these seq numbers belong to. A client stores it beside its cursor.
    pub epoch: String,
```

In the `pull` handler, immediately after the existing horizon check, add:

```rust
    let epoch = crate::sync::epoch::current(&state.db).await?;
    // A first pull (`since` 0) has no cursor to invalidate, so it needs no epoch. Any other
    // cursor is a claim about a specific database, and a client that cannot back that claim --
    // wrong epoch, or none at all -- must not be allowed to resume against seq numbers that may
    // since have been reissued to different ops.
    if params.since > 0 && params.epoch.as_deref() != Some(epoch.as_str()) {
        return Err(AppError::Gone);
    }
```

and include `epoch` in the returned `PullOut`.

In the `bootstrap` handler, after the existing `map.insert` calls, add:

```rust
    map.insert("epoch".into(), serde_json::json!(crate::sync::epoch::current(&state.db).await?));
```

- [ ] **Step 6: Document the change**

In `docs/openapi.json`, add the `epoch` query parameter to `/sync/pull` (optional string, required whenever `since` is non-zero, `410` otherwise) and the `epoch` property to both the pull result and bootstrap result schemas. Do not add or remove any path or method — `tests/openapi.rs` checks route parity in both directions.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets --locked -- -D warnings`
Expected: PASS for both. Existing sync tests that pull with a non-zero cursor will need the epoch added to their URLs — that is the point of the change, not a failure; update them.

- [ ] **Step 8: Commit**

```bash
git add migrations/0008_sync_epoch.sql src/sync tests/sync.rs docs/openapi.json src/api/sync.rs
git commit -m "feat: tie a sync cursor to the database it came from"
```

---

### Task 3: The nightly backup

**Files:**
- Create: `src/backup.rs`
- Modify: `src/lib.rs`, `src/config.rs`, `src/tasks.rs`, `tests/common/mod.rs`
- Test: `tests/backup.rs`

**Interfaces:**
- Consumes: `db::backup_to`, `db::today`, `db::local_hour`.
- Produces:
  - `backup::tick(state: &App, hour_now: u32) -> Result<Option<std::path::PathBuf>, AppError>`
  - `backup::verify(path: &std::path::Path) -> Result<(), crate::db::BoxError>`
  - `Config` gains `backup_dir: Option<PathBuf>` (`LOGBY_BACKUP_DIR`) and `backup_hour: u32` (`LOGBY_BACKUP_HOUR`, default 3).

Taking the hour as a parameter is what makes this testable in a second rather than a day — the same shape `notify::tick(&state, hour)` already uses.

- [ ] **Step 1: Write the failing test**

Create `tests/backup.rs`:

```rust
mod common;

#[tokio::test]
async fn a_run_writes_and_verifies_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 3;
    }).await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    assert!(logby::backup::tick(&app.state, 2).await.unwrap().is_none(), "too early in the day");

    let made = logby::backup::tick(&app.state, 3).await.unwrap().expect("a snapshot was due");
    assert!(made.exists());
    assert_eq!(made.file_name().unwrap(), format!("logby-{}.db", logby::db::today()).as_str());
    logby::backup::verify(&made).await.expect("the snapshot opens and passes integrity_check");

    // The snapshot is the real database, not an empty file that happens to be valid SQLite.
    let pool = logby::db::connect_existing(made.parent().unwrap()).await;
    assert!(pool.is_err(), "connect_existing looks for logby.db, not a dated snapshot");
    let count: i64 = {
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&made).read_only(true);
        let p = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query_scalar("SELECT count(*) FROM objects").fetch_one(&p).await.unwrap()
    };
    assert_eq!(count, 1, "the object is in the snapshot");

    assert!(logby::backup::tick(&app.state, 3).await.unwrap().is_none(), "already done today");
}

#[tokio::test]
async fn backup_is_off_unless_a_directory_is_configured() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert!(logby::backup::tick(&app.state, 23).await.unwrap().is_none());
}

#[tokio::test]
async fn a_corrupt_snapshot_is_rejected_and_the_previous_one_survives() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    let good = backups.join("logby-2020-01-01.db");
    std::fs::write(&good, b"pretend this is yesterday's good snapshot").unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;

    // Today's slot already holds a file that is not a database at all.
    let today = backups.join(format!("logby-{}.db", logby::db::today()));
    std::fs::write(&today, b"not a database").unwrap();

    let made = logby::backup::tick(&app.state, 1).await.unwrap()
        .expect("an unverifiable snapshot must be replaced, not trusted");
    logby::backup::verify(&made).await.expect("the replacement is sound");
    assert!(good.exists(), "an earlier snapshot is never touched by a failure");
}

#[tokio::test]
async fn retention_keeps_the_newest_fourteen() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // Twenty days of history, oldest first.
    for day in 1..=20 {
        std::fs::write(backups.join(format!("logby-2020-01-{day:02}.db")), b"old").unwrap();
    }
    // Something that is not a snapshot must survive untouched.
    std::fs::write(backups.join("notes.txt"), b"keep me").unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    logby::backup::tick(&app.state, 1).await.unwrap().expect("today's snapshot");

    let mut names: Vec<String> = std::fs::read_dir(&backups).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("logby-"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 14, "fourteen kept, the rest pruned: {names:?}");
    assert_eq!(names[0], "logby-2020-01-08.db", "the oldest survivors are the newest of the old");
    assert!(names.last().unwrap().contains(&logby::db::today()), "today's is kept");
    assert!(backups.join("notes.txt").exists(), "unrelated files are not ours to delete");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test backup`
Expected: FAIL to compile — `logby::backup` does not exist and `Config` has no `backup_dir`.

- [ ] **Step 3: Add the configuration**

In `src/config.rs`, after the `backup` field:

```rust
    /// Directory for nightly database snapshots. Unset disables automatic backup entirely, so
    /// an instance that has not opted in behaves exactly as it did before this existed.
    #[arg(long, env = "LOGBY_BACKUP_DIR")]
    pub backup_dir: Option<PathBuf>,
    /// Hour (0-23), in `LOGBY_TIMEZONE`, at which the nightly snapshot is written.
    #[arg(long, env = "LOGBY_BACKUP_HOUR", default_value_t = 3)]
    pub backup_hour: u32,
```

In `tests/common/mod.rs`, add to the `Config` literal in `test_config`:

```rust
        backup_dir: None,
        backup_hour: 3,
```

- [ ] **Step 4: Write the backup module**

Create `src/backup.rs`:

```rust
//! The nightly snapshot.
//!
//! `VACUUM INTO` (via `db::backup_to`) reads through a consistent snapshot, so this is safe
//! against a live instance -- unlike copying the file, which can catch it mid-write and miss the
//! WAL entirely.

use crate::db::{self, BoxError};
use crate::error::AppError;
use crate::state::App;
use std::path::{Path, PathBuf};

/// How many snapshots survive. Two weeks is long enough to notice a bad delete that nobody
/// spotted the same day, and bounded so the directory can never fill the volume.
pub const KEEP: usize = 14;

/// Opens a finished snapshot and asks SQLite whether it is sound.
///
/// A backup nobody has opened is a guess. This is the cheapest possible proof, and it runs in
/// milliseconds on a database this size.
pub async fn verify(path: &Path) -> Result<(), BoxError> {
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
    let result: Result<(String,), _> = sqlx::query_as("PRAGMA integrity_check").fetch_one(&pool).await;
    pool.close().await;
    match result {
        Ok((r,)) if r == "ok" => Ok(()),
        Ok((r,)) => Err(format!("integrity_check said {r}").into()),
        Err(e) => Err(Box::new(e)),
    }
}

/// Writes today's snapshot if it is due and not already there. Returns the path when one was
/// made, `None` when there was nothing to do.
pub async fn tick(state: &App, hour_now: u32) -> Result<Option<PathBuf>, AppError> {
    let Some(dir) = state.config.backup_dir.clone() else { return Ok(None) };
    if hour_now < state.config.backup_hour {
        return Ok(None);
    }
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("logby-{}.db", db::today()));

    // An existing file only counts as done if it verifies. One that does not is worse than
    // nothing -- it occupies today's slot while being unrestorable -- so it is replaced.
    if dest.exists() {
        if verify(&dest).await.is_ok() {
            return Ok(None);
        }
        tracing::warn!(path = %dest.display(), "replacing an unverifiable snapshot");
        std::fs::remove_file(&dest)?;
    }

    db::backup_to(&state.db, &dest).await.map_err(|e| AppError::Internal(e.to_string()))?;
    if let Err(e) = verify(&dest).await {
        // Leave no unrestorable file behind, and leave every earlier snapshot alone: a failure
        // today must not cost yesterday's good copy.
        let _ = std::fs::remove_file(&dest);
        return Err(AppError::Internal(format!("snapshot failed verification: {e}")));
    }

    prune(&dir)?;
    Ok(Some(dest))
}

/// Deletes all but the newest `KEEP` snapshots.
///
/// Only files this module named are considered. A dated name sorts chronologically as a string,
/// so ordering needs no parsing -- and anything else in the directory is not ours to delete.
fn prune(dir: &Path) -> std::io::Result<()> {
    let mut ours: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("logby-") && n.ends_with(".db"))
        })
        .collect();
    ours.sort();
    let excess = ours.len().saturating_sub(KEEP);
    for path in ours.into_iter().take(excess) {
        tracing::debug!(path = %path.display(), "pruning an expired snapshot");
        std::fs::remove_file(path)?;
    }
    Ok(())
}
```

In `src/lib.rs`, add `pub mod backup;` in alphabetical position (before `pub mod config;`).

- [ ] **Step 5: Wire it into the background loop**

In `src/tasks.rs`, inside the loop beside the existing `notify::tick` call:

```rust
            match crate::backup::tick(&state, db::local_hour()).await {
                Ok(Some(path)) => tracing::info!(path = %path.display(), "wrote database snapshot"),
                Ok(None) => {}
                Err(e) => tracing::error!(error = %e, "database snapshot failed"),
            }
```

It sits beside `notify::tick` rather than inside the hourly `PRUNE_EVERY` block for the same reason that one does: it guards itself on having already run today, so a coarse tick is enough and a restart cannot skip the day.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets --locked -- -D warnings`
Expected: PASS for both.

- [ ] **Step 7: Commit**

```bash
git add src/backup.rs src/lib.rs src/config.rs src/tasks.rs tests/common/mod.rs tests/backup.rs
git commit -m "feat: write a verified database snapshot every night"
```

---

### Task 4: `--restore`, and the README

**Files:**
- Create: `src/restore.rs`
- Modify: `src/lib.rs`, `src/config.rs`, `src/main.rs`, `README.md`
- Test: `tests/backup.rs`

**Interfaces:**
- Consumes: `backup::verify` (Task 3), `sync::epoch::rotate` (Task 2), `db::connect`.
- Produces: `restore::run(data_dir: &Path, snapshot: &Path) -> Result<restore::Report, crate::db::BoxError>` with `pub struct Report { pub replaced_to: Option<PathBuf>, pub epoch: String }`; `Config` gains `restore: Option<PathBuf>`.

- [ ] **Step 1: Write the failing test**

Append to `tests/backup.rs`:

```rust
#[tokio::test]
async fn restore_brings_back_the_snapshot_and_changes_the_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let snapshot = logby::backup::tick(&app.state, 1).await.unwrap().expect("a snapshot");
    let epoch_before = logby::sync::epoch::current(&app.state.db).await.unwrap();

    // The mistake we are recovering from.
    app.create_object(&app.client, "Regrettable", None).await;
    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM objects")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live, 2);

    // The running instance holds the database open, so restore has to happen against a stopped
    // one. Point it at a data directory of its own, seeded from this instance's snapshot.
    let target = tempfile::tempdir().unwrap();
    std::fs::copy(&snapshot, target.path().join("logby.db")).unwrap();
    let report = logby::restore::run(target.path(), &snapshot).await.unwrap();

    assert!(report.replaced_to.is_some(), "the database it replaced is kept, not deleted");
    assert!(report.replaced_to.as_ref().unwrap().exists());
    assert_ne!(report.epoch, epoch_before, "a restored database is a different database");

    let pool = logby::db::connect_existing(target.path()).await.unwrap();
    let restored: i64 = sqlx::query_scalar("SELECT count(*) FROM objects").fetch_one(&pool).await.unwrap();
    assert_eq!(restored, 1, "the regrettable object is not in the restored database");
    let name: String = sqlx::query_scalar("SELECT name FROM objects").fetch_one(&pool).await.unwrap();
    assert_eq!(name, "Golf");
}

#[tokio::test]
async fn restore_refuses_a_file_that_is_not_a_database() {
    let target = tempfile::tempdir().unwrap();
    let junk = target.path().join("not-a-snapshot.db");
    std::fs::write(&junk, b"absolutely not a database").unwrap();
    std::fs::write(target.path().join("logby.db"), b"the live one").unwrap();

    let err = logby::restore::run(target.path(), &junk).await.unwrap_err();
    assert!(err.to_string().contains("not a usable snapshot"), "got: {err}");
    assert_eq!(
        std::fs::read(target.path().join("logby.db")).unwrap(),
        b"the live one",
        "a refused restore must not have touched the live database"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test backup restore`
Expected: FAIL to compile — `logby::restore` does not exist.

- [ ] **Step 3: Write the restore module**

Create `src/restore.rs`:

```rust
//! Putting a snapshot back.
//!
//! The order of operations is the design: nothing is touched until the source has proved itself,
//! and the database being replaced is moved aside rather than deleted, because a restore is
//! destructive and operators do sometimes restore the wrong file.

use crate::db::{self, BoxError};
use std::path::{Path, PathBuf};

pub struct Report {
    /// Where the replaced database was moved, if there was one.
    pub replaced_to: Option<PathBuf>,
    /// The identity the restored database now advertises. Every device holding the old one is
    /// sent back to a full bootstrap.
    pub epoch: String,
}

pub async fn run(data_dir: &Path, snapshot: &Path) -> Result<Report, BoxError> {
    // 1. Prove the source before risking anything.
    crate::backup::verify(snapshot)
        .await
        .map_err(|e| -> BoxError { format!("{} is not a usable snapshot: {e}", snapshot.display()).into() })?;
    {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(snapshot)
            .create_if_missing(false)
            .read_only(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
        let looks_right: Result<(i64,), _> =
            sqlx::query_as("SELECT count(*) FROM _sqlx_migrations").fetch_one(&pool).await;
        pool.close().await;
        looks_right.map_err(|e| -> BoxError {
            format!("{} is not a usable snapshot: no migration history ({e})", snapshot.display()).into()
        })?;
    }

    // 2. Move the live database aside. Its -wal and -shm go with it: leaving them beside a
    //    different database would have SQLite reading another file's journal.
    let live = data_dir.join("logby.db");
    let replaced_to = if live.exists() {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let dest = data_dir.join(format!("logby.db.replaced-{stamp}"));
        std::fs::rename(&live, &dest)?;
        for suffix in ["-wal", "-shm"] {
            let from = data_dir.join(format!("logby.db{suffix}"));
            if from.exists() {
                std::fs::rename(&from, data_dir.join(format!("logby.db.replaced-{stamp}{suffix}")))?;
            }
        }
        Some(dest)
    } else {
        None
    };

    // 3. Put the snapshot in place, then let `connect` migrate it -- a snapshot may predate the
    //    binary restoring it.
    std::fs::copy(snapshot, &live)?;
    let pool = db::connect(data_dir).await?;

    // 4. Give it a new identity, so no device resumes on a cursor whose meaning has changed.
    let epoch = crate::sync::epoch::rotate(&pool).await?;
    pool.close().await;

    Ok(Report { replaced_to, epoch })
}
```

In `src/lib.rs`, add `pub mod restore;` in alphabetical position (after `pub mod notify;`).

- [ ] **Step 4: Add the CLI mode**

In `src/config.rs`, after the `backup_hour` field:

```rust
    /// Replace the database with this snapshot and exit. The server must be stopped. The
    /// database being replaced is kept alongside it, and every synced device is sent back to a
    /// full bootstrap.
    #[arg(long, value_name = "PATH")]
    pub restore: Option<PathBuf>,
```

In `tests/common/mod.rs`, add `restore: None,` to the `Config` literal.

In `src/main.rs`, immediately after the existing `--backup` block:

```rust
    if let Some(src) = config.restore.clone() {
        let report = logby::restore::run(&config.data_dir, &src).await?;
        println!("restored {} into {}", src.display(), config.data_dir.display());
        if let Some(kept) = report.replaced_to {
            println!("the database it replaced is kept at {}", kept.display());
        }
        println!("sync epoch is now {} -- every device will re-bootstrap", report.epoch);
        return Ok(());
    }
```

- [ ] **Step 5: Document both halves in the README**

In `README.md`, extend the existing `## Backup` section with the automatic path, and add a `## Restore` section after it:

````markdown
Set `LOGBY_BACKUP_DIR` to turn on a nightly snapshot, written at `LOGBY_BACKUP_HOUR`
(default 3) and verified with `PRAGMA integrity_check` before it counts. The newest 14
are kept. Point it at a volume that is itself backed up — a snapshot on the same disk
protects you from your own mistakes, not from the disk's.

## Restore

The server must be stopped, so run it as a one-shot container against the same volume:

```bash
docker compose stop logby
docker compose run --rm logby /logby --restore /data/backups/logby-2026-09-01.db
docker compose start logby
```

The database being replaced is kept as `logby.db.replaced-<timestamp>` in the same
directory — restoring the wrong snapshot is recoverable.

A restored database gets a new sync epoch, so every phone re-bootstraps instead of
resuming from a cursor that now points at different history. That is deliberate and
you do not need to do anything about it.
````

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test && cargo clippy --all-targets --locked -- -D warnings`
Expected: PASS for both.

- [ ] **Step 7: Commit**

```bash
git add src/restore.rs src/lib.rs src/config.rs src/main.rs README.md tests/backup.rs tests/common/mod.rs
git commit -m "feat: restore a snapshot, and tell every device the database changed"
```

---

## Self-Review

**Spec coverage.** Every section maps to a task: the health check (Task 1); the epoch, its migration, and the `410` rule (Task 2); `LOGBY_BACKUP_DIR`/`LOGBY_BACKUP_HOUR`, the dated filename, `integrity_check` verification, 14-day retention and the same-day no-op (Task 3); `--restore` with source validation before any mutation, the preserved replaced database, migration of an older snapshot, epoch rotation, and the README's Backup and Restore sections (Task 4).

The spec's "out of scope" items stay out: nothing moves backups off the machine, and blobs are never archived.

**Placeholder scan.** No TBDs, no "add error handling", no "similar to Task N". Task 2 Step 6 and Task 4 Step 5 describe documentation edits in prose rather than as diffs, because both are additive text whose surrounding content the implementer must read first — the required content is stated exactly.

**Type consistency.** `backup::tick` and `backup::verify` are defined in Task 3 and used unchanged in Task 4's tests and in `restore::run`. `epoch::current` / `epoch::rotate` are defined in Task 2 and used in Task 3's tests and Task 4's implementation. `AppError::Unavailable` is added in Task 1 before its only use. `Report { replaced_to, epoch }` is defined once and destructured identically in `main.rs` and the tests. `Config` gains `backup_dir`, `backup_hour` (Task 3) and `restore` (Task 4); both tasks update `tests/common/mod.rs`, which will not compile otherwise.

**One ordering constraint the implementer must respect.** Task 2 must land before Task 4, because `restore::run` calls `epoch::rotate`. Tasks 1 and 3 are independent of both.
