# PostgreSQL part 2: what SQLite's single writer was hiding — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the five places that assume one writer safe on PostgreSQL, so a device cannot skip
changes, two setups cannot both succeed, and a purge cannot delete a row nobody was told about.

**Architecture:** Rather than repair five races individually, make the assumption true: on
PostgreSQL, a transaction that intends to write takes a transaction-scoped advisory lock, so
writes serialise exactly as SQLite already forces them to. Each site then gets the smaller,
specific fix it still needs — an idempotent insert, a gapless cursor, one transaction around the
purge — so nothing depends on the lock alone.

**Tech Stack:** Rust, axum, sqlx 0.9 over `AnyPool`, SQLite and PostgreSQL 16.

## Why a lock rather than five fixes

Every one of these sites is correct on SQLite for the same reason: SQLite permits one writer, so
check-and-act is atomic without saying so. PostgreSQL's MVCC removes that, and the audit found
five consequences. Fixing each in isolation leaves the *class* alive — the sixth instance gets
written next year by someone who read the surrounding code and reasonably assumed what everyone
else assumed.

Serialising writes restores the property the whole codebase is written against. The cost is
honest and worth stating: **write throughput on PostgreSQL is capped at one transaction at a
time.** That is exactly what SQLite does today, on the instance actually in use, so nothing
regresses — and reads stay fully concurrent, which is where PostgreSQL's benefit for this
project lies.

The specific fixes still land on top, because a lock that is ever forgotten should not be the
only thing standing between a user and a lost write.

## The five sites, from the audit

| Site | What breaks without one writer |
|---|---|
| `src/api/auth.rs:137` | two concurrent setups both insert: two admins |
| `src/api/sync.rs:95` | idempotency is SELECT-then-INSERT; a concurrent duplicate hits `idx_changes_user_op` and 500s |
| `src/sync/apply.rs:398` | `field_clock` read-compare-write: commit order, not `edited_at`, picks the last-write-wins winner |
| `src/sync/feed.rs:178` | purge's `NOT EXISTS` guards are separate autocommit statements; a child created in the gap is cascade-deleted with no tombstone |
| `changes.seq` | the pull cursor: a device can read past an uncommitted number and never see it |

## Global Constraints

- No behaviour change on SQLite. 311 backend tests pass unchanged at every task boundary.
- PostgreSQL: 310 passing today with `concurrent_setup_creates_exactly_one_admin` excluded. That
  exclusion is removed in Task 3 and must never be reinstated.
- No new dependency.
- `cargo clippy --all-targets --locked -- -D warnings` clean.
- Every concurrency test must be shown to fail without its fix. A race that has never been
  observed failing proves nothing.
- Run verification in the FOREGROUND. Background runs die when a subagent's turn ends; two
  agents on the previous part stalled that way.

## Running PostgreSQL

```bash
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16
until docker exec logb-pg pg_isready -q; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test
docker rm -f logb-pg
```

A fixed `sleep` is not enough; wait for `pg_isready`. The harness opens a pool of 2 per test app,
so a stock container suffices.

## File structure

| File | Responsibility |
|---|---|
| `src/dialect.rs` | The lock statement, beside the other four dialect differences |
| `src/db.rs` | `begin_write` — begin a transaction and take the lock |
| `src/api/auth.rs` | Setup runs inside that transaction |
| `src/api/sync.rs` | Idempotency becomes an insert that cannot double-apply |
| `src/sync/record.rs` | `seq` assigned gaplessly, in commit order |
| `src/sync/feed.rs` | Purge runs as one transaction |
| `tests/concurrency.rs` (new) | The races, run against whichever backend is configured |

---

### Task 1: One writer at a time

**Files:**
- Modify: `src/dialect.rs`, `src/db.rs`
- Test: `tests/concurrency.rs` (new)

