# Hardening Round: Security, File Integrity, Offline Correctness, Ops, CI

Date: 2026-09-25. State at design time: `main` at `7232bb7` (0.16.1), working tree clean.
Source: three read-only audits (backend, frontend, CI/repo) run the same day. Two findings were
confirmed by hand before this spec: `client_ip` takes the first `X-Forwarded-For` hop
(`src/auth.rs:232`), and SQLite reuses an `AUTOINCREMENT` id after a rolled-back insert.

Five tracks, each its own commit series on its own branch, merged into `hardening-round`.
Target release: 0.17.0.

## Global constraints

- `tests/schema_parity.rs` stays green. Migration numbers are reserved per track (below) so
  parallel branches do not collide.
- No copy change alters an existing `aria-label`; e2e finds controls by accessible name.
- New UI strings go into both `en.ts` and `de.ts` (`tests/i18n.test.ts` enforces parity).
- Each track leaves green: `cargo clippy --all-targets --locked -- -D warnings`,
  `cargo test --locked`, the postgres suite
  (`LOGB_TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:55432/postgres cargo test --all-targets --locked`),
  and in `frontend/`: `npm run check`, `npm test`, `npm run build`. Playwright runs once on the
  merged branch.
- A bug fix lands with a test that failed before it (TDD), except CI/config changes.

| Track | SQLite migration | PostgreSQL migration |
|---|---|---|
| A security | `0027_*` | `0018_*` |
| B files | none planned | none planned |
| D ops | `0028_*` | `0019_*` |

---

## Track A: security (backend + the one form it needs)

1. **Proxy headers.** `client_ip` uses the **rightmost** `X-Forwarded-For` entry (the one the
   trusted proxy appended). Same for `X-Forwarded-Host` in `api/pairing.rs`. The test at
   `auth.rs:491` is inverted; a new test spoofs a leading hop and stays rate-limited.
2. **Per-username login limit** alongside the per-IP one, same window and maximum, keyed on the
   lower-cased username, swept the same way. Unknown usernames count too (no user enumeration).
3. **Argon2 off the runtime.** Hashing and verifying run in `spawn_blocking` at every call site.
4. **Current password on self-change.** `PATCH /users/{id}` with a new password and
   `me.id == id` requires `current_password`; wrong or missing → 403 with a stable code and
   the attempt counts against the login limiter. An admin resetting *another* user is unchanged.
   The Settings password form gains a current-password field (new i18n keys, new label).
   `docs/openapi.json` documents the field.
5. **Hashed session tokens.** `sessions.token` stores `sha256(token)` hex; lookup hashes the
   cookie. Migration A deletes every existing session (everyone signs in once) and says so in
   `docs/upgrading.md` under 0.17.0.
6. **Atomic multi-statement writes.** `users::update` runs its statements in one write
   transaction (`db::begin_write`); `delete_sessions_for_user` too, or takes the caller's
   transaction.
7. **Webhook test is not a probe.** `/me/notifications/test` returns only sent/failed (no
   upstream error text; log it server-side). Outgoing notification clients use
   `redirect(Policy::none())`. Plain-http loopback push endpoints only under `cfg(test)` or an
   explicit test flag.

## Track B: file storage integrity

1. **Thumbnails by content hash.** `thumb_path(sha)` → `thumbs/ab/<sha>.jpg`, sharded like
   blobs. Thumbnails are written before the DB transaction opens. A startup step moves existing
   `thumbs/{id}.jpg` to the hash path for ids that exist, deletes the rest; idempotent. Fix the
   wrong "burns its number" comment in `migrations/sqlite/0007_sync.sql` (comment only — edit
   no applied migration's SQL; if sqlx checksums the file, put the correction in
   `src/sync/feed.rs` instead).
2. **Blob write durability.** `write_blob` fsyncs file then directory before/after rename, and
   when the destination exists verifies its hash (or rewrites) instead of trusting it.
3. **Discard race.** The "no row references this sha" check and the unlink happen under the
   write connection, so a concurrent same-hash insert either sees the blob or recreates it.
   `purge_orphan_files` runs under the write lock too.
4. **Orphan sweep.** Daily from the task loop: blobs with no `files` row, thumbnails with no
   `files` row, `.part`/`.tmp` scratch older than 24 h. Anything younger than 24 h is left.
5. **Import.** Stream the body to a scratch file and read with `ZipArchive<File>` (drop the
   `to_vec` copy); validate each blob with the upload MIME rules and `max_upload`; hash and write
   blobs and thumbnails *before* `begin_write`; inside the transaction, inserts only.
6. **Memory.** `serve_original` streams with `ReaderStream`. Upload keeps one buffer (no
   `.to_vec()` / `.clone()` copies). `process_image` sets `image::Limits` (max 12 000 px per side,
   256 MiB alloc) and decodes run behind a semaphore of 2.

