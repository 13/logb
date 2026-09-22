# Serialized SQLite Writes

Date: 2026-09-22, against `main` at `8e4dd0d` (v0.16.0).

## The defect

A write that loses SQLite's write lock for longer than the five-second busy timeout answers
`500 {"error":"internal","message":"internal error"}`. SQLite's busy handler is not a queue: it
retries on a backoff, and a writer can lose every retry while others take the lock, so under
enough concurrent writers one is starved past the deadline.

Reproduced deliberately: set the busy timeout in `src/db.rs:155` from `5_000` to `20`, then run
`taskset -c 0 cargo test --locked --test concurrency a_puller_sees`. The server log shows

```
ERROR ... POST /api/objects/1/activities: request failed
  error=error returned from database: (code: 5) database is locked
```

which is the exact failure `a_puller_sees_every_change_exactly_once_under_concurrent_writers`
hit on CI at 14:12 on 2026-09-22, on a commit whose write path had not changed. That test drives
sixteen concurrent writers on purpose; a two-core runner is enough to starve one of them.

The bundled web client survives this for the writes it queues, because `isRejection` in
`frontend/src/lib/api-error.ts` treats only a 4xx as a refusal and replays everything else
against the same `client_op_id`. A write outside the outbox, a sync push, or any third-party API
client sees the 500.

## Design

### A second pool, of one connection, for writes

`AppState` gains `write_db: AnyPool`.

- **SQLite:** a second pool on the same file, `max_connections(1)`, the same after-connect
  pragmas, and an explicit acquire timeout. One connection means one writer; the rest wait in
  `pool.acquire()`, which is a queue, instead of racing the file lock.
- **PostgreSQL:** `write_db` is a clone of `db` (the same pool). Writers are already serialized
  by `pg_advisory_xact_lock(4479001)`, there is no file lock to lose, and a one-connection pool
  would add a second queue in front of the advisory lock for no gain. This change is
  SQLite-only, and `src/dialect.rs` is where that asymmetry gets written down.

The writer pool is built without running migrations or seeding: `db::connect_with_pool_size`
keeps doing that once, for `db`, and a new `db::connect_writer(url)` opens the second pool
against a database that is already migrated.

### `begin_write` takes the state

`db::begin_write(pool, backend)` becomes `db::begin_write(state: &App)`, using `state.write_db`
and `state.backend`. The existing two-argument form survives as `db::begin_write_on(pool,
backend)` for the two callers that are not the server's own database:

- `copy::copy_from`, which writes to the destination database through its own pool.
- Tests that take a write lock by hand to prove that something else blocks.

`copy::run_live` changes from `(db: &AnyPool, backend, dest_url)` to taking `&App`, so the source
lock it holds for the whole copy is the writer connection. Otherwise a copy would hold the file
lock on a connection the queue knows nothing about, and every queued writer would wait its five
seconds and fail rather than simply waiting for the copy.

### Why a pool rather than a mutex

The connection is the lock, so nothing has to carry a guard beside the transaction through
thirty-four call sites, and `begin_write` keeps returning a plain `Transaction`. It also retires
a live hazard: with one pool of four, four concurrent writers can hold every connection, and a
handler that acquires a second one deadlocks until the acquire timeout. That is why
`src/api/reminders.rs:632-641` hoists a `stats` call out of its transaction with a comment
explaining the trap. With reads and writes on separate pools, a read taken inside a write
transaction can no longer exhaust the pool the writer came from.

The invariant that makes this safe is already in force: no `begin_write` region anywhere in
`src/` acquires a second pooled connection, and the three places that came close hoist the work
outside the transaction with comments saying why (`src/api/reminders.rs:632`,
`src/sync/feed.rs:395`, `src/api/users.rs:168`).

### The residual, and what a client is told

Forty-two single-statement writes run straight on `db` and stay there: the `last_used_at` touch
on every bearer-token request, session creation, delivery bookkeeping, settings writes. They are
one statement each, so they can only lose the lock to a transaction that holds it for seconds,
which means a copy, an import, or the retention purge. Routing them through the writer would make
every authenticated request queue behind an import, which is worse than the problem.

Instead, losing the lock stops being a 500. `AppError::status_and_code` classifies a busy or
locked database, and a writer-pool acquire timeout, as `503 Service Unavailable` with code
`unavailable` and a `Retry-After: 1` header, logged at warn rather than error. The body carries a
fixed sentence rather than the driver's text, so no database internals reach a client. The
existing `AppError::Unavailable` variant already maps to 503, so this is a new arm in the same
match rather than a new concept.

### The wait budget

The writer pool's acquire timeout is five seconds, the same number as the busy timeout it
replaces, so the longest a client waits before being told to retry does not change. Under
ordinary contention the queue drains in milliseconds; the timeout only bites while something
long-running holds the lock, which is exactly when a client should be told to come back rather
than hold a request open. An unbounded wait was considered and rejected: a request that hangs for
the length of a database copy will be killed by a proxy anyway, and a 503 with `Retry-After` says
the same thing honestly.

## What changes for a user

- A write under contention waits its turn instead of failing. This is the fix.
- A write attempted during a database copy, a large import, or the retention purge waits up to
  five seconds and then answers 503 with `Retry-After: 1`, where today it answers 500.
- A SQLite instance holds five connections instead of four: four for reads, one for writes.
- PostgreSQL behaviour is unchanged in every respect.

## Tests

Four existing tests know about the mechanism and must move with it:

- `tests/pool.rs::the_after_connect_hook_runs_on_every_pooled_connection` pins the pool at four
  connections; it gains the writer pool's single connection and asserts the pragmas on it too,
  since a writer connection without WAL or the busy timeout is the same silent loss the test
  exists to catch.
- `tests/concurrency.rs::two_write_transactions_do_not_overlap` takes two write transactions by
  hand and asserts the second cannot start while the first is open. It must exercise the writer
  pool, or it proves nothing about the new path.
- `tests/concurrency.rs::a_child_created_during_a_purge_is_not_cascade_deleted` and
  `tests/objects.rs::a_patch_that_omits_parent_id_cannot_write_back_a_stale_parent` hold a write
  lock by hand while a real request blocks on it. Both must hold the writer connection, or the
  request under test will no longer block on them and the tests will pass vacuously.

Two new tests:

- Thirty-two concurrent activity creates against a single object all answer 201. On the current
  code with a shortened busy timeout this fails; with the writer pool it cannot.
- A held writer connection makes the next write answer 503 with `Retry-After`, not 500. This one
  costs the five-second acquire timeout in wall clock, which is accepted rather than adding a
  configuration knob that exists only for tests.

## Out of scope

- Routing single-statement writes through the writer pool.
- Any change to PostgreSQL's write path, including the advisory lock.
- Batching or chunking the long transactions (import, copy, purge). They hold the lock as long as
  they hold it; this design only changes what happens to everyone else while they do.
- A configurable wait budget.

## Release

A fix on top of 0.16.0, released as 0.16.1, on a branch off `main`.
