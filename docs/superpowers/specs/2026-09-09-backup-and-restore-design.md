# Automated backup, a real restore, and a health check that tells the truth

Status: approved design, not yet implemented.

## Problem

logb holds records that cannot be reconstructed — a decade of a car's maintenance history is
not something anyone can retype. It has no automated backup. `--backup` exists and works, but
runs only when a human remembers, and the README has never contained the word "restore": there
is no documented, let alone tested, path from a snapshot back to a running instance.

Two things make this worse than it looks:

**Restore is now dangerous in a way it was not before sync.** `changes.seq` is an autoincrement
cursor. A restored database reissues those numbers for entirely different ops, so a phone
holding cursor 500 would pull ops 501+ that are not the edits it missed. It would apply the
wrong history, silently, with nothing anywhere reporting an error.

**Nothing notices when the instance is broken.** `/api/health` returns a static literal. During
this project's own rename, Docker reported the container healthy for nineteen straight hours
while it ran against a database with no tables, logging `no such table: sessions` every hour.

## Scope

In scope: a scheduled verified snapshot with retention, a `--restore` command, a sync epoch that
makes restore safe for devices, and a health check that touches the database.

Out of scope: moving backups off the machine. logb's job is to produce good snapshots in a
directory the operator chooses; getting that directory onto other hardware is the operator's,
and any file-level tool does it better than an app could.

Blobs are also out of scope, by construction rather than by omission: files under `files/` are
content-addressed and never rewritten, so any file-level sync covers them and re-archiving them
nightly would copy gigabytes of provably unchanged bytes.

## Backup

Off unless `LOGB_BACKUP_DIR` is set, so an existing deployment behaves exactly as it does now
until its operator opts in. `LOGB_BACKUP_HOUR` (default 3) picks the hour.

The job runs from the existing hourly block in `src/tasks.rs`, once per day, taking the hour as a
parameter the way `notify::tick` already does — that is what makes it testable in a second
rather than a day.

Each run writes `logb-YYYY-MM-DD.db` through the existing `db::backup_to`, which is SQLite's
`VACUUM INTO` and therefore safe against a live instance. It then **opens the finished file and
runs `PRAGMA integrity_check`**. Only a snapshot that passes counts: a failure logs at error,
removes the bad file, and leaves the previous night's snapshot in place. A backup nobody has
opened is a guess, and this is the cheapest possible proof.

Retention keeps the newest 14 and deletes the rest. If today's file already exists and verifies,
the run is skipped, so restarts do not re-snapshot.

## The sync epoch

A `sync_epoch` row in `settings`, seeded by migration with a fresh UUID.

`GET /api/sync/pull` and `GET /api/sync/bootstrap` both return it. The rule:

> A pull with `since > 0` and a missing or mismatched `epoch` is answered `410`.

A first pull (`since = 0`) carries no epoch and is always legal. `410` already means "your cursor
is meaningless, re-bootstrap" for the stale-horizon case, so this reuses a path that exists and
is tested rather than adding a second thing a client must handle.

`--restore` stamps a new epoch. That is the whole reason restore is code rather than a README
paragraph: a manual file swap cannot stamp anything, and every device would resume on a reused
cursor with no warning.

## Restore

```
logb --restore <snapshot>
```

Order matters, because each step protects the one after it:

1. Validate the source read-only — `integrity_check`, and confirm it is actually a logb
   database (`_sqlx_migrations` present) rather than an unrelated file. Nothing is touched until
   this passes.
2. Move the live database aside as `logb.db.replaced-<timestamp>`, not delete it. A restore is
   destructive and operators do restore the wrong file; the previous state stays recoverable.
3. Copy the snapshot into place.
4. Run migrations, since a snapshot may predate the running binary.
5. Stamp a fresh `sync_epoch`.
6. Report what happened: which file was restored, where the replaced database went, and that
   every device will re-bootstrap.

The server must be stopped. SQLite's own locking is the check, so a restore attempted against a
live instance fails cleanly instead of corrupting it.

Under Docker the service is down, so the command runs in a one-shot container against the same
volume:

```bash
docker compose stop logb
docker compose run --rm logb /logb --restore /data/backups/logb-2026-09-01.db
docker compose start logb
```

The README gains that as a Restore section, next to the Backup one it already has.

## Health check

`/api/health` asserts the database is present and correct: that the schema exists and the applied
migration count matches what the binary expects. On failure it answers `503` with a reason
naming what is wrong.

The container's `HEALTHCHECK` already treats any non-2xx as failure, so Docker begins telling the
truth with no change to the Dockerfile. This is the piece that turns the nineteen-hour silence
into a first-minute failure.

## Testing

- **Backup**: a run creates and verifies a snapshot; a deliberately corrupt file is rejected and
  the previous night's snapshot survives; retention keeps exactly 14 and deletes the oldest; a
  second run on the same day is a no-op.
- **Restore**: snapshot a populated database, mutate it, restore, and assert the data is back,
  the replaced database is preserved, and the epoch changed.
- **Epoch**: a pull with `since > 0` and a stale epoch is `410`; with the current epoch it is
  `200`; `since = 0` needs none.
- **Health**: a healthy instance returns 200; one whose schema is missing returns 503.

Backup and restore tests use `tempfile::TempDir` like the rest of `tests/`, so nothing touches a
real data directory.

## Interaction with the sync work

This closes the "Known interaction" recorded in
`docs/superpowers/specs/2026-09-08-offline-sync-design.md`: restoring a server snapshot silently
rolls back writes that phones believe were accepted. The epoch is the resolution — a restored
server advertises a new one, and every device reconciles from a fresh bootstrap rather than
resuming from a cursor whose meaning has changed underneath it.
