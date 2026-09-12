# PostgreSQL, part 5: what happens to backups

Status: implemented. Last of five. Depends on part four.

**Reading the series in order:** three of the bullets below were already done when this part
started, and were not built here. Part one needed them the day `LOGB_DATABASE_URL` first
accepted a PostgreSQL URL, because without them an instance pointed at PostgreSQL would have
failed obscurely inside `VACUUM INTO` or logged a nightly backup failure forever. So `--backup`
and `--restore` refusing with an explanation, the nightly job not running, and the startup line
arrived there. What this part built is the rest: `GET /database/backup`, the Settings section
that reports backup status honestly for both backends, and the documentation. The startup line
was also rewritten here — part one wrote it as a warning that PostgreSQL was "not a supported
configuration yet", which was true then and is not now; it states the two real properties of the
configuration instead, at INFO, and says them once.

## Problem

Backup and restore are SQLite mechanisms in this app. `db::backup_to` uses `VACUUM INTO`
(`src/db.rs:52`), and `--restore` replaces the database file and moves the old one aside. Neither
means anything on PostgreSQL, and `pg_dump` does not exist inside a `scratch` image.

Left alone, an instance switched to PostgreSQL would log a nightly backup failure forever, or
worse, appear to be taking backups that do not exist.

## Decision

**On PostgreSQL, LogB does not back up.** If you run PostgreSQL you have backup tooling for it,
and a self-hosted app inventing a second, weaker mechanism beside `pg_basebackup` and your
existing schedule serves nobody.

What matters is that this is said clearly rather than discovered:

- At startup on PostgreSQL, one log line at INFO: automatic backup is off because the database
  is PostgreSQL, and backing it up is the operator's own.
- The nightly job does not run and does not warn every night.
- `--backup` exits non-zero with that same explanation instead of failing obscurely inside
  `VACUUM INTO`.
- `--restore` exits non-zero, saying that restoring a PostgreSQL database is done with
  PostgreSQL's tools, and that `logb --copy-to` exists for moving data between databases.
- Settings shows backup status honestly: on SQLite, where snapshots go and when the last one
  was; on PostgreSQL, that LogB is not taking them.

The archive export and import (`/export`, `/import`) are backend-independent and keep working on
both. They remain the portable, self-contained copy — not a substitute for database backups, and
the README should not present them as one.

## On SQLite, nothing changes

The nightly snapshot, `--backup`, `--restore`, the verification that rejects a zero-length file:
all unchanged, all still covered by their existing tests.

## Testing

- On PostgreSQL: the nightly job does not run, `--backup` and `--restore` exit non-zero with the
  explanation, and the startup line is emitted once.
- On SQLite: the existing backup and restore tests are untouched and still pass. This is the
  test that matters most, because it is the one that catches this slice breaking the backup path
  that is actually in use.
- The Settings status is asserted for both backends.
