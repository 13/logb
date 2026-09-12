# PostgreSQL part 4: choosing it from Settings — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An admin can point LogB at a PostgreSQL database from Settings — test the connection, copy the data, and restart onto it — without a shell.

**Architecture:** The server copies from its own live pool while holding the write lock, so the snapshot is consistent without stopping anything. The chosen URL is written to a file beside the data, because it cannot live in the database it points away from. A restart applies it.

**Tech Stack:** Rust, axum, sqlx over `AnyPool`, Svelte 5, Playwright.

## The conflict this plan resolves first

`copy::run` refuses a source that a server still holds — it takes `PRAGMA locking_mode = EXCLUSIVE` on SQLite and counts connections on PostgreSQL. That check is right for the command-line case and makes the Settings flow impossible as written: the server doing the copying *is* the process holding the database.

So the server gets its own entry point. It copies from the pool it already has, inside a `db::begin_write` transaction — which on SQLite is `BEGIN IMMEDIATE` and on PostgreSQL takes the advisory lock. Writes are blocked for the duration, which is correct: a migration that let writes land on the old database while copying would lose them.

Both paths share one body. Two copies of this logic would drift, and the one that drifted would be the one nobody ran.

## Global Constraints

- SQLite 330 backend tests, PostgreSQL 330, vitest 168, Playwright 22 pass unchanged at every task boundary. No `--skip`, no `#[ignore]`.
- No new dependency.
- `cargo clippy --all-targets --locked -- -D warnings` clean.
- Every user-facing string exists in **both** `frontend/src/i18n/en.ts` and `de.ts`.
- **A connection string holds a password.** It is never returned to the browser once saved, never logged, and never included in an error body. Tests assert this by searching responses and captured logs for the password, not by reading the code.
- Admin only, like the other destructive settings.
- Every test must be watched failing before its fix. Seven assertions on this project could not fail; each was found by running it.
- Run verification in the FOREGROUND. Background runs die when a subagent's turn ends.

## Running PostgreSQL

```bash
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16
until docker exec logb-pg pg_isready -q; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test
docker rm -f logb-pg
```

## File structure

| File | Responsibility |
|---|---|
| `src/copy.rs` | One copy body; `run` (offline) and `run_live` (from a running server) |
| `src/pointer.rs` (new) | Reading and writing the database pointer file |
| `src/api/database.rs` (new) | The four endpoints the screen calls |
| `frontend/src/routes/Settings.svelte` | The section |
| `frontend/src/i18n/{en,de}.ts` | Its strings |

---

### Task 1: Copying from a running server

**Files:**
- Modify: `src/copy.rs`
- Test: `tests/copy.rs`

**Interfaces:**
- Produces: `copy::run_live(db: &AnyPool, backend: Backend, dest_url: &str) -> Result<Report, BoxError>`.

- [ ] **Step 1: Write the failing test**

Append to `tests/copy.rs`:

```rust
/// The Settings flow copies from the database the server is using, which `run` deliberately
/// refuses. `run_live` does it from the pool the server already holds, inside the write
/// transaction -- so the snapshot is consistent and no write can land on the old database while
/// the copy is in flight.
#[tokio::test]
async fn a_running_server_can_copy_its_own_database() {
    let app = seeded().await;                 // its server is running and holds the database
    let dest = common::scratch_database().await;

    let report = logb::copy::run_live(&app.state.db, app.state.backend, &dest.url).await.unwrap();

    assert!(report.tables.iter().any(|(t, n)| t == "activities" && *n == 2), "{:?}", report.tables);
    // And the source is untouched and still serving: this is not a move.
    let objects: serde_json::Value = app.get_json("/objects").await;
    assert_eq!(objects["items"][0]["name"], "Golf");
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test --test copy`

Expected: `run_live` does not exist.

- [ ] **Step 3: Split the body from the entry points**

Extract what `run` does between opening the source and reporting into a private function taking
an open source pool and a destination URL. `run` keeps the in-use refusal and calls it;
`run_live` wraps it in `db::begin_write` on the caller's pool.

