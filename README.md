# memto

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

Everything lives in `./data`: `memto.db` (SQLite), `files/` (originals,
content-addressed), `thumbs/`. Back up by stopping the container and copying
`data/`, or use Settings → Export (zip with JSON + files).

## Configuration

| Env                   | Default   |                                                                                                                              |
|-----------------------|-----------|------------------------------------------------------------------------------------------------------------------------------|
| `MEMTO_DATA_DIR`      | `./data`  | database, files, thumbnails                                                                                                  |
| `MEMTO_BIND`          | `0.0.0.0` |                                                                                                                              |
| `MEMTO_PORT`          | `8080`    |                                                                                                                              |
| `MEMTO_MAX_UPLOAD_MB` | `50`      | per file                                                                                                                     |
| `MEMTO_MAX_IMPORT_MB` | `1024`    | largest accepted import archive; an import may decompress to at most twice this                                              |
| `MEMTO_NOTIFY_URL`    | unset     | POST a daily digest of due reminders here; unset disables notifications                                                      |
| `MEMTO_NOTIFY_HOUR`   | `8`       | hour (in `MEMTO_TIMEZONE`) the digest goes out                                                                                |
| `MEMTO_NOTIFY_FORMAT` | `json`    | `json` posts a structured body; `text` posts the plain message with a `Title` header, which is what ntfy renders             |
| `MEMTO_TIMEZONE`      | `UTC`     | IANA name (`Europe/Berlin`); which day a reminder's due date is read against                                                  |
| `MEMTO_SECURE_COOKIE` | `auto`    | `auto` = Secure behind `X-Forwarded-Proto: https`; `true`; `false`                                                            |
| `MEMTO_LOG`           | `info`    | tracing filter                                                                                                               |
| `MEMTO_TRUST_PROXY`   | `false`   | trust `X-Forwarded-For` for the login rate limiter's client IP; enable only behind a reverse proxy that overwrites the header |

Put memto behind a reverse proxy with HTTPS when exposing it beyond your LAN.

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

Set `MEMTO_TIMEZONE` to the household's own zone. Reminder due dates are
compared against today *there*: left at `UTC`, a household in UTC+13 sees a
reminder come due most of a day late and one in UTC−8 sees it a day early. It
also decides when `MEMTO_NOTIFY_HOUR` fires. Stored timestamps stay UTC and are
rendered in the reader's locale.

## Reminder notifications

memto sends no mail of its own. Point `MEMTO_NOTIFY_URL` at a webhook you
already run and it POSTs one digest a day, at `MEMTO_NOTIFY_HOUR` UTC, listing
every reminder that is due across all users:

```bash
MEMTO_NOTIFY_URL=https://ntfy.sh/my-private-topic MEMTO_NOTIFY_FORMAT=text
```

With `text` the body is the message and the summary rides in a `Title` header,
which is what ntfy and similar services render. The default `json` posts
`{ "title", "message", "reminders": [...] }` for a webhook that wants structure.

The digest is a notification, not a queue: the day is marked as handled before
the request goes out, so an endpoint that is down costs one failed request a
day rather than one a minute. Reminders lost to a failure stay due and appear
in the next day's digest.

## Search

`GET /api/search?q=...` returns the caller's own objects and activities whose
name, category, description, title or notes contain the term (`limit`, default
25, caps at 100). The magnifier on the dashboard opens the same thing. It is a
substring scan, not a full-text index: instant at household scale, and
case-insensitive for ASCII only, so `olwechsel` will not find `Ölwechsel`.

## API

JSON under `/api`, cookie session. See `docs/superpowers/specs/2026-09-04-memto-design.md`.

## License

MIT
