# Upgrading

A new image applies any pending database migrations when it starts. Take a snapshot first
(README → Backup) whenever a release below says so.

## 0.22.1: dependency fixes

**0.22.0 was tagged but never published**, because advisories against two build-time npm
packages (`brace-expansion`, `fast-uri`, pulled in by the service-worker build tooling) appeared
the same day and the release checks refuse a known advisory. 0.22.1 is 0.22.0 with those
packages updated. Neither ships in the app. Upgrading from 0.21.0 gets everything described
under 0.22.0 below. No migration.

## 0.22.0: new look, the forms

**Every form has the same shape**: a label, the field, a hint where one helps, and an error
under the field it is about, with the cursor moved there. Units sit inside their field ("km"),
not in the label. Save and Cancel stay at the bottom of the screen (above the tab bar on a
phone), so Save is visible as soon as a form opens. Form buttons are all 48 px tall.

**Reading, reminder, object and activity forms were rebuilt** on that pattern, with new date,
tag and file inputs.

**Optional fields wait under "More details"**, which opens by itself when you edit an entry
that uses any of them: notes, tags, a trip's places, duration and battery, an object's resource
settings, "Inside", description, purchase details, Archive and Private.

**New object**: the type comes first, as a grid of icon tiles, and none is chosen for you; Save
asks for one. Templates follow under "Or start from a template", then the name.

**New reminder**: "Due by: Date / Counter / Both" says which fields the reminder watches. A
reminder due by both is due at whichever comes first. Switching sides keeps what you typed until
you save; only the side you chose is saved.

**Log activity**: one "Notes" field, smaller section headings.

Checkboxes and choices are restyled; drop-down lists stay the phone's own pickers.

**Fixes**: an object's page fetches its reminders once, and opening a page offline whose code
isn't cached yet no longer ends on the browser's error page; it waits and reloads once you are
back online.

No migration.

## 0.21.0: new look, the object page

**On a wide screen (1024 px and up) an object's page has two panes.** The left one holds the
photo, the key figures, anything due, spend per month and everything the Info tab used to show,
and it stays in view while the timeline scrolls. "+ Log" and Edit sit in the page header. There
is no Info tab at that width; an old `?tab=info` link opens the timeline beside the pane. A
"Skip to the tabs" link at the top of the pane jumps past it for keyboard users.

**On a phone** the photo, a row of figures and anything due sit above the tabs. The tabs stay on
one row, the Reminders count sits inside its tab, and Info holds the rest.

**Timeline entries** show a category icon, the amount on the right and their tags inside the
entry, under year headings; the category chips scroll sideways. Charts leave out empty months.
The Reminders and Documents tabs are restyled too; documents get icon buttons for their actions.
The Energy figures now show only for objects that charge (kWh), so a diesel car no longer shows
"Distance per charge".

No migration.

## 0.20.0: new look, the dashboard

**The dashboard is redesigned.** Due reminders are cards with a snooze button, what is coming
up is a short list, and each object's tags sit inside its card. Search, sort and the
Active/Archived switch share one row; on a wide screen the objects fill two or three columns. Dashboard reminders no longer show the object's tags.

**"+ Log" on the dashboard asks which object**, then opens the same quick entry the small "+" on
each card used to: a reading for something with a counter, a fill for a tank, a weight for a
person. The per-card "+" is gone, and "New object" moved to the page header.

No migration.

## 0.19.0: new look, first part

**The app is zinc and amber, set in Inter.** Colours, font and the navigation frame change on
every screen; layouts do not yet. The font is bundled and precached, so it works offline and
nothing is fetched from elsewhere. Further releases rework each screen in turn.

**One "+ Log" button on an object's page** replaces the separate trip, fill/charge and activity
buttons. On an object with only activities it logs an activity directly; otherwise it opens a
short menu.

No migration.

## 0.18.1: heating fuel statistic

**Statistics → household heating fuel no longer counts vehicles.** A car's or van's refuels
were listed as heating fuel. Only objects marked as heating fuel are counted now, plus older
objects that have a fuel unit and no distance counter (a tank or generator measured in hours).
No migration.

## 0.18.0: faster lists, search, imports and first load

**The first start builds six indexes.** Five cover deleted entries (only those, so they stay
small) and one covers reminders completed by an entry. The migration also removes, once,
the per-field edit clocks of rows that no longer exist; from now on they go with their rows.
Both take seconds on a household's database, and the instance answers once they are done.

**SQLite uses more memory, up to about 160 MB.** Each of its five connections may now cache
32 MB of the database instead of 2 MB, and reads go through a memory map of up to 256 MB, which
the kernel shares and can reclaim. On a small database the cache never grows past the database's
own size. PostgreSQL is unchanged.

**Responses can be compressed with brotli or zstd** as well as gzip, whichever the browser
prefers. An attachment's original is never compressed, and answers `Range` requests, so a large
PDF or video can be read in parts.