The two entry points differ only in how the source is made safe to read — one refuses a database
in use, the other holds the write lock. Say that in a comment on each, because a future reader
finding two entry points will otherwise assume one is an accident.

**A hazard already hit three times on this project:** anything inside a `db::begin_write`
transaction that opens its own pool connection blocks against the advisory lock that transaction
holds, and hangs rather than failing. `run_live` must not call anything that acquires from the
pool; hand the transaction down.

- [ ] **Step 4: Run both backends**

Expected: 331 on both. The existing copy tests must pass unchanged — they cover `run`, whose
behaviour this task does not change.

- [ ] **Step 5: Commit**

```bash
git add src/copy.rs tests/copy.rs
git commit -m "feat: let a running server copy its own database"
```

---

### Task 2: The pointer file

**Files:**
- Create: `src/pointer.rs`
- Modify: `src/lib.rs`, `src/config.rs`
- Test: `tests/pointer.rs` (new)

**Interfaces:**
- Produces: `pointer::path(data_dir: &Path) -> PathBuf`,
  `pointer::read(data_dir: &Path) -> Option<String>`,
  `pointer::write(data_dir: &Path, url: &str) -> Result<(), BoxError>`.
- Modifies: `Config::database_url()` consults, in order: `LOGB_DATABASE_URL`, the pointer file, then the default SQLite path.

- [ ] **Step 1: Write the failing tests**

Create `tests/pointer.rs`:

```rust
//! Where LogB remembers which database to open. It cannot live in the database, because it
//! points away from it.

#[test]
fn the_pointer_is_written_readable_only_by_its_owner() {
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), "postgres://user:secret@db/logb").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(logb::pointer::path(dir.path())).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the file holds a password: {mode:o}");
    }
    assert_eq!(logb::pointer::read(dir.path()).as_deref(), Some("postgres://user:secret@db/logb"));
}

#[test]
fn the_environment_wins_over_the_file() {
    // An operator who sets the environment cannot have it changed from a browser.
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), "postgres://from-the-file/logb").unwrap();
    let config = logb::config::Config {
        data_dir: dir.path().to_path_buf(),
        database_url: Some("postgres://from-the-environment/logb".into()),
        ..logb::config::Config::for_test()
    };
    assert_eq!(config.database_url().unwrap(), "postgres://from-the-environment/logb");
}

#[test]
fn no_pointer_and_no_environment_means_the_sqlite_file_beside_the_data() {
    let dir = tempfile::tempdir().unwrap();
    let config = logb::config::Config { data_dir: dir.path().to_path_buf(), database_url: None, ..logb::config::Config::for_test() };
    assert_eq!(config.database_url().unwrap(), logb::db::sqlite_url(dir.path()).unwrap());
}
```

`Config::for_test()` may not exist — `src/config.rs` has a test constructor already; find it and
use whatever it is actually called.

- [ ] **Step 2: Run and watch fail**

Run: `cargo test --test pointer`

Expected: `logb::pointer` does not exist.

- [ ] **Step 3: Write the module**

`src/pointer.rs` holds the file at `<data_dir>/database.url`, containing the URL and nothing
else. Created with mode 0600 on Unix — it holds a password, and the README says so.

Write it atomically: write a temporary file beside it, set the mode, then rename. A half-written
pointer is a database that will not open.

`Config::database_url()` consults the environment, then the file, then the SQLite default.

- [ ] **Step 4: Run both backends**

Expected: 334 on both (331 plus the three new tests).

- [ ] **Step 5: Commit**

```bash
git add src/pointer.rs src/lib.rs src/config.rs tests/pointer.rs
git commit -m "feat: remember which database to open, beside the data"
```

---

### Task 3: Refusing to start on a database that is wrong

**Files:**
- Modify: `src/lib.rs`
- Test: `tests/pointer.rs`

**Interfaces:**
- Consumes: `pointer::read` from Task 2.

This project has already served an empty database for nineteen hours without noticing. An
instance that starts on the wrong database looks identical to one that has lost everything, so
it must not start.

- [ ] **Step 1: Write the failing tests**

Append to `tests/pointer.rs`:

```rust
/// A pointer naming a database that cannot be reached must stop the server, not start it on
/// something else. Starting on the SQLite default instead would silently serve the old data
/// after a migration the operator believes succeeded.
#[tokio::test]
async fn an_unreachable_pointer_refuses_to_start() {
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), "postgres://nobody:nothing@127.0.0.1:1/logb").unwrap();
    let err = logb::build_from_data_dir(dir.path()).await.unwrap_err().to_string();
    assert!(err.contains("database.url"), "the error must name the pointer: {err}");
    assert!(!err.contains("nothing"), "the password must not appear in the error: {err}");
}

/// A pointer naming an empty database is the same failure wearing a friendlier face: the schema
/// would be created and the instance would come up with no data, looking healthy.
#[tokio::test]
async fn a_pointer_to_an_empty_database_refuses_to_start() {
    let Ok(server) = std::env::var("LOGB_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED: needs LOGB_TEST_DATABASE_URL");
        return;
    };
    let empty = common::scratch_database_on(&server).await;
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), &empty.url).unwrap();
    let err = logb::build_from_data_dir(dir.path()).await.unwrap_err().to_string();
    assert!(err.contains("no users"), "the error must say what is wrong: {err}");
}
```

`build_from_data_dir` is indicative — use whatever entry point actually builds the app from a
config, and construct the config with that data directory.

- [ ] **Step 2: Run and watch fail**

Expected: the server starts happily in both cases.

- [ ] **Step 3: Implement the refusals**

At startup, when the URL came from the pointer file rather than the environment or the default:
a connection failure is fatal and names `database.url`; a reachable database with **no users** is
fatal and says so.

The distinction matters: a first run has no users either. So the refusal applies only when a
pointer file exists — which by definition means someone already migrated onto it.

**Scrub the password from every error.** sqlx's connection errors can carry the URL. Redact it
to `postgres://…@host/db` before the error leaves this function, and the first test asserts it.

- [ ] **Step 4: Run both backends and commit**

```bash
git add src/lib.rs tests/pointer.rs
git commit -m "fix: refuse to start on a database the pointer cannot open"
```

---

### Task 4: The four endpoints

**Files:**
- Create: `src/api/database.rs`
- Modify: `src/api/mod.rs` (mount it), `docs/openapi.json`
- Test: `tests/database_api.rs` (new)

**Interfaces:**
- Produces: `GET /database` → `{ backend, host, database, pointer_writable }`;
  `POST /database/test` → `{ reachable, version, state }` where `state` is `"empty"`,
  `"holds_logb_data"` or `"unreachable"`;
  `POST /database/switch` → the copy `Report` plus the pointer written;
  `POST /database/restart` → 202 and the process exits.

- [ ] **Step 1: Write the failing tests**

Create `tests/database_api.rs`. The password assertions are the point of this task:

```rust
mod common;

#[tokio::test]
async fn the_current_database_is_described_without_its_password() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let body: serde_json::Value = app.get_json("/database").await;
    assert!(body["backend"].is_string());
    let text = body.to_string();
    assert!(!text.contains("password"), "{text}");
}

#[tokio::test]
async fn a_saved_url_never_comes_back_out() {
    let Ok(server) = std::env::var("LOGB_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED: needs LOGB_TEST_DATABASE_URL");
        return;
    };
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let dest = common::scratch_database_on(&server).await;

    app.post_json("/database/switch", &serde_json::json!({ "url": dest.url })).await;

    // Neither the description nor any log line may carry it.
    let body: serde_json::Value = app.get_json("/database").await;
    assert!(!body.to_string().contains("logb"), "the password leaked into the description: {body}");
    assert!(!app.captured_logs().contains("logb"), "the password leaked into the logs");
}

#[tokio::test]
async fn only_an_admin_may_switch_the_database() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let plain = app.create_user_client("anna", "password123").await;
    let res = plain.post(app.url("/database/test")).json(&serde_json::json!({ "url": "sqlite::memory:" })).send().await.unwrap();
    assert_eq!(res.status(), 403);
}
```

The password in `LOGB_TEST_DATABASE_URL` is `logb`, which is why those assertions search for it.
If the harness cannot capture logs, add that capability — asserting on the code's shape instead
would be exactly the kind of test this project has repeatedly found to be worthless.

