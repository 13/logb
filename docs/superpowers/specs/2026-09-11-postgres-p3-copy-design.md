# PostgreSQL, part 3: moving the data across

Status: implemented.

This spec and its plan (`docs/superpowers/plans/2026-09-12-postgres-p3-copy.md`) still describe
a `--force` flag on the refusal to copy into a populated destination. It was deliberately removed
during implementation -- there is no override, on purpose (see `NOT_EMPTY` in `src/copy.rs`) --
so both documents are left as historical record rather than edited to match. Do not go looking
for `--force`; it does not exist.

## Problem

An instance has a SQLite database with objects, activities, photographs and reminders in it.
Running on PostgreSQL means that data has to arrive there, intact, with its identifiers and its
sync state preserved.

## Scope

A command that copies a LogB database into an empty PostgreSQL one and proves it arrived:
`logb --copy-to postgres://…`. It reads the currently configured database and writes the
destination.

Out of scope: switching over (part four), and any use of this as an ongoing replication
mechanism. This runs once.

## What it does

1. **Refuses to run against a destination that already holds LogB data**, unless `--force` is
   given. Copying into a populated database silently merges two histories.
2. **Applies the PostgreSQL schema** from part one if the destination is empty.
3. **Copies every table in dependency order** — users, settings, api_tokens, sessions, objects,
   activities, files, attachments, reminders, changes, field_clock — preserving primary keys, so
   every foreign key still points where it did and every device's stored identifiers still
   resolve.
4. **Resets each identity sequence** to the highest copied id. Missing this is invisible until
   the first insert after the switch fails on a duplicate key, which would be days later.
5. **Verifies**: row count per table, and a per-table checksum over the primary keys and
   `updated_at`. A mismatch fails the command with a non-zero exit and names the table.
6. **Prints what it did**, per table, so the operator sees 412 activities copied rather than a
   silent success.

The server must not be running against the source while this runs. The command says so and
checks for the SQLite lock; on a running instance it refuses rather than copying a moving
target.

## Sync state

`changes` and `field_clock` are copied, and the sync epoch is **rotated** in the destination.

Every device holds a cursor plus an epoch, and a mismatch sends it to a full bootstrap. That is
exactly what is wanted: the cursor numbers survive the copy, but a device resuming mid-stream
against a freshly copied database is not worth the risk when the alternative is one re-sync.

## Testing

- A round trip: seed a SQLite database through the real API — objects, activities, attachments,
  reminders, a completed reminder pointing at an activity — copy it, and assert every table's
  contents match, not merely its counts.
- The identity-sequence reset is proven by inserting a new row after the copy and asserting it
  succeeds and does not collide.
- The refusal path: copying into a populated database without `--force` exits non-zero and
  changes nothing.
- A verification failure is provoked deliberately — delete a row from the destination mid-copy —
  and the command must fail and name the table rather than report success.
