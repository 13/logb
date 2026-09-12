# LogB

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
nobody else can see, send or discard it.

## Backup

```bash
docker compose exec logb /logb --backup /data/snapshot.db
```

`--backup` runs SQLite's `VACUUM INTO`, so it is safe while the server is
running — copying `logb.db` out from under a live instance can catch it
mid-write and miss the WAL. Blobs under `files/` are content-addressed and never
rewritten, so `rsync` covers them. Settings → Export is the other route: one zip
with the JSON and every file, importable into any instance.

Set `LOGB_BACKUP_DIR` to turn on a nightly snapshot, written at `LOGB_BACKUP_HOUR`
(default 3) and verified with `PRAGMA integrity_check` before it counts. The newest 14
are kept. Point it at a volume that is itself backed up — a snapshot on the same disk
protects you from your own mistakes, not from the disk's.

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

## Configuration

| Env                   | Default   |                                                                                                                              |
|-----------------------|-----------|------------------------------------------------------------------------------------------------------------------------------|
| `LOGB_DATA_DIR`      | `./data`  | database, files, thumbnails                                                                                                  |
| `LOGB_DATABASE_URL`  | unset     | database connection URL; unset means the SQLite file in `LOGB_DATA_DIR`. Files and thumbnails stay there either way. Pointing this at PostgreSQL is not a supported configuration yet: two setup requests can race into two admin accounts, a device's sync cursor can permanently skip changes, and there is no automatic backup. `docs/superpowers/specs/2026-09-11-postgres-p2-sync-cursor-design.md` is the work that makes it safe |
| `LOGB_BIND`          | `0.0.0.0` |                                                                                                                              |
| `LOGB_PORT`          | `8080`    |                                                                                                                              |
| `LOGB_MAX_UPLOAD_MB` | `50`      | per file                                                                                                                     |
| `LOGB_MAX_IMPORT_MB` | `1024`    | largest accepted import archive; an import may decompress to at most twice this                                              |
| `LOGB_NOTIFY_URL`    | unset     | POST a daily digest of due reminders here; unset disables notifications                                                      |
| `LOGB_NOTIFY_HOUR`   | `8`       | hour (in `LOGB_TIMEZONE`) the digest goes out                                                                                |
| `LOGB_NOTIFY_FORMAT` | `json`    | `json` posts a structured body; `text` posts the plain message with a `Title` header, which is what ntfy renders             |
| `LOGB_TIMEZONE`      | `UTC`     | IANA name (`Europe/Berlin`); which day a reminder's due date is read against                                                  |
| `LOGB_SECURE_COOKIE` | `auto`    | `auto` = Secure behind `X-Forwarded-Proto: https`; `true`; `false`                                                            |
| `LOGB_LOG`           | `info`    | tracing filter                                                                                                               |
| `LOGB_TRUST_PROXY`   | `false`   | trust `X-Forwarded-For` for the login rate limiter's client IP; enable only behind a reverse proxy that overwrites the header |
| `LOGB_LOGIN_MAX_ATTEMPTS` | `10` | login attempts allowed from one IP per minute before further ones get a 429; raise it where many people share an address |
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

End-to-end tests run the real binary against the built SPA on port 8099 with a
scratch data directory (`.e2e-data`, wiped on each run):

```bash
cd frontend && npm run e2e
```

Every spec must also pass on its own (`npx playwright test 03-search`). One
server and one database are shared across the whole run, so it is easy to write
a spec that quietly depends on data an earlier one left behind — and then a
single-spec run, which is what you reach for when investigating a failure,
fails for an unrelated reason.

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

## Time

Set `LOGB_TIMEZONE` to the household's own zone. Reminder due dates are
compared against today *there*: left at `UTC`, a household in UTC+13 sees a
reminder come due most of a day late and one in UTC−8 sees it a day early. It
also decides when `LOGB_NOTIFY_HOUR` fires. Stored timestamps stay UTC and are
rendered in the reader's locale.

## Reminder notifications

LogB sends no mail of its own. Point `LOGB_NOTIFY_URL` at a webhook you
already run and it POSTs one digest a day, at `LOGB_NOTIFY_HOUR` UTC, listing
every reminder that is due across all users:

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
LogB has no push notifications of its own and asks for no notification
permission.

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
recover it afterwards, so a lost token is revoked and replaced. Issuing and
revoking tokens deliberately require a session cookie, never a token, so a
leaked token cannot mint replacements or revoke the ones you would notice with.
Changing a password revokes every token as well as every session.

Writes that may be retried — `POST` of an activity or an attachment — accept a
`client_op_id`: any string unique to the write, generated once and kept across
retries. The server records it under a unique index, so replaying a write whose
response was lost resolves to the row already created rather than duplicating
it. The bundled web client uses this for its offline queue, and any client that
queues writes should do the same.

`LOGB_CORS_ORIGINS` (comma-separated) lets a web client on another origin call
the API; unset, no CORS headers are sent at all. Credentials are never allowed
cross-origin whatever is listed, so such a client must authenticate with a
bearer token rather than the session cookie.

## License

MIT
