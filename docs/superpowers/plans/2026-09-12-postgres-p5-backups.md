# PostgreSQL part 5: what happens to backups — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings says honestly whether LogB is backing the database up, so nobody discovers the answer when they need a backup.

**Architecture:** Most of this spec already shipped with part one — `--backup` and `--restore` refuse on PostgreSQL, the nightly job stands down, and a startup line says so. What is missing is the half a user can see: Settings reports where snapshots go and when the last one was on SQLite, and that LogB is not taking them on PostgreSQL.

**Tech Stack:** Rust, axum, Svelte 5, Playwright.

## What already exists, and must not be rebuilt

- `src/backup.rs:22` — the refusal text, shared by `--backup` and the nightly job.
- `src/backup.rs:95` — `tick`, which does not run on PostgreSQL.
- `src/restore.rs` — refuses on PostgreSQL and points at `logb --copy-to`.
- The startup line at INFO when the backend is PostgreSQL.

Read those before writing anything. This plan adds one endpoint and one Settings section.

## The honest answers

| Situation | What Settings says |
|---|---|
| SQLite, `LOGB_BACKUP_DIR` set | where snapshots go, and when the last one was written |
| SQLite, unset | that automatic backup is off, and which variable turns it on |
| PostgreSQL | that LogB does not back up here, and that it is the operator's own job |

The third is the one that matters. An operator who migrated to PostgreSQL through the Settings screen in part four has every reason to assume backups carried over. They did not.

## Global Constraints

- SQLite 363 backend tests, PostgreSQL 363, vitest 168, Playwright 23 pass unchanged at every task boundary. No `--skip`, no `#[ignore]`.
- No new dependency.
- `cargo clippy --all-targets --locked -- -D warnings` clean.
- Every string in both `frontend/src/i18n/en.ts` and `de.ts`.
- Run verification in the FOREGROUND. Six agents on this project stalled on background runs that died with their turn.

## Running PostgreSQL

```bash
docker run -d --rm --name logb-pg -e POSTGRES_PASSWORD=logb -p 55432:5432 postgres:16
until docker exec logb-pg pg_isready -q; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:logb@127.0.0.1:55432/postgres cargo test
docker rm -f logb-pg
```

---

### Task 1: The endpoint

**Files:**
- Modify: `src/api/database.rs`, `docs/openapi.json`
- Test: `tests/database_api.rs`

**Interfaces:**
- Produces: `GET /database/backup` → `{ "state": "scheduled" | "off" | "not_ours", "directory": string | null, "last_at": string | null, "hour": number | null }`.

`not_ours` is PostgreSQL: LogB is not the thing that backs this up. `off` is SQLite with no
`LOGB_BACKUP_DIR`. `scheduled` carries the directory, the hour, and the newest snapshot's
timestamp if there is one.

- [ ] **Step 1: Write the failing tests**

Append to `tests/database_api.rs`:

```rust
/// The answer an operator needs before they need it. On PostgreSQL, LogB is not the thing that
/// backs this database up -- and someone who migrated through the Settings screen has every
/// reason to assume otherwise.
#[tokio::test]
async fn backup_status_says_who_is_responsible() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let body: serde_json::Value = app.get_json("/database/backup").await;

    match app.state.backend {
        logb::dialect::Backend::Postgres => {
            assert_eq!(body["state"], "not_ours");
            assert!(body["directory"].is_null(), "there is no directory to name: {body}");
        }
        logb::dialect::Backend::Sqlite => {
            // The harness sets no backup directory, so this instance is not taking any.
            assert_eq!(body["state"], "off");
        }
    }
}

/// With a directory configured, the status names it and the schedule -- a reader should be able
/// to check the path themselves without going to the compose file.
#[tokio::test]
async fn a_configured_backup_directory_is_reported_with_its_hour() {
    let dir = tempfile::tempdir().unwrap();
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(dir.path().to_path_buf());
        c.backup_hour = 4;
    })
    .await;
    if app.state.backend != logb::dialect::Backend::Sqlite {
        eprintln!("SKIPPED: automatic backup is a SQLite mechanism");
        return;
    }
    app.setup("ben", "correct horse").await;
    let body: serde_json::Value = app.get_json("/database/backup").await;
    assert_eq!(body["state"], "scheduled");
    assert_eq!(body["directory"], dir.path().display().to_string());
    assert_eq!(body["hour"], 4);
}

/// Reading backup status is not an admin-only secret, but it is not public either: it names a
/// filesystem path.
#[tokio::test]
async fn a_plain_user_cannot_read_backup_status() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let plain = app.create_user_client("anna", "password123").await;
    let res = plain.get(app.url("/database/backup")).send().await.unwrap();
    assert_eq!(res.status(), 403);
}
```

`spawn_with` already exists in the harness and takes a closure over `Config`.

- [ ] **Step 2: Run and watch them fail**

Run: `cargo test --test database_api`

Expected: 404 — the route does not exist.

- [ ] **Step 3: Implement it**

