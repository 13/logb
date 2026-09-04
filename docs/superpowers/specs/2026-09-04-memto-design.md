# memto — Design

Date: 2026-09-04
Status: approved (brainstorm)

## Goal

A simple digital memory of everything done to an owned object. Users create
objects (car, e-bike, home, tool, …) and log activities with date, counter
reading (mileage/hours), description, cost, notes, photos and documents. Each
object has a chronological timeline, total costs, and reminders for future
maintenance.

## Decisions

| Topic        | Decision                                                        |
|--------------|-----------------------------------------------------------------|
| Platform     | Self-hosted web app, mobile-first PWA                           |
| Backend      | Rust: axum 0.8, tokio, sqlx (SQLite, WAL), single static binary |
| Frontend     | Svelte 5 + TypeScript + Vite + vite-plugin-pwa, embedded via rust-embed |
| Users        | Multi-user, private objects, no sharing in v1                   |
| Signup       | First user becomes admin via `/setup`; admin creates further users |
| Object model | Generic object + optional counter unit (`km`, `mi`, `h`, none)  |
| Activities   | Fixed category enum                                             |
| Reminders    | Due by date and/or counter, optional repeat; in-app only        |
| i18n         | EN + DE UI; one currency per instance (settings); money in integer cents |
| Deploy       | One Docker image (scratch + binary), one `data/` volume         |

## Data model (SQLite)

```
users        id, username UNIQUE, password_hash (argon2id), is_admin, lang, created_at
sessions     token PK, user_id, expires_at
settings     key PK, value                       -- currency (default EUR); admin edits
objects      id, user_id, name, category TEXT, counter_unit NULL ('km'|'mi'|'h'),
             description, purchase_date NULL, purchase_price_cents NULL,
             archived_at NULL, cover_attachment_id NULL, created_at, updated_at
activities   id, object_id, date, category, title, notes, counter_value NULL,
             cost_cents NULL, created_at, updated_at
             category IN ('maintenance','repair','purchase','inspection',
                          'modification','fuel','other')
files        id, user_id, sha256, original_name, mime, size, width NULL, height NULL,
             taken_at NULL, created_at            -- UNIQUE(user_id, sha256)
attachments  id, object_id, activity_id NULL, file_id, kind ('photo'|'document'), caption
reminders    id, object_id, title, notes, due_date NULL, due_counter NULL,
             repeat_months NULL, repeat_counter NULL, done_at NULL,
             done_activity_id NULL, created_at
             CHECK (due_date IS NOT NULL OR due_counter IS NOT NULL)
```

Rules:

- Totals are never stored; `SUM(cost_cents)` on read. Purchase price shown separately.
- Object current counter = `MAX(counter_value)` over its activities.
- Reminder is *due* when `today >= due_date` or `current_counter >= due_counter`
  (whichever applies). Computed on read; no scheduler.
- `attachments.object_id` is denormalized so the object Documents tab lists all
  files, including those attached to activities. Object-level documents
  (manual, registration papers) have `activity_id NULL`.
- Deleting an object cascades to activities, reminders, attachments. A `files`
  row and its blobs are removed when the last attachment referencing it is gone.
- Archive (not delete) for sold/disposed objects: history kept, hidden from
  default list.
- Dates stored as ISO-8601 text, money as integer cents, ids INTEGER autoincrement.
- Schema versioned via sqlx migrations in `migrations/`.

## File storage

- `data/memto.db`
- `data/files/<sha256[0..2]>/<sha256>` — immutable originals, content-addressed
- `data/thumbs/<file_id>.jpg` — 400 px longest side, JPEG q80, EXIF-rotated
- Upload: multipart streamed to temp file, hashed, moved into place. Duplicate
  hash for same user reuses the existing `files` row.
- Max upload size from `MEMTO_MAX_UPLOAD_MB` (default 50).
- Allowed types: `image/*`, `application/pdf`, `.txt .md .doc .docx .xls .xlsx`.
- Photos: EXIF `DateTimeOriginal` stored as `taken_at` and returned so the
  activity form can prefill the date. Thumbnail generated synchronously on
  upload inside `spawn_blocking`.
- Served through `/api/files/:id` and `/api/files/:id/thumb` after ownership
  check; documents get `Content-Disposition: attachment`.

## API

All JSON under `/api`, cookie session (`HttpOnly`, `SameSite=Lax`, `Secure`
when behind HTTPS).

```
POST /auth/setup                       first user only, 409 if any user exists
POST /auth/login   POST /auth/logout   GET /auth/me
GET/POST /users    PATCH/DELETE /users/:id                     admin only
GET /settings      PUT /settings                               PUT admin only
GET/POST /objects?archived=bool        GET/PATCH/DELETE /objects/:id
                                        (GET includes stats: total_cost_cents,
                                         activity_count, current_counter,
                                         due_reminder_count)
GET/POST /objects/:id/activities?category=&from=&to=
GET/PATCH/DELETE /activities/:id
POST /objects/:id/attachments           multipart: file, activity_id?, kind, caption?
PATCH/DELETE /attachments/:id
GET /files/:id     GET /files/:id/thumb
GET/POST /objects/:id/reminders        PATCH/DELETE /reminders/:id
POST /reminders/:id/done {activity_id?}  marks done; if repeat set, creates the
                                          next reminder (date + repeat_months,
                                          counter + repeat_counter)
GET /reminders/due                      across all user's objects (dashboard)
GET /export                             zip: data.json + files
POST /import                            zip from /export; merges into current user
GET /health
```

