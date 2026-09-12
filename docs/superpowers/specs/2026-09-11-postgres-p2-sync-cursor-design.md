# PostgreSQL, part 2: what SQLite's single writer was hiding

Status: implemented.

## Problem

`migrations/0007_sync.sql:31` describes `changes.seq` as "the pull cursor: monotonic, gapless
per database, and ordered". A device pulls with `seq > cursor` and stores the highest `seq` it
saw. That is correct on SQLite for a reason the comment does not state: SQLite has exactly one
writer, so a row's `seq` order is its commit order.

PostgreSQL does not work that way. Two writers can take `seq` 4 and 5 and commit in the other
order. A phone pulling in the window between those commits sees 5, stores 5, and never sees 4
again. The entry is not lost from the server — it is silently missing from the device, for
good, with nothing reporting an error.

This is the one place where porting the SQL is not enough, and it is the failure this app can
least afford: a maintenance log that quietly forgets a service you recorded is worse than one
that refuses to save it.

## Scope

The ordering guarantee for `changes` on PostgreSQL, and every other place the code assumes one
writer. Implementing part one surfaced a second instance before this spec was revisited, so the
scope is the class of defect, not the one example.

**First-run setup can be raced.** `src/api/auth.rs:140` creates the first admin with
`INSERT … SELECT … WHERE NOT EXISTS (SELECT 1 FROM users)`. That re-check inside the statement
is atomic on SQLite because there is one writer. Under PostgreSQL's READ COMMITTED, two
concurrent setup requests both see an empty table and **both succeed**, leaving two admin
accounts — proven by the existing `concurrent_setup_creates_exactly_one_admin` test, which
passes on SQLite and fails on PostgreSQL. On an instance reachable before it is set up, that is
a way in.

The fix is the same mechanism as the cursor's: serialise the operation with a
transaction-scoped advisory lock on PostgreSQL, leaving SQLite untouched.

**The audit has been done**, during part one's final review: seventeen read-then-write sites,
of which these are unsafe without a single writer. Part two's scope is all of them.

| Site | What breaks |
|---|---|
| `src/api/auth.rs:140` | first-run setup: two concurrent setups both succeed, two admins |
| `src/api/sync.rs:95` | push idempotency is SELECT-then-INSERT on `changes`; a concurrent duplicate hits `idx_changes_user_op` and 500s instead of answering idempotently. The SAVEPOINT at `apply.rs:455` does not cover that INSERT |
| `src/sync/apply.rs:396-425` | `field_clock` read-compare-write: commit order rather than `edited_at` decides the last-write-wins winner, so the older edit can win |
| `src/sync/feed.rs:180-193` | purge's `NOT EXISTS` parent guards run as separate autocommit statements. A child created in the gap is hard-deleted by `ON DELETE CASCADE`, with no tombstone — exactly the loss that code's own comment warns about |
| `changes.seq` | the cursor, described above |

Judged safe, with reasons: `users::create`, the `client_op_id` creates and identical-bytes
uploads (a unique index plus a winner re-lookup — their `concurrent_*` tests pass on
PostgreSQL), `auth.rs:239`'s conditional `last_used_at`, `epoch::rotate`'s upsert, and
`record::cascade_*`, which are single statements.

Racy on SQLite too, so pre-existing rather than introduced by the port, and out of scope here:
`users::update` demotion, and `purge_orphan_files` blob deletion.

## Approach

Append to `changes` inside a transaction holding a transaction-scoped advisory lock, and assign
`seq` as `COALESCE(MAX(seq), 0) + 1` under that lock. The lock is released when the transaction
commits or rolls back, so assignment order is commit order, and a rolled-back write leaves no
gap.

This serialises appends to the change log. That is not a regression: SQLite serialises *all*
writes today, so PostgreSQL with a serialised change log is still strictly more concurrent than
what runs now. The rest of the schema keeps PostgreSQL's ordinary concurrency.

`MAX(seq) + 1` rather than an identity column, because an identity sequence hands out numbers
that survive a rollback — the cursor would skip a number that never existed, and "gapless" is
what `0007_sync.sql` promises. Under the lock this costs one indexed lookup per append.

Rejected: reading `xmin` to derive commit order (ties the protocol to PostgreSQL internals and
transaction ID wraparound); a logical replication slot (a second moving part, and the client
protocol would still need a total order); allowing gaps and having clients tolerate them (every
client, including ones already installed, would need to change).

## The cost, stated

Serialising writes means one write transaction at a time on PostgreSQL. That is what SQLite
does today, so nothing regresses — but two things are worth knowing rather than discovering.

`src/api/attachments.rs` writes a thumbnail to disk inside its write transaction, and import
writes blobs inside one. Under a global write lock, an upload's image decode and disk write
therefore block every other write in the instance. At household scale that is tolerable; on a
large import it is not invisible. Moving the disk work outside the transaction is the fix if it
ever matters, and it is not free — a blob written before its row is an orphan until
`purge_orphan_files` collects it.

Reads are unaffected and stay fully concurrent, which is where PostgreSQL's benefit for this
project actually lies.

## Testing

The test has to be able to fail, which means it has to actually race.

- **Concurrent writers, one puller.** Many writers append changes on separate connections while
  a puller repeatedly pulls with its cursor. Every `seq` the writers committed must be seen
  exactly once and in order. Run it enough times to be meaningful.
- **The same test must fail without the lock.** Remove the advisory lock, run it, and watch a
  puller skip. A concurrency test that has never been seen to fail proves nothing at all — this
  branch has already shipped three assertions that could not fail.
- **Rollback leaves no gap.** A transaction that appends and then fails must not consume a
  number.
- On SQLite the existing sync tests are unchanged and must stay green; the lock is
  PostgreSQL-only.

## What this deliberately does not do

It does not make the whole application safe under concurrent writers — it makes the *cursor*
safe. Field-level last-write-wins already decides conflicts between two edits of the same field,
and that logic is backend-independent and unchanged here.