- [ ] **Step 2: Run and watch fail**

Expected: 404s — the routes do not exist.

- [ ] **Step 3: Implement the endpoints**

All four are `AdminUser`. `test` connects and reports the server version and whether the
destination is empty or already holds LogB data, **without** writing anything. `switch` calls
`copy::run_live`, and only on success writes the pointer — the copy is reversible until that
moment. `restart` answers 202, then exits the process after the response is flushed.

`GET /database` reports `pointer_writable: false` when `LOGB_DATABASE_URL` is set, so the screen
can explain why it is read-only rather than failing when saved.

- [ ] **Step 4: Update the API description**

`docs/openapi.json` is checked against the router by `tests/openapi.rs`, which will fail until
the new paths are described. Add them.

- [ ] **Step 5: Run both backends and commit**

```bash
git add src/api/database.rs src/api/mod.rs docs/openapi.json tests/database_api.rs tests/common/mod.rs
git commit -m "feat: describe, test and switch the database over HTTP"
```

---

### Task 5: The screen, and the restart

**Files:**
- Modify: `frontend/src/routes/Settings.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests-e2e/10-database.spec.ts` (new)

**Interfaces:**
- Consumes: the four endpoints from Task 4.

- [ ] **Step 1: Write the failing end-to-end test**

Create `frontend/tests-e2e/10-database.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

// Seeds its own object with a distinctive name: one server and one database are shared across
// the whole run.
test('the database section shows where the data is, and is admin only', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: /Database/ })).toBeVisible();
  // SQLite is the default, and the section says so rather than showing a connection string.
  await expect(page.getByText(/SQLite/)).toBeVisible();
});
```

The switch itself is not driven end-to-end — it needs a PostgreSQL server, which the Playwright
run does not have. It is covered by `tests/database_api.rs`. Say so in a comment in the spec
file, so the next reader does not think it was forgotten.

- [ ] **Step 2: Run and watch fail**

Run: `cd frontend && npm run build && npx playwright test 10-database`

Expected: no such heading.

- [ ] **Step 3: Build the section**

Under Settings, admin only, following the file's existing section pattern: where the data is now;
a connection string field with **Test connection**; **Copy data and switch**, which shows the
per-table result; and, once a switch is pending, **Restart now** with plain text saying the app
will be unavailable for a few seconds and will only come back if something is supervising it.

When `pointer_writable` is false, the field is read-only and the section says the environment
decides.

Every string goes in both `en.ts` and `de.ts`. The German for the restart warning should read as
German, not as translated English.

- [ ] **Step 4: Run everything**

Run: `cd frontend && npm run check && npm run build && npx playwright test`, plus both backend
suites.

Expected: 23 Playwright tests, vitest 168, and both backends unchanged.

- [ ] **Step 5: Look at it**

Screenshot the section in both themes and both languages, and confirm the connection string
field does not render a saved password. Report what you see.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/routes/Settings.svelte frontend/src/i18n frontend/tests-e2e/10-database.spec.ts
git commit -m "feat: point LogB at another database from Settings"
```

---

### Task 6: Say how it works

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-09-11-postgres-p4-settings-design.md`

- [ ] **Step 1: Document the screen and the file**

The README's PostgreSQL section gains: the Settings route as an alternative to `--copy-to`, that
the chosen URL is stored in `<data dir>/database.url` with mode 0600, **that this file contains
the password in plaintext**, and that setting `LOGB_DATABASE_URL` overrides it and makes the
screen read-only.

State the restart plainly: the app exits and relies on the supervisor to bring it back;
`restart: unless-stopped` in the compose file does that.

- [ ] **Step 2: Mark the spec implemented, and record what changed**

The spec says the copy runs with the server stopped. It does not — the server copies from its
own pool under the write lock, because the alternative was a screen that could never work. Add a
line saying so; a spec that disagrees with the code is worse than one that is out of date.

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/specs/2026-09-11-postgres-p4-settings-design.md
git commit -m "docs: switching database from Settings, and where the URL is kept"
```
