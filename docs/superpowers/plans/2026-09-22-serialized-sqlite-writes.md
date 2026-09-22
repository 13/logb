# Serialized SQLite Writes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A SQLite write that contends for the database's write lock waits its turn instead of
failing, and the residual case that still cannot be served answers a retryable 503 rather than a
500.

**Architecture:** `AppState` gains a second pool, `write_db`, which on SQLite holds exactly one
connection to the same file; every write transaction borrows it, so waiters queue inside
`pool.acquire()` instead of racing SQLite's busy handler. On PostgreSQL `write_db` is the
existing pool and nothing changes. Separately, `error.rs` classifies a busy or locked database,
and a writer-pool acquire timeout, as 503 with `Retry-After: 1`.

**Tech Stack:** Rust 1.97, axum 0.8, sqlx 0.9 over the `Any` driver (SQLite and PostgreSQL),
tokio.

## Global Constraints

- No new runtime or dev dependency. `tokio` is already `features = ["full"]`.
- No migration; `tests/schema_parity.rs` stays green.
- PostgreSQL behaviour does not change in any respect, including the advisory lock.
- No new configuration surface. The five-second wait budget is a constant in `src/db.rs`.
- The invariant that makes this safe must survive: no code between `begin_write` and its commit
  may acquire a second pooled connection. Do not move any work into a write transaction.
- Every task ends green on `cargo clippy --all-targets --locked -- -D warnings`,
  `cargo test --locked`, and the PostgreSQL suite:

```bash
docker run --rm -d --name logb-pg -e POSTGRES_PASSWORD=logb -p 5432:5432 postgres:16
until docker exec logb-pg pg_isready -q; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:5432/postgres cargo test --all-targets --locked
docker rm -f logb-pg
```

- Commit messages: `type: sentence`, lower case, no trailing period, with the trailer

```
Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JaziF6pYRCjHuK5MdcdY2Z
```

Work on branch `serialized-sqlite-writes`, which already holds the design commit.

---

### Task 1: The writer pool

**Files:**
- Modify: `src/db.rs` (`pool_options` caller at :155, `begin_write` at :163-176)
- Modify: `src/state.rs` (the `AppState` struct)
- Modify: `src/lib.rs` (:184-218, state construction)
- Modify: `src/auth.rs` (the `#[cfg(test)]` `AppState` literal near :463)
- Modify: every `begin_write` call site in `src/` (34) and `tests/` (5)
- Modify: `src/copy.rs` (:121 `run_live`, :173), `src/api/database.rs` (:287)
- Modify: `src/sync/feed.rs` (:304)
- Test: `tests/pool.rs`, `tests/concurrency.rs`, `tests/objects.rs`

**Interfaces:**
- Produces: `AppState.write_db: AnyPool`;
  `db::connect_writer(url: &str, db: &AnyPool) -> Result<AnyPool, BoxError>`;
  `db::begin_write(state: &App) -> Result<Transaction<'static, Any>, sqlx::Error>`;
  `db::begin_write_on(pool: &AnyPool, backend: Backend) -> Result<Transaction<'static, Any>, sqlx::Error>`;
  `copy::run_live(state: &App, dest_url: &str) -> Result<Report, BoxError>`.
  Task 2 uses `begin_write` and the writer pool's acquire timeout.

- [ ] **Step 1: Name the busy timeout once**

`src/db.rs` currently spells `5_000` inline at :155 and `30_000` at :224. Add above
`after_connect` (:108):

```rust
/// How long a SQLite connection waits for the database's write lock before giving up, and how
/// long a writer waits for the one writer connection. One number, because they are the same
/// promise to a caller: five seconds of waiting, then an answer telling it to retry.
const WRITE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
```

and change :155 to use `WRITE_WAIT.as_millis() as u32` in place of `5_000`. Leave the
30-second one-shot pool at :224 alone; its doc comment already explains why it differs.

Run: `cargo test --locked --test pool`
Expected: pass — `busy_timeout` is still 5000.

- [ ] **Step 2: Add `connect_writer`**

In `src/db.rs`, after `connect_with_pool_size` (ends :161):