**Interfaces:**
- Produces: `db::begin_write(pool: &AnyPool, backend: Backend) -> Result<Transaction<'static, Any>, sqlx::Error>` — begins with `backend.begin_write()` and, on PostgreSQL, takes the advisory lock before returning.
- Produces: `Backend::write_lock(&self) -> Option<&'static str>`.

- [ ] **Step 1: Write the failing test**

Create `tests/concurrency.rs`:

```rust
//! The races that SQLite's single writer hid.
//!
//! Every test here must be able to fail: each one is paired with a note saying what to remove
//! to see it red, and the task that added it proved it. A concurrency test that has never been
//! observed failing is decoration.

mod common;

use std::time::Duration;

/// Two write transactions must not overlap. On SQLite `BEGIN IMMEDIATE` already guarantees it;
/// on PostgreSQL the advisory lock does. Remove `Backend::write_lock`'s PostgreSQL arm and this
/// fails on PostgreSQL while still passing on SQLite -- which is the whole point of it.
#[tokio::test]
async fn two_write_transactions_do_not_overlap() {
    let app = common::spawn().await;
    let a = logb::db::begin_write(&app.state.db, app.state.backend).await.unwrap();

    // The second one must not be able to start while the first is open.
    let blocked = tokio::time::timeout(
        Duration::from_millis(750),
        logb::db::begin_write(&app.state.db, app.state.backend),
    )
    .await;
    assert!(blocked.is_err(), "a second write transaction began while the first was still open");

    drop(a);
    // And it must proceed once the first is done, rather than deadlocking forever.
    let after = tokio::time::timeout(
        Duration::from_secs(5),
        logb::db::begin_write(&app.state.db, app.state.backend),
    )
    .await;
    assert!(after.is_ok(), "the lock was not released when the transaction ended");
}
```

`common::spawn()` must expose `state` — check `tests/common/mod.rs`; if `TestApp` does not
already carry the `App` state, add it following that file's existing style.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test --test concurrency`

Expected: compilation failure — `db::begin_write` does not exist.

- [ ] **Step 3: Add the lock**

In `src/dialect.rs`, beside the other differences:

```rust
    /// How a write transaction claims the right to be the only one.
    ///
    /// SQLite needs nothing here: `BEGIN IMMEDIATE` already took the database's write lock, and
    /// there is exactly one. PostgreSQL permits concurrent writers, which is precisely what
    /// this codebase is not written for -- the audit in part two's spec found five places where
    /// check-then-act is atomic only because SQLite serialises writers.
    ///
    /// The key is arbitrary but must never change: it names this application's write lock, and
    /// a different value would let two versions of LogB write concurrently against one
    /// database. `pg_advisory_xact_lock` releases at commit or rollback, including a rollback
    /// nobody wrote -- a dropped transaction, a panic, a killed connection -- which is why it
    /// is the transaction-scoped form rather than the session one.
    pub fn write_lock(&self) -> Option<&'static str> {
        match self {
            Self::Sqlite => None,
            Self::Postgres => Some("SELECT pg_advisory_xact_lock(4479001)"),
        }
    }
```

In `src/db.rs`:

