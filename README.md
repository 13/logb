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

| Env                    | Default   |                                                |
|-------------------------|-----------|------------------------------------------------|
| `MEMTO_DATA_DIR`       | `./data`  | database, files, thumbnails                    |
| `MEMTO_BIND`           | `0.0.0.0` |                                                |
| `MEMTO_PORT`           | `8080`    |                                                |
| `MEMTO_MAX_UPLOAD_MB`  | `50`      | per file                                       |
| `MEMTO_SECURE_COOKIE`  | `auto`    | `auto` = Secure behind `X-Forwarded-Proto: https`; `true`; `false` |
| `MEMTO_LOG`            | `info`    | tracing filter                                 |
| `MEMTO_TRUST_PROXY`    | `false`   | trust `X-Forwarded-For` for the login rate limiter's client IP; enable only behind a reverse proxy that overwrites the header |

Put memto behind a reverse proxy with HTTPS when exposing it beyond your LAN.

## Development

```bash
cargo run                      # API on :8080 (serves frontend/dist if built)
cd frontend && npm install && npm run dev   # Vite on :5173, proxies /api
cargo test                     # backend tests
cd frontend && npm test        # frontend unit tests
cd frontend && npm run e2e     # Playwright against the built binary
./build.sh                     # full build into dist/
```

End-to-end tests run the real binary against the built SPA on port 8099 with a
scratch data directory (`.e2e-data`, wiped on each run):

```bash
cd frontend && npm run e2e
```

## API

JSON under `/api`, cookie session. See `docs/superpowers/specs/2026-09-04-memto-design.md`.

## License

MIT
