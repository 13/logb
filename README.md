# <img src="frontend/public/icon.svg" width="40" height="40" alt="" align="top"> LogB

Complete history of your owned objects — cars, e-bikes, homes, tools.
Log what you did (date, mileage, cost, notes, photos, documents), see the
timeline and total cost per object, and get reminded of upcoming maintenance.

Self-hosted, single binary, one data folder. Mobile-first PWA.

## Run with Docker

```bash
docker compose up -d --build
```

Open http://localhost:8080, create the first (admin) user, add users under
Settings.

The container runs as uid 65532. A named volume inherits that ownership; chown
a host directory to 65532 before bind-mounting one.

Everything lives in `./data`: `logb.db` (SQLite), `files/` (originals,
content-addressed), `thumbs/`.

### Released images

Tagged releases are published to GitHub Container Registry:

```bash
docker pull ghcr.io/13/logb:latest
```

`0.2.0` pins exactly, `0.2` follows patches, `latest` follows releases. To use one instead of
building locally, edit `docker-compose.yml`: delete the `build: .` line and change
`image: logb:latest` to `image: ghcr.io/13/logb:latest`.

Upgrading is a pull and a restart:

```bash
docker compose pull
docker compose up -d
```

A new image applies any pending database migrations when it starts. Take a snapshot first if the
release notes mention a schema change — see [Backup](#backup).

> No login is needed: a package published by Actions from a public repository inherits that
> repository's visibility, so `docker pull` works anonymously. If a pull ever fails with
> `denied`, check the package's visibility under Package settings on GitHub — that is the only
> thing which makes this private, and the error does not hint at the cause.

### Upgrading to 0.3.0

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

## On a phone

LogB is a PWA and the phone is the case it is designed for: logging a fill-up
at the pump, photographing a receipt in a garage with no signal. Installed from
Chrome's "Add to Home screen", it runs full-screen, works offline, and queues
what you log until there is a connection again.

**It has to be served over HTTPS with a certificate the phone trusts.** This is
not a hardening recommendation, it is what makes the app work at all: browsers
gate service workers on a secure origin, so over plain http there is no offline
shell, nothing to install, and no cross-tab coordination. `localhost` counts as
secure; `http://192.168.1.10:8080` does not. A self-signed certificate is not
enough either — the phone has to trust it.

Two approaches that work:

- **Tailscale** (simplest for one household): install it on the server and the
  phone, enable MagicDNS and HTTPS, and reach LogB at
  `https://myserver.tailnet-name.ts.net`. Nothing is exposed to the internet.
- **A reverse proxy with a real certificate**: Caddy or nginx with Let's
  Encrypt on a domain you own, proxying to LogB's port. Set
  `LOGB_TRUST_PROXY=true` so the login rate limiter sees the real client
  address, and leave `LOGB_SECURE_COOKIE=auto`.

Over plain http on the LAN the app still runs and still saves — nothing depends
on `crypto.randomUUID`, which browsers withhold there — but it is a website,
not an installed app: no offline, no home-screen icon.

If several people sign in on one phone, note that a write queued offline
belongs to whoever made it: it stays put until that person signs back in, and
nobody else can see, send or discard it. Started without a connection, the app
opens as the last person signed in, on what it saved for them; signing out
removes that, and nobody else ever sees another person's saved data.

## Weight tracking

Choose **Body** when adding an object to enable weight tracking. Pick kg or lb and,
optionally, record a starting weight. **Log weight** accepts decimals (dot or comma), a
date and optional notes; it works offline using the same queue as other activities.

The Body page shows the latest dated weight, the change from the previous entry, and a
chart with month, three-month and all-time views. Use the chart's selector to inspect
individual measurements. Weight entries can be edited or deleted from the timeline.
Changing the display unit converts the number without changing the stored measurement.

Weight is stored separately from mileage/hours, in integer grams. The latest value is
ordered by measurement date, creation timestamp, then ID; a backdated entry does not
replace a more recent measurement. Deleting the newest entry reveals the previous one.
Pending offline entries are labelled until they sync.

**Remind me to log weight** uses a recurring reading reminder on a Body object. Only a
weight entry satisfies it; symptoms, treatments, and appointments do not. As with counter
reminders, dates up to tomorrow count to accommodate device/server timezone differences.
The reminder's metric follows the object's type if it is changed later.

Weight and unit preferences are included in sync and export/import. Older archives
remain importable and default to kg. The SQLite upgrade rebuilds the activities table
while preserving its data and references; take a database backup before upgrading.

## Household resources and recurring activities

Electricity meters, heating-oil tanks and water meters are ordinary objects, so they keep the same timeline,
documents, reminders, tags, export and sync behavior as a car or appliance. The new-object form
offers shortcuts for an electricity meter, heating-oil tank, water meter and football training.
Electricity entries use kWh; liquid fuel entries use litres or gallons. A tank can also store its
capacity, and each fuel entry can record the observed remaining percentage. Water supports
cumulative meter readings or billed-period usage in m³, litres or gallons. Household statistics
show monthly use, costs, targets, unusual meter changes and the newest tank level, including a
consumption-based estimate of how long the remaining fuel will last. Resource histories can also
be imported from CSV on the object's Info tab.

Use the **Session** activity for recurring activities such as football training. It records when
the session happened, its location and duration without inventing a separate object model.

New objects, activity entries and reminders are durable offline. Pending objects appear on the
dashboard until their server id is assigned; dependent entries follow the object when replay gives
it a server id. Settings → Data can exclude Body objects from an export when sharing household
records. Any object can also be marked private, which omits it from full-account exports while its
explicit single-object export remains available.

## Backup

```bash
docker compose exec logb /logb --backup /data/snapshot.db
```

`--backup` runs SQLite's `VACUUM INTO`, so it is safe while the server is
running — copying `logb.db` out from under a live instance can catch it
mid-write and miss the WAL. Blobs under `files/` are content-addressed and never
rewritten, so `rsync` covers them.

Set `LOGB_BACKUP_DIR` to turn on a nightly snapshot, written at `LOGB_BACKUP_HOUR`
(default 3) and verified with `PRAGMA integrity_check` before it counts. The newest 14
are kept. Point it at a volume that is itself backed up — a snapshot on the same disk
protects you from your own mistakes, not from the disk's.

**Settings → Backup says which of these is actually happening**, for an admin, without
a shell on the host: where the nightly snapshot goes and when the newest one landed, or
that none are being taken and `LOGB_BACKUP_DIR` turns them on, or — on PostgreSQL — that
LogB is taking none and never did. Check it after any change to the backup settings, and
after moving to PostgreSQL; it is the one place that reports what is true of the running
instance rather than what was configured.

**Settings → Export is not a database backup.** It writes one zip holding the JSON and
every file for the account you exported from, self-contained and importable into any LogB
instance, which makes it the right tool for moving one user's data between instances or
keeping a copy you can read without LogB at all. LogB is multi-user, and the export is
per-user: it carries no other account, no API token and no sync state, so it is not a copy
of the whole database either. It is also written only when somebody asks for one, and
`--restore` does not take it. A nightly snapshot is what gets you back to last night; an
export is what gets one user's data out. Keep both if you like, but do not count the
export as the backup.

## Restore

The server must be stopped, so run it as a one-shot container against the same volume:

```bash
docker compose stop logb
docker compose run --rm logb --restore /data/backups/logb-2026-09-01.db
docker compose start logb
```

The database being replaced is kept as `logb.db.replaced-<timestamp>` in the same
directory — restoring the wrong snapshot is recoverable.

A restored database gets a new sync epoch, so every phone re-bootstraps instead of
resuming from a cursor that now points at different history. That is deliberate and
you do not need to do anything about it.

Restoring only checks that the snapshot is a sound LogB database, not that it came
from this host, so moving a backup between hosts works on purpose: point a new
instance's `--restore` at another instance's backup directory to migrate its data.
The database only holds references to blobs by hash, not the blobs themselves, so
`files/` has to be copied across too — following this section alone leaves you with
a database whose photos and documents all 404.

## Moving to PostgreSQL

`logb --copy-to` moves an existing SQLite database into an empty PostgreSQL one, once.
It copies every table in dependency order inside a single transaction, keeps every id
exactly as it was, and verifies row counts and a primary-key fingerprint before it
commits — a copy that lost rows rolls back instead of leaving a half-copied database
that looks fine. It refuses a destination that already holds LogB data, and it refuses
to read a source the server is still holding open.

**The database only holds references to blobs by hash, not the blobs themselves —
`--copy-to` does not touch `files/`.** Only the database moves to PostgreSQL; photos,
documents and thumbnails stay on disk under `LOGB_DATA_DIR`, and the server keeps
reading them from there. With the compose file in this repository that is the same
`./data` directory before and after, so there is nothing to copy. **If the new
instance runs on another host, or from another volume, copy `data/files/` (and
`data/thumbs/`) across as well** — skip that and the new instance looks completely
healthy until someone opens a photo.

Stop the server, because a copy refuses a database anything still holds open, and
copy:

```bash
docker compose stop logb
docker compose run --rm logb --copy-to postgres://user:pass@host/logb
```

Then point the server at the new database. The `environment:` block in
`docker-compose.yml` is a literal list with no `${...}` in it, so the URL goes in
the file rather than in your shell — an `export` before `docker compose` would be
ignored, and the server would come back up on the old SQLite database, healthy and
wrong. Uncomment the line the `environment:` block already carries, with your own
URL in it:

```yaml
    environment:
      LOGB_DATABASE_URL: "postgres://user:pass@host/logb"
```

and recreate the container so it picks the new environment up — `docker compose
start` would only restart the container built from the old one:

```bash
docker compose up -d logb
```

To confirm the running container really has it — the failure this section exists to
prevent is a server that comes back up healthy on the old SQLite file:

```bash
docker inspect logb --format '{{range .Config.Env}}{{println .}}{{end}}' | grep LOGB_DATABASE_URL
```

The source database is never written to — the move is reversible for as long as it
still exists, so keep it around until the new instance has been checked over. The
copy rotates the destination's sync epoch, so every device does one full re-bootstrap
the next time it syncs; that is expected and needs nothing from you.

**Your nightly backups do not come with you.** LogB takes no backups of a PostgreSQL
database — that is PostgreSQL's own tooling's job, and `LOGB_BACKUP_DIR` is ignored once
you are there, even if it is still set. Settings → Backup says so on the screen, and the
server says it once in the log at every start. Set up `pg_dump`, `pg_basebackup` or a
WAL-level snapshot on the database server before you consider the move done. The other
property of running here — writes serialising under a global advisory lock — is in
[Configuration](#configuration).

### From Settings, instead of the shell

An admin can do the same move from Settings, without a shell on the host: paste a connection
string, test it, then "Copy data and switch". It runs the identical copy `--copy-to` runs — same
table order, same row-count and fingerprint verification, same refusal of a destination that
already holds data — but it runs it live, from the server's own database connection, rather than
requiring the server to be stopped first.

The request blocks for as long as the copy takes and only returns once it has been verified.
Writes are blocked for that whole time — the copy reads inside the same lock every write goes
through, which is what makes the copied data a single consistent snapshot — but reads are not
affected, and the instance keeps serving the old database throughout. There is nothing to poll:
the response is either a finished, verified copy or a refusal, and until it succeeds nothing
about the running instance has changed.

Once chosen, the URL is written to `database.url` inside `LOGB_DATA_DIR`, mode 0600 — readable
only by the account LogB runs as — and **it holds the password in plaintext, next to the data**.
That is the real cost of choosing a database from a web form instead of the environment; encoding
or hashing it would only hide that cost, not remove it. Setting `LOGB_DATABASE_URL` overrides
this file completely and turns the Settings screen read-only, so an operator who has already made
the choice in the environment cannot have it changed from a browser.

**A switch from Settings does not copy blobs either** — the same caveat as `--copy-to` applies,
for the same reason: the database only holds references to files by hash, not the files
themselves. If the new database is on a different host than the one serving `LOGB_DATA_DIR`
today, `files/` (and `thumbs/`) still has to be copied there by hand, or the instance comes up
looking healthy with every photo and document 404ing.

Switching only chooses the database for the *next* start — nothing about a running instance can
be swapped out from under it. Settings shows a "Restart now" button once a switch is pending; it
exits the process and nothing more. It comes back only if something is watching for that and
starts it again — `restart: unless-stopped` in this repository's compose file is that something.
LogB has no way to confirm one exists, so it says exactly that rather than implying the app
restarts itself: if nothing supervises the process, "Restart now" is "stop now" and it stays down
until it is started by hand.

## Configuration

| Env                   | Default   |                                                                                                                              |
|-----------------------|-----------|------------------------------------------------------------------------------------------------------------------------------|
| `LOGB_DATA_DIR`      | `./data`  | database, files, thumbnails                                                                                                  |
| `LOGB_DATABASE_URL`  | unset     | database connection URL; unset means the SQLite file in `LOGB_DATA_DIR`. Files and thumbnails stay there either way. Pointing this at PostgreSQL is a supported configuration with two properties worth reading once, which the server also logs once at every start. LogB takes no backups of a PostgreSQL database: `--backup` and `--restore` refuse on purpose, `LOGB_BACKUP_DIR` is ignored, and backing it up is PostgreSQL's own tooling's job -- Settings -> Backup reports which of those applies to the running instance. Every write also takes a global advisory lock, serialising writers the same as SQLite does today -- an upload writes its thumbnail to disk inside that lock, so a large upload or import blocks other writes while it runs. Neither is unfinished work; SQLite is still the default, and the more exercised path. Use `logb --copy-to`, or Settings, to bring an existing SQLite database across -- see [Moving to PostgreSQL](#moving-to-postgresql) |
| `LOGB_BIND`          | `0.0.0.0` |                                                                                                                              |
| `LOGB_PORT`          | `8080`    |                                                                                                                              |
| `LOGB_MAX_UPLOAD_MB` | `50`      | per file                                                                                                                     |
| `LOGB_MAX_IMPORT_MB` | `1024`    | largest accepted import archive; an import may decompress to at most twice this                                              |
| `LOGB_NOTIFY_URL`    | unset     | POST a daily digest of due reminders here, for every user without a webhook of their own (Settings > Notifications)          |
| `LOGB_NOTIFY_HOUR`   | `8`       | hour (in the instance timezone) the digest goes out, to webhooks and browsers alike                                          |
| `LOGB_NOTIFY_FORMAT` | `json`    | `json` posts a structured body; `text` posts the plain message with a `Title` header, which is what ntfy renders             |
| `LOGB_TELEGRAM_BOT_TOKEN` | unset | Telegram Bot API token; users connect their own private chat under Settings → Notifications |
| `LOGB_PUBLIC_URL`    | unset     | the address LogB is opened at (`https://logb.example.com`); puts links into the digest, so a notification opens the right form. Also the recommended setting behind a reverse proxy for QR sign-in (Account > Connect a phone): unset, the address in the pairing link and QR code is rebuilt from the request's `Host` (or `X-Forwarded-Host`, only when `LOGB_TRUST_PROXY` is set), which is right for a direct connection but easy to get wrong through a proxy that rewrites paths or terminates TLS somewhere the browser cannot see; setting `LOGB_PUBLIC_URL` makes it exact |
| `LOGB_TIMEZONE`      | unset     | IANA name (`Europe/Berlin`); which day a reminder's due date is read against. Unset, first-run setup stores the browser's and an admin can change it in Settings; set, it wins and Settings shows it as fixed |
| `LOGB_SECURE_COOKIE` | `auto`    | `auto` = Secure behind `X-Forwarded-Proto: https`; `true`; `false`                                                            |
| `LOGB_LOG`           | `info`    | tracing filter                                                                                                               |
| `LOGB_TRUST_PROXY`   | `false`   | trust `X-Forwarded-For` for the login rate limiter's client IP; enable only behind a reverse proxy that overwrites the header |
| `LOGB_LOGIN_MAX_ATTEMPTS` | `10` | login attempts allowed from one IP per minute before further ones get a 429; raise it where many people share an address. QR sign-in's redeem step (a phone swapping a pairing code for a token) shares this same limit and counter, by the same IP — it is not a separate budget |
| `LOGB_CORS_ORIGINS`  | *(empty)* | comma-separated origins allowed to call the API from another origin; empty sends no CORS headers. Never permits credentials — a cross-origin client uses a bearer token |

Put LogB behind a reverse proxy with HTTPS when exposing it beyond your LAN.

## Development

```bash
cargo run                      # API on :8080 (serves frontend/dist if built)
cd frontend && npm install && npm run dev   # Vite on :5173, proxies /api
cargo test                     # backend tests
cd frontend && npm test        # frontend unit tests
cd frontend && npm run e2e     # Playwright against the built binary
./build.sh                     # full build into dist/
cargo clippy --all-targets -- -D warnings   # lint gate, as CI runs it
```

`frontend/dist/` is embedded into the binary at compile time, so it must exist
for `cargo build` to work; a fresh clone gets an empty one (the SPA then serves
a 503 until `npm run build` fills it).

CI (`.github/workflows/ci.yml`) runs clippy, the backend tests, the frontend
type check, unit tests and bundle, Playwright end-to-end, and a Docker build
whose image has to answer `/api/health`. There is no `cargo fmt` gate: the
codebase uses single-line guard clauses that stable rustfmt cannot express.

End-to-end tests run the real binary against the built SPA, once per viewport:
a mobile project on port 8099 with its own scratch data directory
(`.e2e-data-mobile`) and a desktop project on port 8100 with its own
(`.e2e-data-desktop`), both wiped on each run:

```bash
cd frontend && npm run e2e
```

Every spec must also pass on its own (`npx playwright test 03-search`). Within
each project, one server and one database are still shared across that
project's whole run, so it is easy to write a spec that quietly depends on
data an earlier one left behind — and then a single-spec run, which is what
you reach for when investigating a failure, fails for an unrelated reason.

### Two rules learned the hard way

**When a second review lands in the same area, redesign instead of patching.**
The offline outbox went through five review rounds, each finding a real defect
in the previous round's fix. The ones that kept recurring were patched at the
point of failure, each patch adding a mechanism: a cache key gained a sentinel,
then early publication, then a second comparison. What finally settled it was
deleting the key and having the flush report whether it changed anything.

**A mutation check that passes means the test is wrong, not that the code is
safe.** Deleting a guard and watching its test still pass happened twice here:
once because the object under test was large enough to mask the condition, once
because the staged failure tripped a different safeguard. Both times the check
looked like reassurance and was worth none. Make the mutation as small as the
guard, and be suspicious of a green result.

Bugs that need a browser to find are expensive; the fix is usually to move the
logic somewhere a unit test can reach it (`lib/timeline-load.ts` is the result
of doing that), not to write another end-to-end test.

## Security

Sessions are 30-day cookies, `HttpOnly` and `SameSite=Lax`, `Secure` behind an
https proxy. Changing a password ends every session of that account (the
browser making the change is re-issued one); Settings → Sign out everywhere
ends them all, this browser included.

Uploaded files are served from the app's own origin, so they go out with
`nosniff` and a sandbox `Content-Security-Policy`, and only a short list of
types (JPEG, PNG, GIF, WebP, AVIF, BMP, PDF) may render in place. Everything
else downloads — an SVG is an image by MIME type and a scriptable document in
practice. The app itself is served under a policy that permits no off-origin
resource at all.

For use without a connection, the app keeps the signed-in user's recently read
objects, entries, reminders, types, tags and files on the device (in the
service worker's cache and local storage) until that user signs out. An app
started offline opens as the last user who signed in. On a shared device,
another person should sign in only after the previous user has signed out:
signing out removes that data from the device.

Signing in and out also sends `Clear-Site-Data: "cache"`, telling the browser
to drop its HTTP cache for the site, so files a shared browser cached under an
older version don't outlive the session that fetched them.

## Time

Set `LOGB_TIMEZONE` to the household's own zone. Reminder due dates are
compared against today *there*: left at `UTC`, a household in UTC+13 sees a
reminder come due most of a day late and one in UTC−8 sees it a day early. It
also decides when `LOGB_NOTIFY_HOUR` fires. Stored timestamps stay UTC and are
rendered in the reader's locale.

## Reminder notifications

LogB sends no mail of its own. Point `LOGB_NOTIFY_URL` at a webhook you
already run and it POSTs one digest a day, at `LOGB_NOTIFY_HOUR` in the
instance timezone, listing every reminder that is due across all users:

```bash
LOGB_NOTIFY_URL=https://ntfy.sh/my-private-topic LOGB_NOTIFY_FORMAT=text
```

With `text` the body is the message and the summary rides in a `Title` header,
which is what ntfy and similar services render. The default `json` posts
`{ "title", "message", "reminders": [...] }` for a webhook that wants structure.

That `text` shape is exactly what [ntfy](https://ntfy.sh) expects, which makes
it the path of least resistance to notifications on an Android phone: pick an
unguessable topic name, point `LOGB_NOTIFY_URL` at it, install the ntfy app
and subscribe to the same topic. A topic on the public server is readable by
anyone who knows its name, so treat the name as the secret or self-host ntfy.

Each person can also set it up for themselves under Settings > Notifications:

- **Their own webhook.** Their reminders then go there, in the language they
  use the app in, and leave the shared digest.
- **Browser notifications.** "Turn on notifications" subscribes that browser,
  and the digest arrives as a push notification that opens the right screen
  when tapped. Nothing needs configuring: the instance creates its signing key
  the first time it is asked. On an iPhone this works once LogB is added to
  the home screen. LogB never asks for notification permission until someone
  presses that button.
- **A test notification**, sent at once to everywhere theirs go.

**Telegram** is another per-person destination. Create a bot with [BotFather](https://t.me/BotFather),
set `LOGB_TELEGRAM_BOT_TOKEN` on the LogB server, restart it, then open Settings → Notifications
and choose **Connect Telegram**. LogB gives you a short-lived link; open it in Telegram and press
Start. The bot token stays on the server, and each person's chat is stored separately. Disconnecting
removes the chat association. Telegram receives the same daily digest as the webhook and browser
notifications, in the user's language.

Reading reminders ("log the odometer every month") come after the services,
under `Readings to log:`. They clear themselves as soon as any entry with a
counter value is logged, so there is nothing to mark done. With
`LOGB_PUBLIC_URL` set, every item carries a `link`, each reading line ends in
the URL of its one-field reading form, and a text digest about a single
reminder also sends ntfy's `Click` header, so tapping the notification opens
that form.

The digest is a notification, not a queue: the day is marked as handled before
the request goes out, so an endpoint that is down costs one failed request a
day rather than one a minute. Reminders lost to a failure stay due and appear
in the next day's digest.

## Languages and currency

The interface is English and German; each user picks their own under Settings.

Currency is one instance-wide setting, not a per-user one, and deliberately so:
costs are stored as integer cents of a single currency, so showing one user
`$249.90` and another `€249.90` for the same activity would be a relabelling,
not a conversion. Amounts are formatted in each user's own locale.

## Search

`GET /api/search?q=...` returns the caller's own objects and activities whose
name, category, description, title or notes contain the term (`limit`, default
25, caps at 100). The magnifier on the dashboard opens the same thing. It is a
substring scan, not a full-text index: instant at household scale, and
case-insensitive for ASCII only, so `olwechsel` will not find `Ölwechsel`.

## Statistics

The Statistics screen totals spend across every object you own: over time, by object (a child's
cost rolls into its parent), by type and by category. `GET /api/stats?year=&purchases=` is the
same data over the API, both parameters optional. An object's purchase price counts only with the
`purchases` toggle on, dated by its purchase date or else the day it was created, and is skipped
when a costed `purchase` activity already records that money.

An object's Info tab shows the same rules for one object: total cost of ownership (≈ per year
once owned 90 days), spend per month, consumption per fill, and an "Include contents" switch on
objects that have others inside them.

## API

JSON under `/api`. Described by [`docs/openapi.json`](docs/openapi.json), which
`tests/openapi.rs` checks against the router — a route added, removed or
renamed without updating it fails the build.

Two credentials work everywhere except token management: the `logb_session`
cookie a browser gets from `POST /auth/login`, and an API token for clients
that are not browsers.

```bash
# Create one under Settings → API access, or over the API with a session:
curl -sS -X POST https://logb.example/api/auth/tokens \
  -H 'content-type: application/json' -b cookies.txt \
  -d '{"name":"laptop"}'

curl -sS https://logb.example/api/objects -H 'authorization: Bearer logb_pat_...'
```

The plaintext is shown once and stored only as a hash; there is no way to
recover it afterwards, so a lost token is revoked and replaced. Issuing a
token deliberately requires a session cookie, never a token, so a leaked
token cannot mint replacements. Revoking one accepts a session cookie too,
and additionally the token's own bearer credential — never another token's —
so an app can revoke its own token on sign-out without holding a cookie. Not
every server this app talks to is new enough to support it, so `GET /health`
lists `token-self-revoke` in its `features` array for a client to detect
support before relying on it.
Changing a password revokes every token as well as every session.

Writes that may be retried — `POST` of an activity or an attachment — accept a
`client_op_id`: any string unique to the write, generated once and kept across
retries. The server records it under a unique index, so replaying a write whose
response was lost resolves to the row already created rather than duplicating
it. The bundled web client uses this for its offline queue, and any client that
queues writes should do the same.

A client that keeps a local mirror — the Android app — goes one step further:
every create (`POST` of an object, activity, reminder or attachment) accepts a
`client_uuid`, the sync identity the client minted for the row before the
server saw it. A replay carrying the same value answers `200` with the row the
first attempt made; a value naming another account's row, another object's row
or a tombstone answers `409`. Every object, activity, reminder and attachment
response carries its `client_uuid` (an attachment also its `file_uuid`), and
every row from `GET /api/sync/pull` carries `entity_id`, the server's integer
id for that uuid. The full shapes are in `docs/openapi.json`.

`LOGB_CORS_ORIGINS` (comma-separated) lets a web client on another origin call
the API; unset, no CORS headers are sent at all. Credentials are never allowed
cross-origin whatever is listed, so such a client must authenticate with a
bearer token rather than the session cookie.

## License

MIT