```rust
/// Begins a transaction that intends to write, and makes it the only one.
///
/// Every write path goes through here. See `dialect::Backend::write_lock` for why PostgreSQL
/// needs more than a `BEGIN`.
pub async fn begin_write(
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

- [ ] **Step 4: Run both backends**

Run the SQLite suite, then the PostgreSQL suite with the commands under **Running PostgreSQL**.

Expected: 312 on SQLite (311 plus the new test), 311 on PostgreSQL with the setup race still
excluded.

- [ ] **Step 5: Prove the test can fail**

Remove the PostgreSQL arm of `write_lock` (return `None` for both), run `tests/concurrency.rs`
against PostgreSQL, and confirm it goes red. Restore it and confirm green. Report both outputs.

- [ ] **Step 6: Commit**

```bash
git add src/dialect.rs src/db.rs tests/concurrency.rs tests/common/mod.rs
git commit -m "feat: on PostgreSQL, one write transaction at a time"
```

---

### Task 2: A cursor that cannot skip

**Files:**
- Modify: `src/sync/record.rs:95`, `migrations/postgres/0001_schema.sql:161`
- Test: `tests/concurrency.rs`

**Interfaces:**
- Consumes: `db::begin_write` from Task 1.

- [ ] **Step 1: Write the failing test**

Append to `tests/concurrency.rs`:

```rust
/// The cursor's promise, from `0007_sync.sql`: "monotonic, gapless per database, and ordered".
/// A device pulls with `seq > cursor` and remembers the highest it saw, so a number that
/// appears after the device has moved past it is not late -- it is gone, for that device,
/// permanently and with nothing reporting an error.
///
/// Remove the `MAX(seq) + 1` assignment (let the identity column supply it) and this fails on
/// PostgreSQL: the writers commit out of order and the puller walks past a number that has not
/// landed yet.
#[tokio::test]
async fn a_puller_sees_every_change_exactly_once_under_concurrent_writers() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;

    // Writers push activities concurrently; a puller follows the log with its own cursor.
    let writers = (0..8).map(|i| {
        let app = app.clone();
        let object = object["id"].clone();
        tokio::spawn(async move {
            for n in 0..5 {
                app.create_activity(&object, &format!("entry {i}-{n}")).await;
            }
        })
    });

    let puller = {
        let app = app.clone();
        tokio::spawn(async move {
            let mut cursor = 0i64;
            let mut seen = Vec::new();
            for _ in 0..60 {
                let page = app.pull(cursor).await;
                for change in page["changes"].as_array().unwrap() {
                    let seq = change["seq"].as_i64().unwrap();
                    seen.push(seq);
                    cursor = cursor.max(seq);
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            seen
        })
    };

    for w in writers {
        w.await.unwrap();
    }
    let seen = puller.await.unwrap();

    // Everything the log holds must have reached the puller, once each, in order.
    let total: i64 = app.count_changes().await;
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(seen, sorted, "the puller saw a seq out of order or twice: {seen:?}");
    assert_eq!(
        sorted.len() as i64, total,
        "the puller saw {} of {total} changes -- one was skipped permanently", sorted.len(),
    );
}
```

Add `pull`, `count_changes` and `create_activity` helpers to `tests/common/mod.rs` if they are
missing, following that file's existing style. `TestApp` must be `Clone` for the spawns; if it
is not, wrap what the tasks need in an `Arc` rather than making the whole struct `Clone` by
force.

- [ ] **Step 2: Run it and watch it fail on PostgreSQL**

Run it against PostgreSQL. Expected: it fails, reporting a skipped or out-of-order `seq`. If it
passes, the race did not occur — raise the writer count and the iterations until it does, and
report what it took. A race you cannot make fail is a race you cannot prove you fixed.

On SQLite it must pass unchanged.

- [ ] **Step 3: Assign `seq` explicitly**

In `src/sync/record.rs:95`, name `seq` in the insert and take it from the log itself:

```rust
        // `seq` is assigned here rather than by the column's default, and the difference
        // matters on PostgreSQL. An identity column hands out numbers before commit and keeps
        // the number even if the transaction rolls back, so the log would have gaps and, worse,
        // numbers could commit out of order -- a puller that read past one would never see it.
        // Inside `db::begin_write`'s lock this is the only transaction writing, so `MAX + 1`
        // is exactly the next number, and it is gapless because a rollback takes it with it.
        "INSERT INTO changes \
         (seq, entity, entity_uuid, op, field, value, edited_at, applied_at, user_id, \
          device_id, client_op_id) \
         VALUES ((SELECT COALESCE(MAX(seq), 0) + 1 FROM changes), $1, $2, $3, $4, $5, $6, $7, \
                 $8, $9, $10)"
```

Leave the PostgreSQL schema's `GENERATED BY DEFAULT AS IDENTITY` in place — `BY DEFAULT` means
an explicit value is accepted, and keeping the identity means a future insert that forgets `seq`
still gets a number rather than violating NOT NULL. Update the comment above that column to say
the application assigns it and why.

- [ ] **Step 4: Run both backends**

Expected: the new test passes on both; the whole suite passes on both.

- [ ] **Step 5: Prove the fix is what fixed it**

Revert Step 3, run the test against PostgreSQL, watch it fail, restore. Report both outputs.

- [ ] **Step 6: Commit**

```bash
git add src/sync/record.rs migrations/postgres/0001_schema.sql tests/concurrency.rs tests/common/mod.rs
git commit -m "fix: assign the cursor's seq in commit order, gaplessly"
```

---

### Task 3: One admin

**Files:**
- Modify: `src/api/auth.rs:128-148`
- Modify: `.github/workflows/ci.yml` — remove the exclusion
- Test: `tests/auth.rs` (the existing `concurrent_setup_creates_exactly_one_admin`)

**Interfaces:**
- Consumes: `db::begin_write` from Task 1.

- [ ] **Step 1: Watch the existing test fail on PostgreSQL**

Run `cargo test --test auth` against PostgreSQL, without the `--skip`.

Expected: `concurrent_setup_creates_exactly_one_admin` fails — it races the setup five times and
reports two admins. Record the output; that is the baseline this task removes.

- [ ] **Step 2: Run setup inside a write transaction**

In `src/api/auth.rs`, replace the pre-check-and-insert with a single write transaction: begin
with `db::begin_write`, do the `user_count` check and the conditional insert inside it, create
the session, and commit.

Keep the `WHERE NOT EXISTS` clause. It is no longer the only guard, but it costs nothing and
states the intent inside the statement that depends on it.

- [ ] **Step 3: Run both backends**

Expected: `concurrent_setup_creates_exactly_one_admin` now passes on **both**, with no `--skip`.

- [ ] **Step 4: Remove the CI exclusion**

In `.github/workflows/ci.yml`, drop `-- --skip concurrent_setup_creates_exactly_one_admin` and
the comment block explaining it. The PostgreSQL job now runs the whole suite.

- [ ] **Step 5: Commit**

```bash
git add src/api/auth.rs .github/workflows/ci.yml
git commit -m "fix: two setup requests cannot both create an admin"
```

---

### Task 4: Idempotency that cannot double-apply

**Files:**
- Modify: `src/api/sync.rs:88-105`, `src/sync/record.rs`
- Test: `tests/concurrency.rs`

**Interfaces:**
- Consumes: `db::begin_write` from Task 1.

- [ ] **Step 1: Write the failing test**

Append to `tests/concurrency.rs`:

```rust
/// A client that retries a push it never saw the answer to sends the same op ids again. Two
/// such attempts can arrive at once -- a flaky connection retrying while the first is still in
/// flight. Both must be answered `accepted`, and the write must happen once.
///
/// Before the fix this raced: idempotency was SELECT-then-INSERT, so both attempts read "not
/// seen", both inserted, and the second hit `idx_changes_user_op` and turned the whole batch
/// into a 500 -- a client that retries forever, forever.
#[tokio::test]
async fn a_push_retried_concurrently_is_applied_once_and_accepted_twice() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    let ops = app.one_set_op(&object, "Golf VII", "op-retry-1").await;

    let (a, b) = tokio::join!(app.push_raw(&ops), app.push_raw(&ops));
    for (which, res) in [("first", a), ("second", b)] {
        assert_eq!(res.status(), 200, "the {which} attempt did not answer 200");
    }
    assert_eq!(app.count_changes().await, 1, "the op was recorded more than once");
}
```

`one_set_op` builds a push body with a single `set` op and the given `client_op_id`; `push_raw`
posts a prepared body. Add both to `tests/common/mod.rs` following its existing style.

- [ ] **Step 2: Run it and watch it fail on PostgreSQL**

Expected: a 500 from the losing attempt, or two rows in `changes`. If neither happens, increase
the number of concurrent attempts until it does and report what it took.

- [ ] **Step 3: Make the insert decide**

Change the log insert in `src/sync/record.rs` to `ON CONFLICT (user_id, client_op_id) DO NOTHING`
and report whether it inserted. When it did not, the op was already applied: the caller answers
`Accepted` without applying it again — the same answer the SELECT was there to produce, now
decided by the unique index rather than by a read that can go stale.

Keep the existing SELECT as a fast path if it reads naturally, but the insert's result is what
decides. Say in a comment that the index is the authority.

- [ ] **Step 4: Run both backends**

Expected: the new test passes on both; the whole suite passes on both; the existing
`client_op_id` idempotency tests still pass unchanged.

- [ ] **Step 5: Prove the fix is what fixed it**

Revert Step 3, run the new test against PostgreSQL, watch it fail, restore. Report both outputs.

- [ ] **Step 6: Commit**

```bash
git add src/api/sync.rs src/sync/record.rs tests/concurrency.rs tests/common/mod.rs
git commit -m "fix: let the unique index decide whether an op was already applied"
```

---

### Task 5: The purge cannot delete what nobody was told about

**Files:**
- Modify: `src/sync/feed.rs:160-195`
- Test: `tests/concurrency.rs`

**Interfaces:**
- Consumes: `db::begin_write` from Task 1.

- [ ] **Step 1: Write the failing test**

Append to `tests/concurrency.rs`:

```rust
/// The purge deletes aged-out tombstones, guarded by `NOT EXISTS` checks that say "only if this
/// parent has no children left". Those guards ran as separate autocommit statements, so a child
/// created between the guard and the delete was taken by `ON DELETE CASCADE` -- hard-deleted,
/// with no tombstone and no log row, so no device would ever learn it existed. That is the
/// exact loss the code's own comment says it exists to prevent.
#[tokio::test]
async fn a_child_created_during_a_purge_is_not_cascade_deleted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    app.delete_object(&object).await;
    app.age_out_tombstones().await;

    // A device that was offline pushes an activity against the object while the purge runs.
    let purge = { let app = app.clone(); tokio::spawn(async move { app.run_purge().await }) };
    let write = { let app = app.clone(); let o = object.clone();
                  tokio::spawn(async move { app.create_activity(&o["id"], "late arrival").await }) };
    let (_, _) = tokio::join!(purge, write);

    // Either the object survived with its child, or both are gone with tombstones that a device
    // can still learn about. What must never happen is a row vanishing unrecorded.
    let orphans: i64 = app.count_orphan_activities().await;
    assert_eq!(orphans, 0, "an activity outlived its object, or was deleted without a tombstone");
}
```

Add `delete_object`, `age_out_tombstones`, `run_purge` and `count_orphan_activities` to
`tests/common/mod.rs`. `age_out_tombstones` backdates `deleted_at` past the purge window
directly in the database — the window is measured in days and a test cannot wait.

- [ ] **Step 2: Run it and watch it fail on PostgreSQL**

Expected: an orphaned or silently-deleted row. Report what you observed. If the race will not
reproduce, say so plainly and describe what you tried rather than weakening the assertion.

- [ ] **Step 3: Purge in one transaction**

Wrap the guard loop in `src/sync/feed.rs` in a single `db::begin_write` transaction and execute
each `DELETE` against it, committing at the end. The guards and their deletes then see one
consistent state, and on PostgreSQL no other write can interleave.

- [ ] **Step 4: Run both backends**

Expected: the new test passes on both; the existing purge and horizon tests pass unchanged —
those are the ones that prove this task did not change what the purge deletes.

- [ ] **Step 5: Commit**

```bash
git add src/sync/feed.rs tests/concurrency.rs tests/common/mod.rs
git commit -m "fix: run the purge's guards and deletes in one transaction"
```

---

### Task 6: The last-write-wins winner is decided by time, not by luck

**Files:**
- Modify: `src/sync/apply.rs:396-425` (comments only, if the lock already settles it)
- Test: `tests/concurrency.rs`

**Interfaces:**
- Consumes: `db::begin_write` from Task 1.

- [ ] **Step 1: Write the failing test**

Append to `tests/concurrency.rs`:

```rust
/// Field-level last-write-wins compares `edited_at`, falling back to `device_id` for a tie. The
/// comparison reads `field_clock`, decides in Rust, then writes -- which is only atomic if
/// nothing else can write between the read and the write.
///
/// Two devices editing the same field at once must leave the field holding the LATER edit,
/// whichever arrived first. Without serialised writes the winner is whichever transaction
/// committed last, so the older edit can win and stay.
#[tokio::test]
async fn the_later_edit_wins_regardless_of_arrival_order() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;

    let older = app.one_set_op_at(&object, "older name", "op-older", "2026-01-01T00:00:00Z").await;
    let newer = app.one_set_op_at(&object, "newer name", "op-newer", "2026-06-01T00:00:00Z").await;

    let (_, _) = tokio::join!(app.push_raw(&newer), app.push_raw(&older));

    let name = app.object_name(&object).await;
    assert_eq!(name, "newer name", "the older edit won: the clock was read before it was safe to");
}
```

`one_set_op_at` is `one_set_op` with an explicit `edited_at`.

- [ ] **Step 2: Run it on both backends**

It may already pass, because Task 1 serialises writes. **Report honestly which it is**, and then
prove the claim: remove the PostgreSQL arm of `write_lock`, run it against PostgreSQL, and see
whether it fails. If it does, the lock is what makes this correct and the comments at
`apply.rs:396` must say so. If it does not, this race needs its own fix — describe what you
found rather than declaring victory.

- [ ] **Step 3: Record what makes it safe**

Update the comment above the `field_clock` read to state plainly that the read-compare-write is
atomic only because writes are serialised, and to name `db::begin_write` as what provides that.
A future reader deciding whether they may skip the lock needs to find this.

- [ ] **Step 4: Commit**

```bash
git add src/sync/apply.rs tests/concurrency.rs tests/common/mod.rs
git commit -m "test: pin that the later edit wins under concurrent pushes"
```

---

### Task 7: Say what is true now

**Files:**
- Modify: `README.md` (the `LOGB_DATABASE_URL` row), `src/lib.rs` (the startup warning)
- Modify: `docs/superpowers/specs/2026-09-11-postgres-p2-sync-cursor-design.md` (status line)

- [ ] **Step 1: Narrow the warning to what remains**

The startup warning and the README currently say PostgreSQL is unsupported because of the setup
race, the cursor, and the lack of automatic backups. After this part, the first two are fixed.

What remains true: LogB takes no automatic backups on PostgreSQL, and there is no supported way
to move an existing SQLite database across yet — that is part three (`logb --copy-to`). Rewrite
both to say exactly that, naming the parts that are still outstanding.

Do not declare PostgreSQL supported. Parts three through five are unbuilt, and a configuration
with no backup path and no migration path is not one to recommend.

- [ ] **Step 2: Verify the warning**

Start the built binary against a PostgreSQL URL and paste the line it logs.

- [ ] **Step 3: Mark the spec implemented**

Change the spec's status line to `Status: implemented.`

- [ ] **Step 4: Commit**

```bash
git add README.md src/lib.rs docs/superpowers/specs/2026-09-11-postgres-p2-sync-cursor-design.md
git commit -m "docs: what is still unsafe about PostgreSQL, and what no longer is"
```

---

## When this is done

The five sites the audit named are safe, each with a test that was watched failing first.
PostgreSQL runs the whole suite in CI with nothing excluded. It is still not a supported
configuration: there is no backup story and no way to bring an existing database across. Parts
three through five cover those.
