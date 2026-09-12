# PostgreSQL, part 4: choosing it from Settings

Status: approved design, not yet implemented. Fourth of five. Depends on parts one to three.

## Problem

The first three parts make the app able to run on PostgreSQL and move the data there, but only
through a shell and an environment variable. This part puts it in Settings.

## What "live" honestly means

The original request was to switch live, without a restart. This design does not do that, and
the reason is worth stating rather than burying: swapping the pool under a running server means
in-flight requests holding connections to a database that is no longer the one of record, a
half-copied window where writes can land on either side, and a rollback path with no good
answer. The app would be at its least reliable at the exact moment it is handling a database
migration.

So Settings configures the switch and a restart applies it. In a compose deployment with
`restart: unless-stopped`, that restart is one button and a few seconds of downtime.

## The screen

Under Settings, admin only:

- **Where the data is now** — SQLite or PostgreSQL, and for PostgreSQL the host and database
  name. Never the password.
- **A connection string field**, with a **Test connection** button that connects, reports the
  server version, and says whether the database is empty or already holds LogB data.
- **Copy data and switch** — runs part three's copy against the given database, shows the
  per-table result, and on success writes the pointer described below.
- **Restart now**, shown once a switch is pending, with plain text saying the app will be
  unavailable for a few seconds and will only come back if something is supervising it.

Every step is reversible until the restart: the pointer is written last, and the SQLite database
is left exactly as it was.

## Revised during planning: the server copies its own database

This design assumed the copy would run with the server stopped, as `logb --copy-to` does. It
cannot: `copy::run` refuses a source that a server still holds, and the process doing the
copying from Settings *is* that server.

So the server copies from the pool it already has, inside the write transaction — `BEGIN
IMMEDIATE` on SQLite, the advisory lock on PostgreSQL. Writes are blocked for the duration,
which is what makes the snapshot consistent; a migration that let writes land on the old
database while copying would lose them. Both paths share one body, because two copies of this
logic would drift and the drifted one would be the one nobody ran.

## Where the pointer lives

Not in the database — it cannot live in the thing it points away from.

`/data/database.toml`, mode 0600, holding the connection URL. Settings writes it; the app reads
it at startup. `LOGB_DATABASE_URL` overrides it entirely, so an operator who sets the
environment cannot have it changed from a browser.

**This file holds a password in plaintext, beside the data.** That is the cost of configuring a
database from a web form instead of the environment, it is not hidden by any amount of file
permission, and it belongs in the README next to the feature rather than in a footnote. An
operator who does not want it sets `LOGB_DATABASE_URL` instead and the form becomes read-only.

## Handling failure

- **Test connection fails**: the error is shown as returned — wrong host, authentication
  failed, database does not exist — not as "could not connect".
- **The copy fails**: nothing is written, the app stays on SQLite, the per-table report shows
  where it stopped.
- **The restart comes back on a database that will not open**: the app logs the failure and
  exits rather than starting empty. An instance serving an empty database looks identical to one
  that has lost everything, and this project has already made that mistake once.
- **The pointer names a database that is empty**: same — refuse to start, say so. Automatically
  running the schema would turn a typo into a new blank instance.

## Security

Admin only, like the other destructive settings. The connection string is never returned to the
browser once saved, in any form; the screen shows host and database name only. The password is
redacted from every log line and every error message that could reach a response body.

## Testing

- An end-to-end test drives the screen against a real PostgreSQL: test, copy, switch, restart,
  and the data is there.
- Non-admin users are refused, and the refusal is tested.
- A saved password is never present in any API response or log line — asserted by searching the
  response bodies and the captured log output for the password, not by reading the code.
- `LOGB_DATABASE_URL` set makes the form read-only and the API refuse to write the pointer.
- The refuse-to-start paths are tested: unreachable database, and empty database.