```rust
/// The pool every write transaction comes from.
///
/// On SQLite this is a second pool of exactly one connection against the same file. SQLite has
/// one write lock and its busy handler is not a queue: it retries on a backoff, so under enough
/// concurrent writers one can lose every retry and fail with `database is locked` while the
/// others make progress. One connection makes the queue explicit -- a writer waits in
/// `pool.acquire()`, in turn -- and this process never contends with itself for the file lock.
///
/// On PostgreSQL it is the ordinary pool. Writers there are already serialized by
/// `pg_advisory_xact_lock` (see `dialect::write_lock`), there is no file lock to lose, and a
/// second queue in front of the advisory lock would buy nothing.
///
/// `connect_with_pool_size` has already migrated and seeded this database, so this opens a pool
/// and does nothing else to it.
///
/// The test is `sqlite_file`, not the `sqlite:` prefix: an in-memory database belongs to the
/// pool that opened it, so a second pool against `sqlite::memory:` would quietly be a second,
/// empty database rather than another way into this one. Nothing in the server opens one, and
/// this is what keeps that true.
pub async fn connect_writer(url: &str, db: &AnyPool) -> Result<AnyPool, BoxError> {
    let Some(_) = sqlite_file(url) else {
        return Ok(db.clone());
    };
    Ok(pool_options(url, 1, WRITE_WAIT.as_millis() as u32)
        .acquire_timeout(WRITE_WAIT)
        .connect(url)
        .await?)
}
```

`sqlite_file` (`src/db.rs:41`) is `pub(crate)` and already returns `None` for both a
PostgreSQL URL and `:memory:`, which is exactly the set that should share one pool.

- [ ] **Step 3: Carry it on the state**

`src/state.rs`, after the `db` field and its comment:

```rust
    /// The pool write transactions come from: on SQLite one connection, so writers queue for
    /// their turn instead of racing the file lock; on PostgreSQL the same pool as `db`. See
    /// `db::connect_writer`.
    pub write_db: AnyPool,
```

`src/lib.rs`, immediately after `db::load_timezone(config.timezone, &db).await?;` (:205):

```rust
    let write_db = db::connect_writer(&url, &db).await?;
```

and add `write_db,` to the `AppState { .. }` literal at :209.

In `src/auth.rs`'s test module the literal is `Arc::new(AppState { db, database_url: url, .. })`,
and `db` is moved by that first field. Put the clone **before** it or the borrow fails:

```rust
        Arc::new(AppState {
            write_db: db.clone(),
            db,
```

A unit test with one pool is right: it never contends, and `begin_write` there only needs a pool
that works.

- [ ] **Step 4: Split `begin_write`**

Replace `src/db.rs:163-176` with:

```rust
/// Begins a transaction that intends to write, and makes it the only one.
///
/// Every write path in a request goes through here. The transaction comes from `write_db`, so
/// on SQLite it is holding the process's single writer connection for its whole life and any
/// other writer is queued behind it rather than competing for the file lock. See
/// `dialect::Backend::write_lock` for what PostgreSQL needs on top of the `BEGIN`.
pub async fn begin_write(
    state: &crate::state::App,
) -> Result<sqlx::Transaction<'static, sqlx::Any>, sqlx::Error> {
    begin_write_on(&state.write_db, state.backend).await
}

/// `begin_write` against a pool that is not this instance's own database: the destination of a
/// copy, or a test taking the lock by hand. A caller that has an `App` wants `begin_write`.
pub async fn begin_write_on(
    pool: &AnyPool,
    backend: crate::dialect::Backend,
) -> Result<sqlx::Transaction<'static, sqlx::Any>, sqlx::Error> {
    let mut tx = pool.begin_with(backend.begin_write()).await?;
    if let Some(lock) = backend.write_lock() {
        sqlx::query(lock).execute(&mut *tx).await?;
    }
    Ok(tx)
}
```

- [ ] **Step 5: Move the call sites**

Thirty-one sites in `src/` are spelled identically, so rewrite them mechanically and let the
compiler find the rest:

```bash
grep -rl 'begin_write(&state\.db, state\.backend)' src | xargs sed -i 's/begin_write(&state\.db, state\.backend)/begin_write(\&state)/g'
grep -rl 'begin_write(&app\.state\.db, app\.state\.backend)' tests | xargs sed -i 's/begin_write(&app\.state\.db, app\.state\.backend)/begin_write(\&app.state)/g'
```

Then the four that differ, by hand:

- `src/sync/feed.rs:304`: `crate::db::begin_write(db, state.backend)` becomes
  `crate::db::begin_write(state)`. Leave `let db = &state.db;` at :204 and every read that uses
  it alone — reads stay on the read pool.