## Track C: frontend correctness and polish

Offline/sync:
1. Replay rewrites `parent_id` and `object_id` from `resolved` within the same pass
   (`outbox.ts`), and the dead-letter write-back keeps the rewritten body.
2. ObjectDetail on a negative id redirects to the real id when a flush resolves it, and drops
   the temp cache entry.
3. Service worker: `registerType: 'prompt'`; a banner offers reload; no automatic reload. Hourly
   `registration.update()`.
4. `FilePicker` uses `newOpId`/`hashToNegativeId`; one definition of the hash in the codebase.
5. Sequence guards on Dashboard `load`, ObjectDetail `loadObject`/`loadChildren`; Reminders
   clears on `objectId` change; Documents reloads on object change, shows load errors, and
   try/catches `remove`/`setCover`.
6. Reminders: busy flag on "Mark done"; error visible inside the dialog; dialog
   `aria-labelledby`; `createReminderQueued` wraps enqueue like its siblings; pending reminders
   shown with the pending chip.

Errors/UX:
7. `errorMessage(e, t)` in `lib/api-error.ts`: network failure → `error.offline`, known API
   `code` → its key, else message. Used at the `(e as Error).message` sites. Fix raw keys
   (`object.pending-lost` gets a string; missing `$t` in ObjectForm/ActivityForm).
   `resource-csv.ts` throws keys.
8. try/catch + busy on the deletes in ObjectForm, ReminderForm; ObjectForm `onMount` load error.
9. Object templates keyed per user, stripped of `parent_id`/`private`/`archived`, cleared by
   `endSession`, quota-safe, unit-tested.
10. A11y: dark-mode chip contrast ≥ 4.5:1, `.field .warn` ≥ 4.5:1, input borders ≥ 3:1; tabs and
    timeline chips expose selection; focus the page `h1` on navigation; `role="alert"` on error
    paragraphs.
11. "auto" theme follows `prefers-color-scheme` changes; `theme-color` meta follows the theme.
12. ObjectForm's text↔milli conversions and the pending-object builder move to
    `lib/object-form.ts`, one builder shared with Dashboard.
13. Delete i18n keys nothing uses (except those this round starts using).

## Track D: backend operations

1. Graceful shutdown on SIGTERM/ctrl-c via a shared cancellation token: stop the task loop,
   drain requests up to 10 s. Settings → restart triggers the same signal instead of `exit(0)`.
2. Health reads `state.database_url`.
3. Backup tick remembers the last verified date; verifies once, right after writing.
4. Telegram: a failing credential is logged and skipped; poll only while a link code is pending.
5. Digest: check "already delivered today" before building; a per-recipient error is logged
   and skipped, not fatal to the tick.
6. Indexes (migration D): `changes(applied_at)`, `files(sha256)`.
7. Request logging at info level (method, path, status, latency, request id).

## Track E: CI, release, repo hygiene

1. Release: multi-arch (`linux/amd64,linux/arm64`) via buildx; build stages use
   `--platform=$BUILDPLATFORM` where the stage is arch-independent. Release requires CI: the
   release workflow calls `ci.yml` via `workflow_call` and needs it.
2. Pin every third-party action by full commit SHA with the tag in a comment.
3. `.github/dependabot.yml` for cargo, npm (`/frontend`), github-actions, docker; weekly. CI
   gains a weekly `schedule:` for the audit job.
4. Compose: named volume by default (bind-mount documented as the alternative with the chown);
   remove personal-instance commentary; `upgrading.md` agrees with what compose sets.
5. CI and release pass `LOGB_BUILD_COMMIT=${{ github.sha }}` as a build arg.
6. `rust-toolchain.toml` pinning the current stable; base images pinned by digest.
7. `.dockerignore` excludes root `node_modules`, `.e2e-data-*`, `graphify-out`, `.superpowers`,
   `docs`, `tests`, `frontend/playwright-report`, `frontend/test-results`, `.github`, `target`.
8. Playwright `forbidOnly: !!process.env.CI`, `retries: process.env.CI ? 1 : 0`; cache browsers.
9. README table: `LOGB_DB_POOL_SIZE`, `LOGB_BACKUP_HOUR`, `LOGB_BUILD_COMMIT`.
10. `openapi.json` `info.version` tracks `Cargo.toml`, asserted in `tests/openapi.rs`.
11. `build.sh` uses `--locked`, drops the redundant `-- --run`.
12. Untrack `node_modules/.vite/...`; ignore `/node_modules`.

## Out of scope

Metrics endpoint; splitting `sync/apply/mod.rs`, `copy.rs`; schema-level openapi checking;
TypeScript 7.
