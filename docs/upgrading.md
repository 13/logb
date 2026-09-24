# Upgrading

A new image applies any pending database migrations when it starts. Take a snapshot first
(README → Backup) whenever a release below says so.

## 0.16.1: writes queue instead of racing

A write that cannot take the database's write lock now waits for it. Under heavy concurrent
writing SQLite could previously fail one writer with a 500 while serving the others; it now
queues them. On SQLite, a write that still cannot be served within five seconds -- because a
database copy, a large import or the nightly purge is holding the lock -- answers `503` with
`Retry-After: 1` instead of `500`, which clients should retry; a SQLite instance now opens five
database connections rather than four. PostgreSQL writers already serialized on an advisory
lock and simply wait their turn on it, with no five-second budget and no `503`.

## 0.16.0: per-user today

A person who has chosen their own timezone under Settings → Notifications now gets the daily
digest for their own day, and "sent today" is counted in that calendar. On the first delivery
after this upgrade their existing delivery record still carries the instance's day, so they may
receive one extra digest that day. It corrects itself from the next day on.

## 0.13.0: body weight

Weight and unit preferences are included in sync and export/import. Older archives
remain importable and default to kg. The SQLite upgrade rebuilds the activities table
while preserving its data and references; take a database backup before upgrading.

## 0.3.0: object types

This release replaces an object's free-text category with a fixed type. The migration maps
known words in both languages (`Auto` → car, `Fahrrad` → bike, `Pedelec` → e-bike); anything it
does not recognise becomes **Other**, and the text you had typed is appended to that object's
description so nothing is lost. A backup archive made before this release imports the same
way — unmapped words land on Other with the original text preserved in the description, so
restoring a year-old export does not lose what each object was.

It also rotates the sync epoch, so every device does one full re-sync on its next connection.
That is expected, not a fault — it is how each device learns the new field.

**Take a backup before upgrading — this one is not optional.** The migration rebuilds two
tables, and `LOGB_BACKUP_DIR` is unset on a default install, which means there is no automatic
backup to fall back on unless you set it. Take one yourself first:

```bash
docker compose exec logb /logb --backup /data/snapshot.db
```

The migration also runs outside a transaction. SQLite refuses to toggle `PRAGMA foreign_keys`
inside one, and without turning it off, `DROP TABLE objects` would cascade and delete every
attachment along with it. The cost of that is that the migration is not atomic with its own
bookkeeping row in `_sqlx_migrations`: a crash in the narrow window after the rebuild finishes
but before that row is written leaves the migration applied but unrecorded, and the next boot
tries to run it again and fails on tables that already exist. Recovering from that is a manual
insert of the one missing row (`version` 9, `description` "object types", `success` 1,
`execution_time`, and a `checksum`) — the checksum has to be the exact SHA-384 hash of
`migrations/sqlite/0009_object_types.sql`'s contents, since sqlx compares it against the file it ships
with and refuses to start on a mismatch. This is rare and narrow, but if it happens, restoring
the backup you just took is simpler than reconstructing the row by hand.

**If you ever run this migration by hand, use `sqlite3 -bail`, never plain `sqlite3`.** The
safety of the whole thing depends on the runner stopping at the first failing statement; without
`-bail`, `sqlite3` keeps going after an error, which is exactly how the cascading delete above
would actually happen.