- `src/copy.rs:173`: `db::begin_write(dest, dest_backend)` becomes
  `db::begin_write_on(dest, dest_backend)`. This is the destination database.
- `src/copy.rs:121-125`: `run_live` takes the state, so its source lock is the writer
  connection and a queued writer waits for the copy instead of timing out against it:

```rust
pub async fn run_live(state: &crate::state::App, dest_url: &str) -> Result<Report, BoxError> {
    // Opened before the write lock is taken: `db::connect` migrates the destination, and every
    // write to this server is stalled for as long as the transaction below is open.
    let dest = db::connect(dest_url).await?;
    let mut src = match db::begin_write(state).await {
```

  Keep the rest of the function as it is, including the `dest.close()` on the error arm and the
  explicit `src.rollback()`. Update its doc comment's first paragraph, which says it "takes the
  pool the server already holds": it now takes the state and writes through `write_db`.
- `src/api/database.rs:287`: `copy::run_live(&state.db, state.backend, &url)` becomes
  `copy::run_live(&state, &url)`.

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean. Any remaining error is a call site the sed did not reach; fix it the same way.

- [ ] **Step 6: Point the mechanism tests at the mechanism**

The sed in Step 5 already moved the five test call sites onto the writer pool, which is what
`tests/concurrency.rs:17,22,31,254` and `tests/objects.rs:774` need: they hold a write lock by
hand and assert that a real request blocks on it. Confirm each still reads as intended and that
`tests/objects.rs:763`'s `db_pool_size = Some(4)` comment still makes sense — the read pool is
what that number now sizes.

Run: `cargo test --locked --test concurrency --test objects`
Expected: pass. `two_write_transactions_do_not_overlap` now proves the writer pool serializes,
which is the property this task adds.

- [ ] **Step 7: Assert the writer connection is configured like the rest**

Append to `tests/pool.rs`:

```rust
/// The writer pool is a second pool, so the after-connect hook has to reach it too: a writer
/// connection without WAL or the busy timeout is the same silent loss the test above exists to
/// catch. Its single connection is the point -- that is what makes writers queue.
#[tokio::test]
async fn the_writer_pool_is_one_configured_connection() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}/logb.db?mode=rwc", dir.path().display());
    let pool = logb::db::connect(&url).await.unwrap();
    let writer = logb::db::connect_writer(&url, &pool).await.unwrap();

    let mut held = writer.acquire().await.unwrap();
    let mode: String =
        sqlx::query_scalar("PRAGMA journal_mode").fetch_one(&mut *held).await.unwrap();
    assert_eq!(mode, "wal", "the after-connect hook did not reach the writer connection");
    let timeout: i64 =
        sqlx::query_scalar("PRAGMA busy_timeout").fetch_one(&mut *held).await.unwrap();
    assert_eq!(timeout, 5000, "the writer connection has no busy timeout");

    // The second acquire cannot be served while the first is held: one connection is the queue.
    let second = tokio::time::timeout(std::time::Duration::from_millis(300), writer.acquire()).await;
    assert!(second.is_err(), "a second writer connection was handed out; writes would race");
    drop(held);
    assert!(writer.acquire().await.is_ok(), "the writer connection was not returned to the pool");
}
```

Run: `cargo test --locked --test pool`
Expected: pass, three tests.

- [ ] **Step 8: The regression test for the starvation itself**

Append to `tests/concurrency.rs`:

```rust
/// Thirty-two writers against one object, all of which must be served. This is the shape that
/// used to fail: SQLite's busy handler retries on a backoff rather than queueing, so a writer
/// could lose every retry for five seconds and answer 500 while the others made progress. It
/// takes the writer pool to make the queue real, so this is a regression test rather than a
/// test that fails on demand -- to watch the old failure, set `WRITE_WAIT` in `src/db.rs` to
/// 20 milliseconds, revert this task's `begin_write`, and run it under `taskset -c 0`.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn every_one_of_many_concurrent_writers_is_served() {
    let app = std::sync::Arc::new(common::spawn_with(|c| c.db_pool_size = Some(4)).await);
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = object["id"].as_i64().unwrap();

    let writers: Vec<_> = (0..32)
        .map(|i| {
            let app = app.clone();
            tokio::spawn(async move {
                let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
                    .json(&serde_json::json!({
                        "date": "2026-01-01", "category": "maintenance",
                        "title": format!("entry {i}"), "notes": ""
                    }))
                    .send().await.unwrap();
                let status = res.status();
                (status, res.text().await.unwrap())
            })
        })
        .collect();

    for w in writers {
        let (status, body) = w.await.unwrap();
        assert_eq!(status, 201, "a writer was refused rather than queued: {body}");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM activities")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(count, 32);
}
```

