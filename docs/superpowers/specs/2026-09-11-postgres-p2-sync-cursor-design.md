# PostgreSQL, part 2: the sync cursor must not skip

Status: approved design, not yet implemented. Second of five. Depends on part one.

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

The ordering guarantee for `changes` on PostgreSQL, and the test that proves it. Nothing else.

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