Add the route to `src/api/database.rs`, `AdminUser` like its neighbours. The backend decides
`not_ours` before anything else — `LOGB_BACKUP_DIR` may well be set on a PostgreSQL instance
that was migrated, and it means nothing there.

`last_at` is the newest file in the directory, by modified time, formatted the way the rest of
the API formats timestamps. A directory that does not exist yet is not an error: it is a
`scheduled` status with no `last_at`, which is what a fresh instance looks like before 03:00.

- [ ] **Step 4: Describe it**

`docs/openapi.json` is checked against the router by `tests/openapi.rs`. Add the path.

- [ ] **Step 5: Run both backends**

Expected: 366 on both (363 plus three, with one skipping on PostgreSQL and saying so).

- [ ] **Step 6: Commit**

```bash
git add src/api/database.rs docs/openapi.json tests/database_api.rs
git commit -m "feat: report whether LogB is backing this database up"
```

---

### Task 2: The section, and the sentence that matters

**Files:**
- Modify: `frontend/src/routes/Settings.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`, `frontend/src/lib/types.ts`
- Test: `frontend/tests-e2e/10-database.spec.ts`

**Interfaces:**
- Consumes: `GET /database/backup` from Task 1.

- [ ] **Step 1: Write the failing test**

Append to `frontend/tests-e2e/10-database.spec.ts`:

```ts
// The Playwright suite runs on SQLite with no backup directory configured, so this is the "off"
// case: the screen must say backups are not being taken and name the variable that turns them
// on. The PostgreSQL wording is covered by tests/database_api.rs, which has a server.
test('settings says whether backups are being taken', async ({ page }) => {
  await signIn(page);
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: /Backup/ })).toBeVisible();
  await expect(page.getByText(/LOGB_BACKUP_DIR/)).toBeVisible();
});
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npm run build && npx playwright test 10-database`

Expected: no such heading.

**Note:** the Playwright suite serves the embedded frontend from `frontend/dist`, which the
debug binary reads from disk — so `npm run build` is enough for a frontend change. A **Rust**
change needs `cargo build`. This has confused two agents in opposite directions.

- [ ] **Step 3: Build the section**

A Backup section under Settings, admin only, following the file's existing `<h2>`-per-section
shape. Three states, three sentences:

- `scheduled`: where snapshots go, at what hour, and when the last one was — or that none has
  been written yet.
- `off`: that LogB is not taking backups, and that `LOGB_BACKUP_DIR` turns them on.
- `not_ours`: that the database is PostgreSQL, that LogB does not back it up, and that
  PostgreSQL's own tooling is what does.

The `not_ours` sentence is the reason this task exists. Someone who migrated through the
Settings screen has every reason to assume backups carried over; write it so that assumption
cannot survive reading the screen. Do not dress it as a warning — it is not a fault, it is a
division of responsibility.

Mention that the export archive is not a database backup, next to the existing export button.
It is portable and self-contained, and it is not the same thing.

Every string in both `en.ts` and `de.ts`, and the German should read as German.

- [ ] **Step 4: Run everything**

`cd frontend && npm run check && npm run build && npx playwright test`, plus both backend suites.

Expected: 24 Playwright, vitest 168, both backends 366.

- [ ] **Step 5: Look at it**

Screenshot the section in both themes and both languages, in the `off` state. Then force the
`not_ours` wording — stub the response in the browser, or point a build at a PostgreSQL
instance — and screenshot that too. Report what the PostgreSQL sentence actually says on screen.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/routes/Settings.svelte frontend/src/i18n frontend/src/lib/types.ts frontend/tests-e2e/10-database.spec.ts
git commit -m "feat: say in Settings whether backups are being taken"
```

---

### Task 3: Close the series

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-09-11-postgres-p5-backups-design.md`, `src/lib.rs`

- [ ] **Step 1: The README**

The PostgreSQL section says backups are the operator's job. Add where Settings reports it, and
that the export archive is not a substitute — it is a portable copy of the data, not a database
backup, and the README should not let anyone conclude otherwise.

- [ ] **Step 2: The startup warning is now the whole truth**

`src/lib.rs` warns that PostgreSQL "is not a supported configuration yet: LogB takes no
automatic backups there". After this part, that sentence is no longer a gap in the work — it is
the design. Decide whether the line should still say "not a supported configuration yet", or
whether it should now state the division of responsibility plainly without the "yet".

Parts one to five are complete. What remains true: writes serialise under a global lock, an
upload holds it while it writes a thumbnail, and LogB does not back up a PostgreSQL database.
None of those is unfinished work. Say what is true, and say it once.

Report what you chose and why — this is the sentence an operator reads first.

- [ ] **Step 3: Mark the spec implemented**

Set `Status: implemented.` and note that the refusals and the startup line arrived in part one
rather than here, so a reader following the series is not confused by finding them already done.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/superpowers/specs/2026-09-11-postgres-p5-backups-design.md src/lib.rs
git commit -m "docs: who backs up a PostgreSQL database, and who does not"
```