Run: `cargo test --locked --test concurrency every_one_of_many`
Expected: pass, 32 creates, no 500.

- [ ] **Step 9: Full gate and commit**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`, then the
PostgreSQL suite from Global Constraints.
Expected: green on both backends.

```bash
git add src tests
git commit -m "fix: give SQLite writes a queue instead of a race" \
  -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JaziF6pYRCjHuK5MdcdY2Z"
```

---

### Task 2: A lost write lock is a retryable answer

**Files:**
- Modify: `src/error.rs` (`status_and_code` at :42-61, `IntoResponse` at :63-79)
- Test: `tests/concurrency.rs`

**Interfaces:**
- Consumes: `db::begin_write(&state)` and the writer pool's acquire timeout from Task 1.
- Produces: nothing later tasks rely on.

- [ ] **Step 1: Write the failing test**

Append to `tests/concurrency.rs`:

```rust
/// A write that cannot take the lock is not an internal error: the request was fine, the
/// database was busy. It answers 503 with `Retry-After`, which the bundled client already
/// treats as "unknown, queue it and replay" and a third-party client can act on.
///
/// Deterministic because the writer pool has one connection: holding it means the next write
/// cannot be served, and it gives up after `WRITE_WAIT`. That wait is what this test costs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_that_cannot_take_the_lock_asks_the_client_to_retry() {
    let app = common::spawn_with(|c| c.db_pool_size = Some(4)).await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = object["id"].as_i64().unwrap();

    let held = logb::db::begin_write(&app.state).await.unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&serde_json::json!({
            "date": "2026-01-01", "category": "maintenance", "title": "blocked", "notes": ""
        }))
        .send().await.unwrap();
    let status = res.status();
    let retry = res.headers().get("retry-after").and_then(|v| v.to_str().ok()).map(str::to_owned);
    let body = res.text().await.unwrap();
    drop(held);

    assert_eq!(status, 503, "{body}");
    assert_eq!(retry.as_deref(), Some("1"), "no Retry-After to act on: {body}");
    assert!(!body.contains("database is locked"), "the driver's text reached the client: {body}");
}
```

Run: `cargo test --locked --test concurrency a_write_that_cannot`
Expected: FAIL with `left: 500, right: 503`, after about five seconds. Record the body it
printed; it is the message this task replaces.

- [ ] **Step 2: Classify it**

In `src/error.rs`, above `impl AppError`:

```rust
/// Whether this is the database saying "not now" rather than "no". Both mean the request never
/// ran and sending it again is the right thing to do.
///
/// `PoolTimedOut` is a writer that waited out `db::WRITE_WAIT` for the one writer connection.
/// SQLite codes 5 and 6 are `SQLITE_BUSY` and `SQLITE_LOCKED`, which a single-statement write on
/// the read pool can still meet while something long-running holds the lock -- an import, a
/// database copy, the retention purge.
fn is_write_contention(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::PoolTimedOut => true,
        sqlx::Error::Database(d) => matches!(d.code().as_deref(), Some("5") | Some("6")),
        _ => false,
    }
}
```

Add an arm to `status_and_code`, directly above the existing `AppError::Db(sqlx::Error::RowNotFound)` arm:

```rust
            AppError::Db(e) if is_write_contention(e) => {
                (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
            }
```

and rewrite `IntoResponse`:

```rust
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let busy = matches!(&self, AppError::Db(e) if is_write_contention(e));
        let message = if busy {
            // Warn, not error: nothing is broken, and the client is being asked to come back.
            tracing::warn!(error = %self, "the database was busy; asked the client to retry");
            "the database is busy, please retry".to_string()
        } else if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
            "internal error".to_string()
        } else {
            self.to_string()
        };
        let body = match self {
            AppError::InUse(count) => json!({ "error": code, "message": message, "count": count }),
            _ => json!({ "error": code, "message": message }),
        };
        if busy {
            // One second is the smallest value that means "not immediately"; a client that
            // queues writes (the bundled one does) uses its own backoff from here.
            return (status, [(axum::http::header::RETRY_AFTER, "1")], Json(body)).into_response();
        }
        (status, Json(body)).into_response()
    }
}
```

If `sqlx::error::DatabaseError` is not in scope for `d.code()`, add
`use sqlx::error::DatabaseError as _;` at the top of the file.

Run: `cargo test --locked --test concurrency a_write_that_cannot`
Expected: PASS.

- [ ] **Step 3: Confirm nothing else changed shape**

Run: `cargo test --locked`
Expected: green. In particular `tests/database_api.rs` and `tests/notifications.rs` assert on
503 answers from `AppError::Unavailable`; those are a different variant and must be unaffected.

- [ ] **Step 4: Full gate and commit**

Run: clippy, `cargo test --locked`, and the PostgreSQL suite.

```bash
git add src/error.rs tests/concurrency.rs
git commit -m "fix: answer a busy database with a retryable 503" \
  -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JaziF6pYRCjHuK5MdcdY2Z"