**The first start folds every object and entry for search.** Search now reads a stored,
pre-folded copy of each row's text instead of folding every row on every search. The copy is
filled at the first start, 500 rows per short transaction, before the instance answers: about a
second for a household with tens of thousands of entries. On PostgreSQL the server also tries to
create the `pg_trgm` extension and two trigram indexes; if the database role may not create
extensions, it logs one line and search scans instead, which is still fast.

**`POST /sync/push` refuses more than 1000 operations** in one request with `413 too_large`.
The bundled app never sends that many; a script that does should split its batch.

## 0.17.1: faster saves

0.17.0 was tagged but never published: its release failed before any image was pushed under a
name. Coming from 0.16.x, read this section and the 0.17.0 one below.

**A SQLite commit is no longer synced to disk on its own.** The database now runs with
`synchronous = NORMAL`, SQLite's recommendation under WAL, instead of syncing the WAL on every
commit -- which on an SD card, a NAS or a network volume was most of what a save cost. The
database still cannot be corrupted by a crash or a power cut; what a power cut can now lose is
the last few seconds of saves before it. Deleting a file still syncs first, so no entry is ever
left pointing at a photo that is gone. PostgreSQL is unchanged.

**Photos are shrunk before they are uploaded.** A JPEG, PNG, WebP or HEIC photo over 1.5 MB is
resized in the browser to 2560 px on its long edge. A resized JPEG keeps its capture date and
camera details; a resized PNG, WebP or HEIC photo loses its capture date. Smaller files, a photo
the browser cannot decode (HEIC in most browsers), and anything that is not a photo are sent as
they are.

**A slow save gives up after 10 seconds and is queued**, exactly as a save made offline is, and
the form carries on. It is sent again later, and cannot create a second entry.

## 0.17.0: hardening

**Everyone is signed out once.** The database now stores a hash of each session token instead
of the token itself, so a copy of the database -- a backup, a snapshot -- no longer holds
cookies anyone could replay. Sessions from before the upgrade cannot be converted and are
deleted when the migration runs: every browser shows the sign-in page once. API tokens are
unaffected; they were already stored hashed.

**Changing your own password asks for the current one.** `PATCH /api/users/{id}` with a new
`password` for your own account now needs `current_password` as well, and answers `403` with
`"error": "wrong_password"` when it is missing or wrong. Each such attempt counts against the
login rate limit. An admin resetting *another* user's password is unchanged.

**Behind a proxy, the last forwarded hop counts.** With `LOGB_TRUST_PROXY` set, the client
address for the login limit is now the **rightmost** `X-Forwarded-For` entry -- the one your
proxy appended -- and the pairing host the rightmost `X-Forwarded-Host` entry. Earlier entries
come from the client and are ignored. A proxy that overwrites these headers behaves exactly as
before; one that appends now works as intended. Logins are also limited per username, with the
same window and `LOGB_LOGIN_MAX_ATTEMPTS`, whatever address they come from.

**The notification test answers `sent` or `failed`.** `POST /api/me/notifications/test` no
longer returns why a webhook or Telegram delivery failed; the reason is in the server log.
Webhook and push deliveries no longer follow redirects: a webhook that answers with one now
counts as failed, so point it at the final address.

Two proxies that both append (a CDN in front of nginx, say) make the rightmost entry the
CDN's address for every client, so all logins share one limit. Have the outer proxy overwrite
`X-Forwarded-For` rather than append, or leave `LOGB_TRUST_PROXY` off there.

**Thumbnails move, and orphaned files are swept.** At the first start, thumbnails named after
a file id move to a name derived from the image's content; ones no image row names are
deleted, since they could only ever be shown for the wrong file. After that a daily sweep
removes blobs and thumbnails no row names -- but only once two sweeps a day apart agree, and
never when so many look orphaned that the database probably is not the one this data directory
belongs to. Starting against an older snapshot with `--restore` therefore leaves a day before
files only the replaced database named are removed: go back within that day if the restore was
a mistake.

**Stopping is orderly.** SIGTERM, ctrl-c and Settings → Restart now let requests in flight
finish (up to ten seconds) instead of killing them; `docker stop` no longer waits out its
timeout.

**Building the image locally needs BuildKit.** The Dockerfile cross-compiles for arm64, which
the legacy builder cannot parse. Docker Desktop and Docker's own packages include BuildKit;
some distributions package it separately (`docker-buildx` on Arch). Pulling the published image
is unaffected, and it is now published for `linux/arm64` as well as `linux/amd64`.

**A fresh `./data` has to belong to uid 65532.** The compose file still bind-mounts `./data`;
create and chown it before the first start (`sudo chown 65532:65532 data`). Existing installs
already have it right.

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
tables, and the nightly snapshot exists only where `LOGB_BACKUP_DIR` is set: this repository's
compose file sets it, the binary on its own and a bare `docker run` do not, and even where it is
set the newest snapshot can be a day old. Take one yourself first:

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