- Error body: `{ "error": "<code>", "message": "<text>" }`; HTTP
  400/401/403/404/409/413/500.
- Every object/activity/reminder/attachment/file route verifies
  `object.user_id == session.user_id`; mismatch returns 404 (no existence leak).
- Login rate limit: 10 attempts per minute per IP, in-memory.

## Backend layout

```
Cargo.toml
migrations/0001_init.sql
src/main.rs        startup, router, embedded SPA fallback
src/config.rs      env/CLI config (clap)
src/db.rs          pool, migrations
src/error.rs       AppError -> JSON response
src/auth.rs        password hashing, sessions, extractor, rate limit
src/files.rs       storage paths, hashing, thumbnails, EXIF
src/spa.rs         rust-embed static handler
src/api/{auth,users,settings,objects,activities,attachments,reminders,export}.rs
src/domain/{reminder.rs, money.rs}   pure logic (next due, formatting)
tests/api.rs       integration tests against a live server on a temp data dir
```

Crates: axum, tokio, tower-http (compression, request body limit), sqlx,
argon2, rand, axum-extra (cookie), rust-embed, image, kamadak-exif, zip,
serde, serde_json, thiserror, tracing, tracing-subscriber, clap, chrono.

## Frontend

Directory `frontend/`, built to `frontend/dist`, embedded at compile time.
Dev mode: `vite dev` proxies `/api` to `cargo run`.

Patterns reused from ordersplease: `lib/router.ts` history router,
`i18n/{en,de}.ts` with key parity test, persisted settings store,
theme via `data-theme` attribute.

Routes:

- `/setup` — first-run admin creation (only when backend reports no users)
- `/login`
- `/` — Dashboard: due/overdue reminders banner, object cards (cover thumb,
  name, category, current counter, total cost), archived toggle, FAB "new object"
- `/objects/:id` — header (cover, total cost, activity count, last counter,
  owned since) + tabs:
  - Timeline: activities newest first, grouped by year; category chip, cost,
    counter, thumbnail strip; category filter; FAB "log activity"
  - Documents: all attachments as grid; object-level upload
  - Reminders: open list with due state; done history collapsible
  - Info: edit object, archive, delete, export this object
- `/objects/:id/activities/new` and `/activities/:id` — form: date (default
  today), category, title, counter (prefilled with current, warns if lower),
  cost, notes, attachments via `<input type="file" accept="image/*,application/pdf">`
  with camera capture; EXIF date offered as activity date
- `/objects/:id/reminders/new` and `/reminders/:id` — form; "Done" dialog links
  an existing activity or creates one inline
- `/settings` — language, theme, currency (admin), users (admin), export/import

`lib/api.ts`: typed fetch wrapper; 401 redirects to `/login`.
PWA: precache app shell only; API is network-only. No offline write queue in v1.

## Configuration

| Env                     | Default   | Notes                                        |
|-------------------------|-----------|----------------------------------------------|
| `MEMTO_DATA_DIR`        | `./data`  | db, files, thumbs                            |
| `MEMTO_BIND`            | `0.0.0.0` |                                              |
| `MEMTO_PORT`            | `8080`    |                                              |
| `MEMTO_MAX_UPLOAD_MB`   | `50`      |                                              |
| `MEMTO_SECURE_COOKIE`   | `auto`    | `auto`: Secure when `X-Forwarded-Proto: https` |
| `MEMTO_LOG`             | `info`    | tracing filter                               |

## Build & deploy

- `Dockerfile` multi-stage: node builds `frontend/dist`; cargo builds
  `x86_64-unknown-linux-musl` release with dist embedded; final stage `scratch`
  with the binary only.
- `docker-compose.yml`: one service, `./data:/data`, port 8080.
- `build.sh` (fshare pattern): tests, musl tarballs into `dist/`.
- Backup: copy `data/` while stopped, or `sqlite3 memto.db ".backup"` plus
  `files/`. `/api/export` zip is the portable alternative.

## Testing

- `tests/api.rs`: setup/login; ownership isolation (user B gets 404 on user A's
  object); activity totals; reminder due logic by date, by counter, repeat
  creates next; upload dedup and thumbnail; export/import round trip.
- Unit tests: next-due computation, current-counter derivation, money
  formatting.
- Frontend: vitest for `api.ts`, i18n key parity, form validation.
  Playwright e2e against the built binary: setup, create object, log activity
  with photo, reminder done flow.
- Implementation follows TDD.

## Out of scope (v1)

Sharing between users, email/ntfy notifications, offline write queue,
custom key/value attributes, per-object currency, OIDC/SSO, mobile native app.