```

---

### Task 3: Say so in the documentation, and release

**Files:**
- Modify: `src/dialect.rs` (module doc, `write_lock` doc at :61-72)
- Modify: `README.md` (the `LOGB_DATABASE_URL` row in Configuration)
- Modify: `docs/upgrading.md`
- Modify: `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, `frontend/package-lock.json`

- [ ] **Step 1: The dialect module says what each backend does now**

In `src/dialect.rs`, replace list item 4 of the module doc:

```rust
//! 4. How a write transaction claims the right to be the only one -- `write_lock`. SQLite
//!    needs nothing in SQL: `BEGIN IMMEDIATE` takes the one write lock the database has, and
//!    writers queue for a single connection in `db::connect_writer` before they ever reach it.
//!    PostgreSQL permits concurrent writers and so has to be told not to.
```

and the first sentence of `write_lock`'s doc comment:

```rust
    /// SQLite needs nothing here: `BEGIN IMMEDIATE` already took the database's write lock,
    /// there is exactly one, and `db::connect_writer` has already made this process take its
    /// writers one at a time. PostgreSQL permits concurrent writers, which is precisely what
```

Leave the rest of that comment, from "this codebase is not written for" onward, untouched.

- [ ] **Step 2: The README's PostgreSQL row**

In the Configuration table's `LOGB_DATABASE_URL` row, replace

```
Every write also takes a global advisory lock, serialising writers the same as SQLite does today
```

with

```
Every write also takes a global advisory lock, serialising writers -- which is what SQLite does too, there by handing every write transaction one shared connection. On either database a write that waits more than five seconds for its turn is answered with 503 and a `Retry-After` rather than an error
```

Leave the rest of that row, from "-- an upload writes its thumbnail to disk inside that lock"
onward, as it is.

- [ ] **Step 3: The upgrade note**

Add to `docs/upgrading.md`, above the `## 0.16.0` heading:

```markdown
## 0.16.1: writes queue instead of racing

A write that cannot take the database's write lock now waits for it. Under heavy concurrent
writing SQLite could previously fail one writer with a 500 while serving the others; it now
queues them. A write that still cannot be served -- because a database copy, a large import or
the nightly purge is holding the lock -- answers `503` with `Retry-After: 1` instead of `500`,
which clients should retry. A SQLite instance now opens five database connections rather than
four.
```

- [ ] **Step 4: Release 0.16.1**

```bash
sed -i '3s/^version = "0.16.0"$/version = "0.16.1"/' Cargo.toml
cargo check --quiet
cd frontend && npm version 0.16.1 --no-git-tag-version && cd ..
```

Run: clippy, `cargo test --locked`, the PostgreSQL suite, and
`cd frontend && npm run check && npm test && npm run build && npx playwright test`.
Expected: all green. Playwright matters here because every write the browser makes goes through
the changed path.

```bash
git add Cargo.toml Cargo.lock frontend/package.json frontend/package-lock.json src/dialect.rs README.md docs/upgrading.md
git commit -m "chore: release 0.16.1" \
  -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JaziF6pYRCjHuK5MdcdY2Z"
```

Do not tag or push. Merging to `main` and pushing `v0.16.1` is the user's call, as it publishes
an image and a GitHub Release.

---

## Final check

After Task 3, confirm the two properties this plan exists for, on `main`'s merge candidate:

1. `cargo test --locked --test concurrency` passes every test, including the sixteen-writer
   puller test that failed on CI and the new thirty-two-writer one.
2. Run the whole backend suite pinned to two cores, which is the CI shape that exposed the
   original failure: `taskset -c 0,1 cargo test --locked`. Expected: green.
