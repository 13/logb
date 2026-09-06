# memto Daily-Use Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make memto faster to log into, able to answer "what has this cost me", and usable with no network.

**Architecture:** Three phases against the existing axum + SQLite + Svelte 5 app. Phase A is frontend plus one read-only endpoint. Phase C adds a migration, pure arithmetic in `src/domain/insights.rs`, an insights endpoint, and reminder lookahead/snooze. Phase B adds `client_op_id` idempotency columns and a client-side outbox that replays queued creates.

**Tech Stack:** Rust 2021, axum 0.8, sqlx 0.9 (SQLite), chrono; Svelte 5 (runes), TypeScript, Vite, vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-06-memto-daily-use-design.md`

## Global Constraints

- Money is `i64` cents; fuel quantity is `i64` milli-units (`quantity_milli`). No floats in the database, ever.
- Every handler that touches an object goes through `load_owned_object(&state, user.id, id)` first — ownership is enforced by the query, not by a check afterwards.
- Pure logic lives in `src/domain/*.rs` with unit tests next to it; handlers do SQL and assembly only. Follow `src/domain/reminder.rs`.
- Migrations are append-only files in `migrations/`, named `NNNN_snake_case.sql`. Never edit an applied migration.
- Every new user-facing string gets a key in **both** `frontend/src/i18n/en.ts` and `frontend/src/i18n/de.ts`.
- Backend tests are integration tests under `tests/`, using `common::spawn()`. Frontend unit tests are vitest under `frontend/tests/`, and only cover pure modules under `frontend/src/lib/*.ts` — not `.svelte` files.
- Run `cargo test` and `cd frontend && npm test && npm run check` before every commit.
- Commit messages: conventional prefix, imperative subject, body explaining *why*. End with:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```

## File Structure

| File | Responsibility |
|------|----------------|
| `src/api/activities.rs` | + `recent_titles` handler, `quantity_milli` on input/row |
| `src/api/objects.rs` | + `fuel_unit` on the object row and input |
| `src/api/insights.rs` (new) | insights endpoint: SQL rollups, calls `domain::insights` |
| `src/domain/insights.rs` (new) | pure consumption and cost-per-counter arithmetic |
| `src/api/reminders.rs` | + `within_days` on `due_list`, + `snooze` handler |
| `src/domain/reminder.rs` | + `days_until`, `counter_until`, `snoozed_date` |
| `migrations/0003_fuel_quantity.sql` (new) | `quantity_milli`, `fuel_unit` |
| `migrations/0005_client_op_id.sql` (new) | idempotency columns and partial unique indexes |
| `frontend/src/lib/activity-form.ts` | + `suggestionsFor` pure helper |
| `frontend/src/lib/outbox.ts` (new) | offline queue: pure logic + storage adapter |
| `frontend/src/lib/idb.ts` (new) | IndexedDB implementation of the outbox storage adapter |
| `frontend/src/lib/Insights.svelte` (new) | rollup rendering for the Info tab |
| `frontend/src/routes/ActivityForm.svelte` | suggestions, repeat chips, quantity field, draft cleanup |
| `frontend/src/lib/FilePicker.svelte` | camera input |
| `frontend/src/lib/ObjectCard.svelte` | quick-log button |
| `frontend/src/routes/Dashboard.svelte` | overdue/upcoming split, snooze |

---

### Task 1: `recent-titles` endpoint

**Files:**
- Modify: `src/api/activities.rs`
- Test: `tests/activities.rs`

**Interfaces:**
- Consumes: `load_owned_object` from `src/api/objects.rs`.
- Produces: `GET /api/objects/{id}/recent-titles` → `[{ title: string, category: string, last_date: string, last_cost_cents: number | null, last_counter: number | null }]`, at most 20, distinct on `(title, category)`, newest first.

- [ ] **Step 1: Write the failing test**

Append to `tests/activities.rs`:

```rust
#[tokio::test]
async fn recent_titles_are_distinct_and_newest_first() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    for (date, title, cost, counter) in [
        ("2026-01-05", "Fuel", 5000, 10_000),
        ("2026-02-05", "Oil change", 9000, 11_000),
        ("2026-03-05", "Fuel", 6210, 12_000),
    ] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
            "date": date, "category": "fuel", "title": title,
            "cost_cents": cost, "counter_value": counter
        })).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let out: Vec<serde_json::Value> = app.client
        .get(app.url(&format!("/objects/{id}/recent-titles")))
        .send().await.unwrap().json().await.unwrap();

    assert_eq!(out.len(), 2, "one row per distinct (title, category)");
    assert_eq!(out[0]["title"], "Fuel", "the most recent title comes first");
    assert_eq!(out[0]["last_date"], "2026-03-05");
    assert_eq!(out[0]["last_cost_cents"], 6210, "the newest occurrence supplies the cost");
    assert_eq!(out[0]["last_counter"], 12_000);
    assert_eq!(out[1]["title"], "Oil change");
}

#[tokio::test]
async fn recent_titles_of_another_users_object_are_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = anna.get(app.url(&format!("/objects/{id}/recent-titles"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
```

- [ ] **Step 2: Run the test and watch it fail**

Run: `cargo test --test activities recent_titles`
Expected: FAIL — the route does not exist, so the response is 404 with an HTML/JSON body and `out.len()` panics on a non-array, or the `assert_eq!(res.status(), 404)` in the second test passes for the wrong reason. Both must be red before proceeding on the first test.

- [ ] **Step 3: Add the route**

In `src/api/activities.rs`, extend `router()`:

```rust
pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/activities", get(list).post(create))
        .route("/objects/{id}/recent-titles", get(recent_titles))
        .route("/activities/{id}", get(read).patch(update).delete(delete))
}
```

- [ ] **Step 4: Implement the handler**

Add to `src/api/activities.rs`, below `list`:

```rust
/// One row per distinct (title, category) an object has seen, newest first, with the
/// values of the most recent occurrence.
///
/// A dedicated endpoint rather than reusing `list`: that one joins every attachment of
/// the object, which is payload a phone does not need in order to fill a datalist.
#[derive(Serialize, sqlx::FromRow)]
pub struct TitleSuggestion {
    pub title: String,
    pub category: String,
    pub last_date: String,
    pub last_cost_cents: Option<i64>,
    pub last_counter: Option<i64>,
}

const SUGGESTION_LIMIT: i64 = 20;

async fn recent_titles(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
) -> Result<Json<Vec<TitleSuggestion>>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    // The correlated subqueries pick the newest occurrence explicitly. SQLite would also
    // hand back a bare column from the MAX() row, but that behaviour is a quirk to rely on,
    // not a contract.
    let rows = sqlx::query_as::<_, TitleSuggestion>(
        "SELECT a.title, a.category, MAX(a.date) AS last_date, \
           (SELECT x.cost_cents FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_cost_cents, \
           (SELECT x.counter_value FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_counter \
         FROM activities a WHERE a.object_id = ?1 \
         GROUP BY a.title, a.category ORDER BY last_date DESC LIMIT ?2",
    )
    .bind(object_id)
    .bind(SUGGESTION_LIMIT)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --test activities`
Expected: PASS, both new tests green and the existing ones unchanged.

- [ ] **Step 6: Commit**

```bash
git add src/api/activities.rs tests/activities.rs
git commit -m "feat: suggest an object's recent activity titles

Typing 'Fuel' again for the fortieth time is the single most repeated
action in the app. The endpoint is separate from the activity list because
that one joins every attachment of the object, which a datalist does not need."
```

---

### Task 2: Fast capture in the form

**Files:**
- Modify: `frontend/src/lib/activity-form.ts`, `frontend/src/lib/types.ts`, `frontend/src/routes/ActivityForm.svelte`, `frontend/src/lib/FilePicker.svelte`, `frontend/src/lib/ObjectCard.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/activity-form.test.ts`

**Interfaces:**
- Consumes: `GET /objects/{id}/recent-titles` from Task 1.
- Produces: `suggestionsFor(all: TitleSuggestion[], category: Category | null): TitleSuggestion[]` from `frontend/src/lib/activity-form.ts`.

- [ ] **Step 1: Write the failing test**

Append to `frontend/tests/activity-form.test.ts`:

```ts
import { suggestionsFor } from '../src/lib/activity-form';
import type { TitleSuggestion } from '../src/lib/types';

const s = (title: string, category: string): TitleSuggestion =>
  ({ title, category, last_date: '2026-01-01', last_cost_cents: null, last_counter: null }) as TitleSuggestion;

describe('suggestionsFor', () => {
  it('narrows to the chosen category', () => {
    const all = [s('Fuel', 'fuel'), s('Oil change', 'maintenance')];
    expect(suggestionsFor(all, 'fuel').map((x) => x.title)).toEqual(['Fuel']);
  });

  it('keeps order and drops repeated titles when no category is chosen', () => {
    const all = [s('Fuel', 'fuel'), s('Fuel', 'other'), s('Oil change', 'maintenance')];
    expect(suggestionsFor(all, null).map((x) => x.title)).toEqual(['Fuel', 'Oil change']);
  });

  it('returns an empty list when nothing matches', () => {
    expect(suggestionsFor([s('Fuel', 'fuel')], 'repair')).toEqual([]);
  });
});
```

The file already imports `describe`/`it`/`expect` from vitest and pulls helpers from
`../src/lib/activity-form` on line 2. Extend those existing import lines with `suggestionsFor`
and add the `TitleSuggestion` type import — do not add a second `from 'vitest'` import.

- [ ] **Step 2: Run the test and watch it fail**

Run: `cd frontend && npx vitest run tests/activity-form.test.ts`
Expected: FAIL with "No 'suggestionsFor' export is defined".

- [ ] **Step 3: Add the type and the helper**

In `frontend/src/lib/types.ts`, after the `Activity` interfaces:

```ts
export interface TitleSuggestion {
  title: string; category: Category; last_date: string;
  last_cost_cents: number | null; last_counter: number | null;
}
```

In `frontend/src/lib/activity-form.ts`:

```ts
import type { Activity, ActivityInput, Category, TitleSuggestion } from './types';

/** Suggestions for the chosen category (all of them when none is chosen), one per title. */
export function suggestionsFor(all: TitleSuggestion[], category: Category | null): TitleSuggestion[] {
  const seen = new Set<string>();
  return all
    .filter((s) => category === null || s.category === category)
    .filter((s) => {
      if (seen.has(s.title)) return false;
      seen.add(s.title);
      return true;
    });
}
```

Keep the existing `import type { Activity, ActivityInput } from './types';` line replaced by the one above rather than adding a second import.

- [ ] **Step 4: Run the test**

Run: `cd frontend && npx vitest run tests/activity-form.test.ts`
Expected: PASS.

- [ ] **Step 5: Add the i18n keys**

In `frontend/src/i18n/en.ts`, in the activity block:

```ts
  'activity.repeat': 'Repeat',
  'activity.take-photo': 'Take photo',
  'activity.discard-draft': 'Discard this entry and its uploads?',
  'dash.log': 'Log',
```

In `frontend/src/i18n/de.ts`, the same keys:

```ts
  'activity.repeat': 'Wiederholen',
  'activity.take-photo': 'Foto aufnehmen',
  'activity.discard-draft': 'Eintrag und hochgeladene Dateien verwerfen?',
  'dash.log': 'Erfassen',
```

- [ ] **Step 6: Wire suggestions into the form**

In `frontend/src/routes/ActivityForm.svelte`, add to the imports:

```ts
  import { emptyActivity, exifDate, suggestionsFor, toActivityInput, validateActivity } from '../lib/activity-form';
  import type { TitleSuggestion } from '../lib/types';
```

Add state and loading, next to the existing declarations:

```ts
  let allSuggestions = $state<TitleSuggestion[]>([]);
  const suggestions = $derived(suggestionsFor(allSuggestions, input.category));
```

Inside `onMount`, after the object load:

```ts
    allSuggestions = await api<TitleSuggestion[]>('GET', `/objects/${oid}/recent-titles`);
```

Add the prefill function below `buildInput()`:

```ts
  /** Prefill from a past entry. The user still reviews and saves; nothing is written here. */
  function repeat(s: TitleSuggestion) {
    input.title = s.title;
    input.category = s.category;
    if (s.last_cost_cents !== null) costText = centsToInput(s.last_cost_cents);
  }
```

Replace the title field markup with:

```svelte
    {#if !editing && suggestions.length > 0}
      <div class="chips">
        {#each suggestions.slice(0, 3) as s (s.title + s.category)}
          <button type="button" class="chip" onclick={() => repeat(s)}>{$t('activity.repeat')}: {s.title}</button>
        {/each}
      </div>
    {/if}
    <div class="field">
      <label for="ti">{$t('activity.title')}</label>
      <input id="ti" list="titles" bind:value={input.title} required />
      <datalist id="titles">
        {#each suggestions as s (s.title + s.category)}<option value={s.title}></option>{/each}
      </datalist>
    </div>
```

- [ ] **Step 7: Delete the abandoned draft on cancel**

`ensureSaved()` inserts a real activity so uploads have a parent. Cancelling afterwards
currently leaves it in the timeline. In `frontend/src/routes/ActivityForm.svelte`:

```ts
  /** True when `saved` exists only because the user attached a file, never because they saved. */
  let autoDraft = $state(false);

  async function ensureSaved(): Promise<Activity> {
    if (saved) return saved;
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) throw new Error($t(bad));
    saved = await api<Activity>('POST', `/objects/${oid}/activities`, body);
    autoDraft = true;
    return saved;
  }

  /** Cancel throws the auto-created draft away; keeping it would leave a stray timeline entry. */
  async function cancel() {
    if (autoDraft && saved) {
      if (attachments.length > 0 && !confirm($t('activity.discard-draft'))) return;
      try { await api('DELETE', `/activities/${saved.id}`); } catch { /* leaving it is better than blocking the exit */ }
    }
    back(`/objects/${oid}`);
  }
```

Set `autoDraft = false` in `submit()` immediately before `go(...)`, so a successful save
never deletes anything. Point the cancel button at the new function:

```svelte
      <button type="button" class="ghost" onclick={cancel}>{$t('nav.cancel')}</button>
```

- [ ] **Step 8: Add the camera input**

Replace the markup in `frontend/src/lib/FilePicker.svelte` with:

```svelte
<div class="picker">
  <input bind:this={el} type="file" multiple accept="image/*,application/pdf,.txt,.md,.doc,.docx,.xls,.xlsx"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <!-- `capture` cannot live on the input above: on mobile it suppresses picking an existing file. -->
  <input bind:this={cam} type="file" accept="image/*" capture="environment"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <div class="row">
    <button type="button" class="ghost" disabled={busy} onclick={() => el.click()}>
      {busy ? $t('activity.uploading') : `+ ${$t('activity.add-files')}`}
    </button>
    <button type="button" class="ghost" disabled={busy} onclick={() => cam.click()}>📷 {$t('activity.take-photo')}</button>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
</div>
```

Declare `let cam: HTMLInputElement;` beside `let el: HTMLInputElement;`, extend the style
rule to `.picker input { display: none; }` (already correct — it matches both), and reset
both inputs in the `finally` block: `el.value = ''; cam.value = '';`.

- [ ] **Step 9: Add the quick-log button to the object card**

In `frontend/src/lib/ObjectCard.svelte`, the card is itself a `<button>`, so the new
control cannot nest inside it. Wrap both in a row instead:

```svelte
<div class="card-row">
  <button class="card list-card" onclick={() => go(`/objects/${object.id}`)}>
    <!-- unchanged card contents -->
  </button>
  <button class="ghost quicklog" aria-label={$t('dash.log')}
          onclick={() => go(`/objects/${object.id}/activities/new`)}>＋</button>
</div>
```

```css
  .card-row { display: flex; gap: 8px; align-items: stretch; }
  .card-row > .list-card { flex: 1; min-width: 0; }
  .quicklog { flex: none; width: 48px; font-size: 1.4rem; border: 1px solid var(--border); border-radius: var(--radius); }
```

- [ ] **Step 10: Verify the whole frontend**

Run: `cd frontend && npm test && npm run check`
Expected: all vitest suites PASS, `svelte-check` reports 0 errors.

- [ ] **Step 11: Commit**

```bash
git add frontend/src frontend/tests
git commit -m "feat: cut the taps and typing out of logging an activity

Quick-log straight from the object card, remembered titles as a datalist and
three repeat chips, a camera button that actually opens the camera, and cancel
now removes the draft that uploading a file silently created."
```

---

### Task 3: Fuel quantity in the schema and API

**Files:**
- Create: `migrations/0003_fuel_quantity.sql`
- Modify: `src/api/activities.rs`, `src/api/objects.rs`
- Test: `tests/activities.rs`, `tests/objects.rs`

**Interfaces:**
- Produces: `quantity_milli: number | null` on the activity row and input; `fuel_unit: 'l' | 'gal' | 'kwh' | null` on the object row and input.

- [ ] **Step 1: Write the failing tests**

Append to `tests/activities.rs`:

```rust
#[tokio::test]
async fn fuel_quantity_round_trips_and_is_validated() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-03-05", "category": "fuel", "title": "Fuel",
        "counter_value": 12_000, "cost_cents": 6210, "quantity_milli": 41_300
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let a: serde_json::Value = res.json().await.unwrap();
    assert_eq!(a["quantity_milli"], 41_300);

    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-03-06", "category": "fuel", "title": "Fuel", "quantity_milli": -1
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "a negative quantity is rejected");

    let no_counter = app.create_object(&app.client, "Drill", None).await;
    let nid = no_counter["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{nid}/activities"))).json(&json!({
        "date": "2026-03-06", "category": "fuel", "title": "Fuel", "quantity_milli": 1000
    })).send().await.unwrap();
    assert_eq!(res.status(), 400, "a quantity without a counter cannot become consumption");
}
```

Append to `tests/objects.rs`:

```rust
#[tokio::test]
async fn fuel_unit_round_trips_and_is_validated() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "E-bike", "category": "bike", "counter_unit": "km", "fuel_unit": "kwh"
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let bike: serde_json::Value = res.json().await.unwrap();
    assert_eq!(bike["fuel_unit"], "kwh");

    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Car", "category": "car", "fuel_unit": "barrels"
    })).send().await.unwrap();
    assert_eq!(res.status(), 400);
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test --test activities fuel_quantity && cargo test --test objects fuel_unit`
Expected: FAIL — `quantity_milli` and `fuel_unit` are dropped by serde on input and absent from the response, so both `assert_eq!` on the echoed value fail.

- [ ] **Step 3: Write the migration**

`migrations/0003_fuel_quantity.sql`:

```sql
-- Fuel amount, scaled by 1000 like cost is scaled by 100: the database stores no floats.
ALTER TABLE activities ADD COLUMN quantity_milli INTEGER;

-- Which unit that quantity is in. NULL derives from counter_unit (km -> l, mi -> gal, h -> l);
-- explicit so an e-bike in the same instance as a petrol car can record kwh.
ALTER TABLE objects ADD COLUMN fuel_unit TEXT CHECK (fuel_unit IN ('l', 'gal', 'kwh'));
```

- [ ] **Step 4: Thread `quantity_milli` through the activity API**

In `src/api/activities.rs`:

- Add `pub quantity_milli: Option<i64>,` to `ActivityRow`, after `cost_cents`.
- Add to `ActivityInput`:
  ```rust
      #[serde(default)]
      pub quantity_milli: Option<i64>,
  ```
- In `ActivityInput::validate`, after the `cost_cents` check:
  ```rust
      if let Some(q) = self.quantity_milli {
          if q < 0 { return Err(AppError::BadRequest("quantity_milli must be >= 0".into())); }
          if object.counter_unit.is_none() {
              return Err(AppError::BadRequest("quantity_milli needs an object with a counter".into()));
          }
      }
  ```
- Add `quantity_milli` to every `SELECT` column list in this file (`load_owned_activity`,
  `list_for_object`, and the `RETURNING` clause of `create`), to the `INSERT` column list
  with one more `?` placeholder and `.bind(body.quantity_milli)` in matching position, and
  to the `UPDATE` in `update` as `quantity_milli = ?` with its bind before `updated_at`.

- [ ] **Step 5: Thread `fuel_unit` through the object API**

In `src/api/objects.rs`:

- Add `pub fuel_unit: Option<String>,` to `ObjectRow` after `counter_unit`.
- Add to `ObjectInput`:
  ```rust
      #[serde(default)]
      pub fuel_unit: Option<String>,
  ```
- In `ObjectInput::validate`, after the `counter_unit` check:
  ```rust
      if let Some(u) = &self.fuel_unit {
          if !matches!(u.as_str(), "l" | "gal" | "kwh") {
              return Err(AppError::BadRequest("fuel_unit must be l, gal, kwh or null".into()));
          }
      }
  ```
- Add `fuel_unit` to every `SELECT` column list in the file, to the `INSERT`/`RETURNING`
  clauses, and to the `UPDATE` statement, mirroring how `counter_unit` is handled in each.

- [ ] **Step 6: Run the full backend suite**

Run: `cargo test`
Expected: PASS. Export/import tests still pass — they select named columns, so a new
nullable column does not disturb them.

- [ ] **Step 7: Carry both columns through export and import**

A column the archive does not carry is a column a restore silently erases. In
`src/api/export.rs`:

- Add `quantity_milli: Option<i64>,` to the activity struct at line 46, `quantity_milli` to
  the `SELECT` at line 131, `quantity_milli: a.quantity_milli,` to both struct literals
  (lines 152 and 362), and the column plus its `.bind(a.quantity_milli)` to the `INSERT` at
  line 285.
- Do the same for `fuel_unit` on the object struct, its `SELECT`, its literals and its
  `INSERT`, mirroring how `counter_unit` is handled in each place.

Both fields deserialize with `#[serde(default)]` so an archive exported before this change
still imports, with the new fields null.

Append to `tests/export.rs`:

```rust
#[tokio::test]
async fn export_round_trips_fuel_quantity() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let res = app.client.post(app.url("/objects")).json(&json!({
        "name": "Golf", "category": "car", "counter_unit": "km", "fuel_unit": "l"
    })).send().await.unwrap();
    let car: serde_json::Value = res.json().await.unwrap();
    let id = car["id"].as_i64().unwrap();
    app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": "2026-03-05", "category": "fuel", "title": "Fuel",
        "counter_value": 12_000, "quantity_milli": 41_300
    })).send().await.unwrap();

    let zip = app.client.get(app.url("/export")).send().await.unwrap().bytes().await.unwrap();

    let fresh = common::spawn().await;
    fresh.setup("ben", "correct horse").await;
    let part = reqwest::multipart::Part::bytes(zip.to_vec())
        .file_name("export.zip").mime_str("application/zip").unwrap();
    let res = fresh.client.post(fresh.url("/import"))
        .multipart(reqwest::multipart::Form::new().part("file", part))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let objects: Vec<serde_json::Value> = fresh.client.get(fresh.url("/objects"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(objects[0]["fuel_unit"], "l");
    let nid = objects[0]["id"].as_i64().unwrap();
    let acts: Vec<serde_json::Value> = fresh.client.get(fresh.url(&format!("/objects/{nid}/activities")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(acts[0]["quantity_milli"], 41_300, "the archive must not drop the quantity");
}
```

Match the existing import endpoint's path and multipart field name in `tests/export.rs` if
they differ from `/import` and `file`.

Run: `cargo test --test export`
Expected: PASS.

- [ ] **Step 8: Update the frontend types**

In `frontend/src/lib/types.ts`, add `quantity_milli: number | null` to both `Activity` and
`ActivityInput`, and `fuel_unit: 'l' | 'gal' | 'kwh' | null` to `MemObject` and `ObjectInput`.
In `frontend/src/lib/activity-form.ts`, add `quantity_milli: null` to `emptyActivity()` and
`quantity_milli: a.quantity_milli` to `toActivityInput()`.

Run: `cd frontend && npm test && npm run check`
Expected: PASS with 0 errors.

- [ ] **Step 9: Commit**

```bash
git add migrations src frontend/src/lib/types.ts frontend/src/lib/activity-form.ts tests
git commit -m "feat: record how much fuel a fill-up held

Consumption is the one number a fuel log cannot reconstruct after the fact, so
the quantity has to be captured at the point of logging. Stored in milli-units
for the same reason money is stored in cents."
```

---

### Task 4: Insights arithmetic and endpoint

**Files:**
- Create: `src/domain/insights.rs`, `src/api/insights.rs`
- Modify: `src/domain/mod.rs`, `src/api/mod.rs`
- Test: unit tests inside `src/domain/insights.rs`; `tests/insights.rs` (new)

**Interfaces:**
- Consumes: `quantity_milli` from Task 3.
- Produces:
  - `domain::insights::Fill { counter: i64, quantity_milli: i64 }`
  - `domain::insights::consumption_per_100_milli(fills: &[Fill]) -> Option<i64>`
  - `domain::insights::cost_per_counter_milli(total_cost_cents: i64, span: i64) -> Option<i64>`
  - `GET /api/objects/{id}/insights`

- [ ] **Step 1: Write the failing unit tests**

Create `src/domain/insights.rs` containing only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn f(counter: i64, quantity_milli: i64) -> Fill { Fill { counter, quantity_milli } }

    #[test]
    fn consumption_excludes_the_first_fill() {
        // 40 L burned over 800 km -> 5 L/100 km. The first fill's fuel was burned before
        // the window opened, so only its odometer reading counts, not its litres.
        let fills = [f(10_000, 45_000), f(10_400, 20_000), f(10_800, 20_000)];
        assert_eq!(consumption_per_100_milli(&fills), Some(5_000));
    }

    #[test]
    fn a_single_fill_cannot_produce_consumption() {
        assert_eq!(consumption_per_100_milli(&[f(10_000, 45_000)]), None);
        assert_eq!(consumption_per_100_milli(&[]), None);
    }

    #[test]
    fn a_zero_span_produces_nothing_rather_than_dividing_by_zero() {
        assert_eq!(consumption_per_100_milli(&[f(10_000, 45_000), f(10_000, 20_000)]), None);
    }

    #[test]
    fn fills_out_of_order_are_sorted_before_measuring() {
        let fills = [f(10_800, 20_000), f(10_000, 45_000), f(10_400, 20_000)];
        assert_eq!(consumption_per_100_milli(&fills), Some(5_000));
    }

    #[test]
    fn cost_per_counter_needs_a_span() {
        assert_eq!(cost_per_counter_milli(48_000, 19_230), Some(2_496));
        assert_eq!(cost_per_counter_milli(48_000, 0), None);
        assert_eq!(cost_per_counter_milli(48_000, -5), None);
    }
}
```

- [ ] **Step 2: Register the module and watch it fail**

In `src/domain/mod.rs`:

```rust
pub mod insights;
pub mod reminder;
```

Run: `cargo test --lib insights`
Expected: FAIL — `cannot find type 'Fill' in this scope` and the two functions unresolved.

- [ ] **Step 3: Implement the arithmetic**

Prepend to `src/domain/insights.rs`:

```rust
/// One fuel entry that carries both an odometer reading and an amount.
#[derive(Clone, Copy, Debug)]
pub struct Fill {
    pub counter: i64,
    pub quantity_milli: i64,
}

/// Quantity burned per 100 counter units, scaled by 1000, or None when it cannot be measured.
///
/// The standard tank method: the earliest fill only marks where the window opens -- its fuel
/// was burned before it -- so every later fill's quantity is divided by the distance from the
/// first fill to the last.
pub fn consumption_per_100_milli(fills: &[Fill]) -> Option<i64> {
    if fills.len() < 2 { return None; }
    let mut sorted = fills.to_vec();
    sorted.sort_by_key(|f| f.counter);
    let span = sorted.last()?.counter - sorted.first()?.counter;
    if span <= 0 { return None; }
    let burned: i64 = sorted[1..].iter().map(|f| f.quantity_milli).sum();
    Some(burned * 100 / span)
}

/// Cents per counter unit, scaled by 1000, or None when the object has not moved.
pub fn cost_per_counter_milli(total_cost_cents: i64, span: i64) -> Option<i64> {
    if span <= 0 { return None; }
    Some(total_cost_cents * 1000 / span)
}
```

`sorted` needs `Vec`, so `fills.to_vec()` requires `Fill: Clone` — the derive above covers it.

- [ ] **Step 4: Run the unit tests**

Run: `cargo test --lib insights`
Expected: PASS, all five tests.

- [ ] **Step 5: Commit the domain layer**

```bash
git add src/domain
git commit -m "feat: measure fuel consumption and cost per counter unit

Kept as pure functions over plain values so the tank method -- the first fill
marks the start of the window and contributes no fuel to it -- is testable
without a database."
```

- [ ] **Step 6: Write the failing endpoint test**

Create `tests/insights.rs`:

```rust
mod common;
use serde_json::json;

#[tokio::test]
async fn insights_roll_up_cost_and_consumption() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    for (date, category, title, cost, counter, qty) in [
        ("2025-06-01", "fuel", "Fuel", 5_000, 10_000, Some(45_000)),
        ("2026-01-10", "fuel", "Fuel", 4_000, 10_400, Some(20_000)),
        ("2026-02-10", "fuel", "Fuel", 4_000, 10_800, Some(20_000)),
        ("2026-03-10", "repair", "Brakes", 30_000, 10_900, None),
    ] {
        let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
            "date": date, "category": category, "title": title,
            "cost_cents": cost, "counter_value": counter, "quantity_milli": qty
        })).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let out: serde_json::Value = app.client
        .get(app.url(&format!("/objects/{id}/insights")))
        .send().await.unwrap().json().await.unwrap();

    let years = out["by_year"].as_array().unwrap();
    assert_eq!(years[0]["bucket"], "2026", "newest year first");
    assert_eq!(years[0]["cost_cents"], 38_000);
    assert_eq!(years[0]["count"], 3);
    assert_eq!(years[1]["bucket"], "2025");
    assert_eq!(years[1]["cost_cents"], 5_000);

    let cats = out["by_category"].as_array().unwrap();
    let repair = cats.iter().find(|c| c["bucket"] == "repair").unwrap();
    assert_eq!(repair["cost_cents"], 30_000);

    assert_eq!(out["counter_span"]["from"], 10_000);
    assert_eq!(out["counter_span"]["to"], 10_900);
    // 43_000 cents over 900 km
    assert_eq!(out["cost_per_counter_milli"], 47_777);
    assert_eq!(out["fuel"]["unit"], "l");
    assert_eq!(out["fuel"]["quantity_milli"], 85_000);
    // 40 L over 800 km
    assert_eq!(out["fuel"]["per_100_milli"], 5_000);
}

#[tokio::test]
async fn insights_of_an_empty_object_are_all_null() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let out: serde_json::Value = app.client
        .get(app.url(&format!("/objects/{id}/insights")))
        .send().await.unwrap().json().await.unwrap();
    assert!(out["by_year"].as_array().unwrap().is_empty());
    assert!(out["cost_per_counter_milli"].is_null());
    assert!(out["counter_span"].is_null());
    assert!(out["fuel"].is_null());
}

#[tokio::test]
async fn insights_of_another_users_object_are_404() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = anna.get(app.url(&format!("/objects/{id}/insights"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
```

- [ ] **Step 7: Run it and watch it fail**

Run: `cargo test --test insights`
Expected: FAIL — the route does not exist; the JSON body is the SPA fallback, so indexing
`out["by_year"]` yields null and `as_array()` panics.

- [ ] **Step 8: Implement the endpoint**

Create `src/api/insights.rs`:

```rust
use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::domain::insights::{consumption_per_100_milli, cost_per_counter_milli, Fill};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub fn router() -> Router<App> {
    Router::new().route("/objects/{id}/insights", get(read))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Bucket {
    pub bucket: String,
    pub cost_cents: i64,
    pub count: i64,
}

#[derive(Serialize)]
pub struct Span {
    pub from: i64,
    pub to: i64,
}

#[derive(Serialize)]
pub struct FuelOut {
    pub unit: String,
    pub quantity_milli: i64,
    pub per_100_milli: Option<i64>,
    pub cost_per_counter_milli: Option<i64>,
}

#[derive(Serialize)]
pub struct InsightsOut {
    pub by_year: Vec<Bucket>,
    pub by_category: Vec<Bucket>,
    pub counter_span: Option<Span>,
    pub cost_per_counter_milli: Option<i64>,
    pub fuel: Option<FuelOut>,
}

/// The unit a quantity is in when the object does not name one: petrol countries measure
/// kilometres in litres and miles in gallons.
fn default_fuel_unit(counter_unit: Option<&str>) -> &'static str {
    match counter_unit {
        Some("mi") => "gal",
        _ => "l",
    }
}

async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
) -> Result<Json<InsightsOut>, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;

    let by_year = sqlx::query_as::<_, Bucket>(
        "SELECT substr(date, 1, 4) AS bucket, COALESCE(SUM(cost_cents), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id = ? GROUP BY bucket ORDER BY bucket DESC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    let by_category = sqlx::query_as::<_, Bucket>(
        "SELECT category AS bucket, COALESCE(SUM(cost_cents), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id = ? GROUP BY category ORDER BY cost_cents DESC",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    let (min_counter, max_counter, total_cost): (Option<i64>, Option<i64>, i64) = sqlx::query_as(
        "SELECT MIN(counter_value), MAX(counter_value), COALESCE(SUM(cost_cents), 0) \
         FROM activities WHERE object_id = ?",
    )
    .bind(object_id)
    .fetch_one(&state.db)
    .await?;

    let counter_span = min_counter.zip(max_counter).map(|(from, to)| Span { from, to });
    let span = counter_span.as_ref().map(|s| s.to - s.from).unwrap_or(0);
    let cost_per_counter_milli = cost_per_counter_milli(total_cost, span);

    let fill_rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT counter_value, quantity_milli FROM activities \
         WHERE object_id = ? AND category = 'fuel' AND counter_value IS NOT NULL \
         AND quantity_milli IS NOT NULL ORDER BY counter_value",
    )
    .bind(object_id)
    .fetch_all(&state.db)
    .await?;

    let fuel = if fill_rows.is_empty() {
        None
    } else {
        let fills: Vec<Fill> = fill_rows
            .iter()
            .map(|(counter, quantity_milli)| Fill { counter: *counter, quantity_milli: *quantity_milli })
            .collect();
        let fuel_cost: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(cost_cents), 0) FROM activities WHERE object_id = ? AND category = 'fuel'",
        )
        .bind(object_id)
        .fetch_one(&state.db)
        .await?;
        Some(FuelOut {
            unit: object
                .fuel_unit
                .clone()
                .unwrap_or_else(|| default_fuel_unit(object.counter_unit.as_deref()).to_string()),
            quantity_milli: fills.iter().map(|f| f.quantity_milli).sum(),
            per_100_milli: consumption_per_100_milli(&fills),
            cost_per_counter_milli: cost_per_counter_milli(fuel_cost, span),
        })
    };

    Ok(Json(InsightsOut { by_year, by_category, counter_span, cost_per_counter_milli, fuel }))
}
```

Register it in `src/api/mod.rs`: add `pub mod insights;` to the module list and
`.merge(insights::router())` to `router()`, after `.merge(objects::router())`.

- [ ] **Step 9: Run the tests**

Run: `cargo test --test insights && cargo test`
Expected: PASS across the board.

- [ ] **Step 10: Commit**

```bash
git add src/api tests/insights.rs
git commit -m "feat: report what an object has cost and consumed

Rollups by year and category, cost per counter unit, and fuel consumption in
one call, so the Info tab needs a single request rather than paging the whole
timeline into the browser to add it up there."
```

---

### Task 5: Insights and quantity in the UI

**Files:**
- Create: `frontend/src/lib/Insights.svelte`
- Modify: `frontend/src/routes/ObjectDetail.svelte`, `frontend/src/routes/ActivityForm.svelte`, `frontend/src/routes/ObjectForm.svelte`, `frontend/src/lib/types.ts`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/format.test.ts`

**Interfaces:**
- Consumes: `GET /objects/{id}/insights` from Task 4.
- Produces: `Insights` interface in `types.ts`; `perCounter(milli, currency, locale)` and `quantity(milli, unit, locale)` in `frontend/src/lib/format.ts`.

- [ ] **Step 1: Write the failing formatter tests**

Append to `frontend/tests/format.test.ts`:

Extend the existing `from '../src/lib/format'` import on line 2 with `perCounter` and
`quantity` rather than adding a second import line, then append:

```ts

describe('perCounter', () => {
  it('renders milli-cents per unit as money with two decimals', () => {
    // 47_777 is cents-per-unit x 1000 -- 47.777 cents/km, i.e. 48 cents.
    expect(perCounter(47_777, 'EUR', 'en')).toBe('€0.48');
  });
  it('renders nothing when there is no value', () => {
    expect(perCounter(null, 'EUR', 'en')).toBe('');
  });
});

describe('quantity', () => {
  it('renders milli-units with one decimal and the unit', () => {
    expect(quantity(41_300, 'l', 'en')).toBe('41.3 l');
  });
  it('renders nothing when there is no value', () => {
    expect(quantity(null, 'l', 'en')).toBe('');
  });
});
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npx vitest run tests/format.test.ts`
Expected: FAIL — "No 'perCounter' export is defined".

- [ ] **Step 3: Implement the formatters**

Append to `frontend/src/lib/format.ts`:

```ts
/** Cents-per-unit, scaled by 1000, as money. */
export function perCounter(milli: number | null | undefined, currency: string, locale: string): string {
  if (milli === null || milli === undefined) return '';
  return new Intl.NumberFormat(locale, { style: 'currency', currency }).format(milli / 100_000);
}

/** A milli-scaled amount with its unit: 41_300 -> "41.3 l". */
export function quantity(milli: number | null | undefined, unit: string, locale: string): string {
  if (milli === null || milli === undefined) return '';
  const n = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(milli / 1000);
  return `${n} ${unit}`;
}
```

Run: `cd frontend && npx vitest run tests/format.test.ts`
Expected: PASS.

- [ ] **Step 4: Add the types and i18n keys**

In `frontend/src/lib/types.ts`:

```ts
export interface Bucket { bucket: string; cost_cents: number; count: number }
export interface Insights {
  by_year: Bucket[]; by_category: Bucket[];
  counter_span: { from: number; to: number } | null;
  cost_per_counter_milli: number | null;
  fuel: { unit: string; quantity_milli: number; per_100_milli: number | null; cost_per_counter_milli: number | null } | null;
}
```

In `frontend/src/i18n/en.ts`:

```ts
  'insights.title': 'Cost',
  'insights.by-year': 'Per year',
  'insights.by-category': 'Per category',
  'insights.per-counter': 'Cost per {unit}',
  'insights.consumption': 'Consumption',
  'insights.fuel-total': 'Fuel logged',
  'insights.none': 'Not enough data yet.',
  'activity.quantity': 'Amount',
  'object.fuel-unit': 'Fuel unit',
```

In `frontend/src/i18n/de.ts`:

```ts
  'insights.title': 'Kosten',
  'insights.by-year': 'Pro Jahr',
  'insights.by-category': 'Pro Kategorie',
  'insights.per-counter': 'Kosten pro {unit}',
  'insights.consumption': 'Verbrauch',
  'insights.fuel-total': 'Getankt',
  'insights.none': 'Noch zu wenig Daten.',
  'activity.quantity': 'Menge',
  'object.fuel-unit': 'Tank-Einheit',
```

- [ ] **Step 5: Build the Insights component**

Create `frontend/src/lib/Insights.svelte`:

```svelte
<script lang="ts">
  import { api } from './api';
  import { money, perCounter, quantity } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, Insights } from './types';

  let { objectId, unit }: { objectId: number; unit: CounterUnit } = $props();

  let data = $state<Insights | null>(null);
  let error = $state('');

  $effect(() => {
    objectId;
    api<Insights>('GET', `/objects/${objectId}/insights`)
      .then((d) => (data = d))
      .catch((e) => (error = (e as Error).message));
  });

  /** Bar width as a percentage of the largest bucket, so the widest bar always fills the row. */
  function pct(value: number, all: { cost_cents: number }[]): number {
    const max = Math.max(...all.map((b) => b.cost_cents), 1);
    return Math.round((value / max) * 100);
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if data}
  {#if data.by_year.length === 0}
    <p class="muted">{$t('insights.none')}</p>
  {:else}
    <h3>{$t('insights.by-year')}</h3>
    {#each data.by_year as b (b.bucket)}
      <div class="bar-row">
        <span class="label">{b.bucket}</span>
        <span class="track"><span class="fill" style={`width:${pct(b.cost_cents, data.by_year)}%`}></span></span>
        <span class="value">{money(b.cost_cents, $currency, $locale)}</span>
      </div>
    {/each}

    <h3>{$t('insights.by-category')}</h3>
    {#each data.by_category as b (b.bucket)}
      <div class="bar-row">
        <span class="label">{$t(`cat.${b.bucket}`)}</span>
        <span class="track"><span class="fill" style={`width:${pct(b.cost_cents, data.by_category)}%`}></span></span>
        <span class="value">{money(b.cost_cents, $currency, $locale)}</span>
      </div>
    {/each}

    {#if data.cost_per_counter_milli !== null && unit}
      <p class="muted">{$t('insights.per-counter', { unit })}: <b>{perCounter(data.cost_per_counter_milli, $currency, $locale)}</b></p>
    {/if}
    {#if data.fuel}
      <p class="muted">{$t('insights.fuel-total')}: <b>{quantity(data.fuel.quantity_milli, data.fuel.unit, $locale)}</b></p>
      {#if data.fuel.per_100_milli !== null && unit}
        <p class="muted">{$t('insights.consumption')}: <b>{quantity(data.fuel.per_100_milli, data.fuel.unit, $locale)}/100 {unit}</b></p>
      {/if}
    {/if}
  {/if}
{/if}

<style>
  h3 { margin: 16px 0 8px; font-size: 1rem; }
  .bar-row { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  .label { flex: none; width: 90px; font-size: .9rem; }
  .track { flex: 1; height: 10px; background: var(--surface-2); border-radius: 5px; overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
  .value { flex: none; font-size: .9rem; }
</style>
```

- [ ] **Step 6: Mount it in the Info tab**

In `frontend/src/routes/ObjectDetail.svelte`, import it (`import Insights from '../lib/Insights.svelte';`)
and render it in the `{:else}` branch of the tab block, directly above `<div class="list info-actions">`:

```svelte
      <h3>{$t('insights.title')}</h3>
      <Insights objectId={oid} unit={object.counter_unit} />
```

- [ ] **Step 7: Add the quantity field to the activity form**

In `frontend/src/routes/ActivityForm.svelte`, add `let quantityText = $state('');`, set it in
`onMount` for an edited activity (`quantityText = a.quantity_milli === null ? '' : String(a.quantity_milli / 1000);`),
include it in `buildInput()`:

```ts
      quantity_milli: String(quantityText).trim() === '' ? null : Math.round(Number(quantityText) * 1000),
```

and render it next to the cost field, only for fuel:

```svelte
      {#if input.category === 'fuel' && object?.counter_unit}
        <div class="field">
          <label for="qt">{$t('activity.quantity')} ({object.fuel_unit ?? (object.counter_unit === 'mi' ? 'gal' : 'l')})</label>
          <input id="qt" type="text" inputmode="decimal" bind:value={quantityText} />
        </div>
      {/if}
```

- [ ] **Step 8: Add the fuel-unit selector to the object form**

In `frontend/src/routes/ObjectForm.svelte`, beside the existing counter-unit select, following
the same markup pattern already used there:

```svelte
    <div class="field">
      <label for="fu">{$t('object.fuel-unit')}</label>
      <select id="fu" bind:value={input.fuel_unit}>
        <option value={null}>{$t('object.counter-none')}</option>
        <option value="l">l</option>
        <option value="gal">gal</option>
        <option value="kwh">kwh</option>
      </select>
    </div>
```

Add `fuel_unit: null` to whichever helper in that file builds an empty object input, and
`fuel_unit: o.fuel_unit` where an existing object is loaded into the form.

- [ ] **Step 9: Verify**

Run: `cd frontend && npm test && npm run check`
Expected: PASS, 0 errors.

- [ ] **Step 10: Commit**

```bash
git add frontend/src frontend/tests
git commit -m "feat: show what an object cost, per year and per category

Bars are divs with a width percentage rather than a charting library: the whole
point of this app is one small binary, and a chart bundle would outweigh the
frontend it is drawn on."
```

---

### Task 6: Upcoming reminders and snooze

**Files:**
- Modify: `src/domain/reminder.rs`, `src/api/reminders.rs`, `frontend/src/routes/Dashboard.svelte`, `frontend/src/lib/types.ts`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: unit tests in `src/domain/reminder.rs`; `tests/reminders.rs`

**Interfaces:**
- Produces:
  - `domain::reminder::snoozed_date(today: NaiveDate, current: Option<NaiveDate>, days: i64) -> NaiveDate`
  - `GET /api/reminders/due?within_days=N` with `days_until` and `counter_until` on each item
  - `POST /api/reminders/{id}/snooze { "days": n }`

- [ ] **Step 1: Write the failing unit test**

Append inside the `mod tests` block of `src/domain/reminder.rs`:

```rust
    #[test]
    fn snooze_runs_from_today_when_the_reminder_is_overdue() {
        // Three months overdue plus seven days would still be in the past, which is not
        // what pressing snooze means.
        let overdue = Some(d("2026-06-01"));
        assert_eq!(snoozed_date(d("2026-09-06"), overdue, 7), d("2026-09-13"));
    }

    #[test]
    fn snooze_runs_from_the_due_date_when_it_is_still_ahead() {
        let future = Some(d("2026-10-01"));
        assert_eq!(snoozed_date(d("2026-09-06"), future, 7), d("2026-10-08"));
    }

    #[test]
    fn snoozing_a_counter_only_reminder_gives_it_a_date() {
        assert_eq!(snoozed_date(d("2026-09-06"), None, 7), d("2026-09-13"));
    }
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --lib reminder::tests::snooze`
Expected: FAIL — `cannot find function 'snoozed_date' in this scope`.

- [ ] **Step 3: Implement it**

Add to `src/domain/reminder.rs`, above the tests:

```rust
use chrono::Days;

/// Where a snoozed reminder lands: `days` after the later of today and its current due date.
pub fn snoozed_date(today: NaiveDate, current: Option<NaiveDate>, days: i64) -> NaiveDate {
    let base = match current {
        Some(d) if d > today => d,
        _ => today,
    };
    base.checked_add_days(Days::new(days.max(0) as u64)).unwrap_or(base)
}

/// Days from today until `due`; negative when it has passed. None when there is no date.
pub fn days_until(today: NaiveDate, due: Option<NaiveDate>) -> Option<i64> {
    due.map(|d| (d - today).num_days())
}

/// Counter units still to go before `due`; negative when passed. None without both readings.
pub fn counter_until(current: Option<i64>, due: Option<i64>) -> Option<i64> {
    current.zip(due).map(|(c, d)| d - c)
}
```

Run: `cargo test --lib reminder`
Expected: PASS.

- [ ] **Step 4: Write the failing endpoint tests**

Append to `tests/reminders.rs`:

```rust
#[tokio::test]
async fn due_list_can_look_ahead() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let soon = (chrono::Utc::now().date_naive() + chrono::Duration::days(10)).to_string();
    let far = (chrono::Utc::now().date_naive() + chrono::Duration::days(90)).to_string();

    for (title, date) in [("Soon", &soon), ("Far", &far)] {
        let res = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
            "title": title, "notes": "", "due_date": date
        })).send().await.unwrap();
        assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    }

    let now: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due"))
        .send().await.unwrap().json().await.unwrap();
    assert!(now.is_empty(), "nothing is due yet, and the default must not change");

    let ahead: Vec<serde_json::Value> = app.client.get(app.url("/reminders/due?within_days=30"))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(ahead.len(), 1, "only the reminder inside the window");
    assert_eq!(ahead[0]["title"], "Soon");
    assert_eq!(ahead[0]["due"], false);
    assert_eq!(ahead[0]["days_until"], 10);
}

#[tokio::test]
async fn snooze_pushes_an_overdue_reminder_into_the_future() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let res = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap();
    let r: serde_json::Value = res.json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(r["due"], true);

    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    let out: serde_json::Value = res.json().await.unwrap();
    assert_eq!(out["due"], false, "a snoozed reminder is no longer due");
    let expected = (chrono::Utc::now().date_naive() + chrono::Duration::days(7)).to_string();
    assert_eq!(out["due_date"], expected);

    for bad in [json!({ "days": 0 }), json!({ "days": 400 })] {
        let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
            .json(&bad).send().await.unwrap();
        assert_eq!(res.status(), 400, "{bad}");
    }
}

#[tokio::test]
async fn a_done_reminder_cannot_be_snoozed() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let r: serde_json::Value = app.client.post(app.url(&format!("/objects/{id}/reminders"))).json(&json!({
        "title": "Oil change", "notes": "", "due_date": "2020-01-01"
    })).send().await.unwrap().json().await.unwrap();
    let rid = r["id"].as_i64().unwrap();
    assert_eq!(app.client.post(app.url(&format!("/reminders/{rid}/done"))).json(&json!({}))
        .send().await.unwrap().status(), 200);
    let res = app.client.post(app.url(&format!("/reminders/{rid}/snooze")))
        .json(&json!({ "days": 7 })).send().await.unwrap();
    assert_eq!(res.status(), 409);
}
```

If `tests/reminders.rs` does not already `use serde_json::json;`, add it at the top.

- [ ] **Step 5: Run and watch them fail**

Run: `cargo test --test reminders`
Expected: FAIL — `within_days` is ignored (the lookahead list comes back empty) and
`/snooze` is 404.

- [ ] **Step 6: Implement the lookahead**

In `src/api/reminders.rs`:

```rust
use crate::domain::reminder::{counter_until, days_until, is_due, next_due, snoozed_date, Repeat};
```

Extend `ReminderOut` and its `From` impl:

```rust
#[derive(Serialize)]
pub struct ReminderOut {
    #[serde(flatten)]
    pub row: ReminderRow,
    pub due: bool,
    /// Days from today until the due date; negative when it has passed.
    pub days_until: Option<i64>,
    /// Counter units still to go; negative when passed.
    pub counter_until: Option<i64>,
}

impl From<ReminderRow> for ReminderOut {
    fn from(row: ReminderRow) -> Self {
        let today = today();
        let date = row.due_date.as_deref().and_then(parse_date);
        let due = row.done_at.is_none() && is_due(today, row.current_counter, date, row.due_counter);
        let days_until = days_until(today, date);
        let counter_until = counter_until(row.current_counter, row.due_counter);
        ReminderOut { row, due, days_until, counter_until }
    }
}
```

Replace the filter in `due_for_user` and give it a window. The existing signature is
`due_for_user(state, user_id)`; add the parameter and update the notify caller in
`src/notify.rs` to pass `0`:

```rust
/// Reminders that are due, plus those coming due within `within_days`. `0` -- the default --
/// reproduces the old behaviour exactly, which is what the daily digest wants.
pub async fn due_for_user(state: &App, user_id: i64, within_days: i64) -> Result<Vec<ReminderOut>, AppError> {
    // ...existing query unchanged, then:
    Ok(rows
        .into_iter()
        .map(ReminderOut::from)
        .filter(|r| r.row.done_at.is_none())
        .filter(|r| r.due || matches!(r.days_until, Some(d) if d > 0 && d <= within_days))
        .collect())
}

#[derive(Deserialize)]
pub struct DueQuery {
    #[serde(default)]
    pub within_days: i64,
}

async fn due_list(user: AuthUser, State(state): State<App>, Query(q): Query<DueQuery>) -> Result<Json<Vec<ReminderOut>>, AppError> {
    Ok(Json(due_for_user(&state, user.id, q.within_days.clamp(0, 365)).await?))
}
```

- [ ] **Step 7: Implement snooze**

> **Superseded during implementation.** Snooze ships as a *suppression* — a `snoozed_until`
> column (migration `0004`) gates `is_due` — not as a rewrite of `due_date`. Rewriting the date
> cannot suppress a reminder that is due by counter, so the button did nothing in exactly the
> case you would press it. The step below, and the `due_date` assertion in its test, describe
> the abandoned design; see the spec's C3 section for what shipped.

Add the route to `router()`:

```rust
        .route("/reminders/{id}/snooze", post(snooze))
```

And the handler:

```rust
#[derive(Deserialize)]
pub struct SnoozeInput {
    pub days: i64,
}

/// Push a reminder out by `days`. Snoozing means "not now, in a week", so an overdue
/// reminder is measured from today rather than from the date it blew past.
async fn snooze(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    Json(body): Json<SnoozeInput>,
) -> Result<Json<ReminderOut>, AppError> {
    if !(1..=365).contains(&body.days) {
        return Err(AppError::BadRequest("days must be between 1 and 365".into()));
    }
    let r = load_owned(&state, user.id, id).await?;
    if r.done_at.is_some() {
        return Err(AppError::Conflict("reminder already done".into()));
    }
    let next = snoozed_date(today(), r.due_date.as_deref().and_then(parse_date), body.days);
    sqlx::query("UPDATE reminders SET due_date = ? WHERE id = ?")
        .bind(next.to_string())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(load_owned(&state, user.id, id).await?.into()))
}
```

A counter-only reminder gains a date here; the table's
`CHECK (due_date IS NOT NULL OR due_counter IS NOT NULL)` is satisfied either way.

- [ ] **Step 8: Run the backend suite**

Run: `cargo test`
Expected: PASS, including `tests/notify.rs`, which must still see exactly today's due set.

- [ ] **Step 9: Commit the backend**

```bash
git add src tests
git commit -m "feat: look ahead at reminders and let them be snoozed

Finding out something was due only after it was due is the failure mode of a
reminder list. within_days defaults to 0 so the daily digest is untouched."
```

- [ ] **Step 10: Split the dashboard banner**

In `frontend/src/lib/types.ts`, add `days_until: number | null; counter_until: number | null` to `Reminder`.

In `frontend/src/i18n/en.ts` / `de.ts`:

```ts
  'dash.upcoming': 'Coming up',
  'dash.in-days': 'in {n} days',
  'dash.in-counter': 'in {n} {unit}',
  'reminder.snooze': 'Snooze a week',
```

```ts
  'dash.upcoming': 'Demnächst',
  'dash.in-days': 'in {n} Tagen',
  'dash.in-counter': 'in {n} {unit}',
  'reminder.snooze': 'Eine Woche später',
```

In `frontend/src/routes/Dashboard.svelte`, fetch the window and split the list:

```ts
  let soon = $state<Reminder[]>([]);

  async function load() {
    loading = true; error = '';
    try {
      objects = await api<MemObject[]>('GET', `/objects?archived=${archived}`);
      const all = await api<Reminder[]>('GET', '/reminders/due?within_days=30');
      due = all.filter((r) => r.due);
      soon = all.filter((r) => !r.due);
    } catch (e) { error = (e as Error).message; } finally { loading = false; }
  }

  async function snooze(r: Reminder) {
    try { await api('POST', `/reminders/${r.id}/snooze`, { days: 7 }); await load(); }
    catch (e) { error = (e as Error).message; }
  }
```

Render the second group below the existing banner:

```svelte
  {#if soon.length > 0}
    <div class="banner soon">
      <b>{$t('dash.upcoming')}</b>
      <ul>
        {#each soon.slice(0, 5) as r (r.id)}
          <li>
            <a href={`/objects/${r.object_id}`} onclick={(e) => { e.preventDefault(); go(`/objects/${r.object_id}?tab=reminders`); }}>{r.object_name}: {r.title}</a>
            <span class="muted">
              {#if r.days_until !== null}{$t('dash.in-days', { n: r.days_until })}{/if}
              {#if r.counter_until !== null && r.counter_unit} · {$t('dash.in-counter', { n: r.counter_until, unit: r.counter_unit })}{/if}
            </span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
```

Add a snooze button to each overdue row in the first banner:

```svelte
          <li>
            <a href={`/objects/${r.object_id}`} onclick={(e) => { e.preventDefault(); go(`/objects/${r.object_id}?tab=reminders`); }}>{r.object_name}: {r.title}</a>
            <button class="ghost snooze" onclick={() => snooze(r)}>{$t('reminder.snooze')}</button>
          </li>
```

```css
  .banner.soon { border-color: var(--border); }
  .snooze { font-size: .8rem; padding: 2px 6px; }
```

- [ ] **Step 11: Verify and commit**

Run: `cd frontend && npm test && npm run check`
Expected: PASS, 0 errors.

```bash
git add frontend/src
git commit -m "feat: warn before a reminder is overdue, and allow a week's grace

The banner now has two tiers: overdue, and coming up within thirty days with
the distance in days or counter units."
```

---

### Task 7: Idempotent creates

**Files:**
- Create: `migrations/0005_client_op_id.sql`
- Modify: `src/api/activities.rs`, `src/api/attachments.rs`
- Test: `tests/activities.rs`, `tests/attachments.rs`

**Interfaces:**
- Produces: `client_op_id: string | null` accepted on activity create (JSON field) and attachment upload (multipart field). A repeat create with a known id returns the existing row with **200**, not 201.

- [ ] **Step 1: Write the failing test**

Append to `tests/activities.rs`:

```rust
#[tokio::test]
async fn a_replayed_create_returns_the_first_row_instead_of_duplicating_it() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let body = json!({
        "date": "2026-03-05", "category": "fuel", "title": "Fuel",
        "cost_cents": 6210, "client_op_id": "op-abc-123"
    });

    let first = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&body).send().await.unwrap();
    assert_eq!(first.status(), 201);
    let first: serde_json::Value = first.json().await.unwrap();

    let again = app.client.post(app.url(&format!("/objects/{id}/activities")))
        .json(&body).send().await.unwrap();
    assert_eq!(again.status(), 200, "a replay is not a new creation");
    let again: serde_json::Value = again.json().await.unwrap();
    assert_eq!(again["id"], first["id"], "the same row comes back");

    let list: Vec<serde_json::Value> = app.client
        .get(app.url(&format!("/objects/{id}/activities")))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 1, "the fill-up was logged once");
}

#[tokio::test]
async fn one_client_op_id_cannot_be_reused_across_objects() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let a = app.create_object(&app.client, "Golf", Some("km")).await;
    let b = app.create_object(&app.client, "Bike", Some("km")).await;
    let (aid, bid) = (a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap());
    let body = json!({ "date": "2026-03-05", "category": "other", "title": "X", "client_op_id": "op-dup" });

    assert_eq!(app.client.post(app.url(&format!("/objects/{aid}/activities")))
        .json(&body).send().await.unwrap().status(), 201);
    let res = app.client.post(app.url(&format!("/objects/{bid}/activities")))
        .json(&body).send().await.unwrap();
    assert_eq!(res.status(), 409, "the id is the client's promise that this is the same op");
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test --test activities replayed`
Expected: FAIL — the second POST returns 201 and the list has two entries.

- [ ] **Step 3: Write the migration**

`migrations/0005_client_op_id.sql`:

```sql
-- An id the client generates before it sends, so a create it retried after a lost response
-- resolves to the row it already made rather than a second copy of it. Partial indexes so the
-- column stays free for every row written by an online client, which sends nothing.
ALTER TABLE activities ADD COLUMN client_op_id TEXT;
CREATE UNIQUE INDEX idx_activities_client_op
  ON activities(client_op_id) WHERE client_op_id IS NOT NULL;

ALTER TABLE attachments ADD COLUMN client_op_id TEXT;
CREATE UNIQUE INDEX idx_attachments_client_op
  ON attachments(client_op_id) WHERE client_op_id IS NOT NULL;
```

- [ ] **Step 4: Handle it in the activity create**

In `src/api/activities.rs`, add to `ActivityInput`:

```rust
    /// Client-generated id for this creation attempt. Present only from the offline outbox.
    #[serde(default)]
    pub client_op_id: Option<String>,
```

Add `client_op_id` to the `INSERT` column list (with its placeholder and bind) and to every
`SELECT`/`RETURNING` list — `ActivityRow` gains `pub client_op_id: Option<String>,`.

Change `create` to look first:

```rust
async fn create(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
    Json(mut body): Json<ActivityInput>,
) -> Result<Response, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    body.validate(&object)?;
    // A retry after a lost response must resolve to the row the first attempt made.
    if let Some(op) = body.client_op_id.as_deref() {
        if let Some(existing) = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
             quantity_milli, client_op_id, created_at, updated_at \
             FROM activities WHERE client_op_id = ?",
        )
        .bind(op)
        .fetch_optional(&state.db)
        .await?
        {
            if existing.object_id != object_id {
                return Err(AppError::Conflict("client_op_id already used for another object".into()));
            }
            return Ok((StatusCode::OK, Json(one_out(&state, existing).await?)).into_response());
        }
    }
    // ...existing INSERT unchanged, plus .bind(&body.client_op_id)
    Ok((StatusCode::CREATED, Json(one_out(&state, row).await?)).into_response())
}
```

Keep the column order in the `SELECT` above identical to every other `ActivityRow` query in
the file — sqlx maps by name, but a diverging list is how a column gets forgotten.

- [ ] **Step 5: Run the activity tests**

Run: `cargo test --test activities`
Expected: PASS.

- [ ] **Step 6: Do the same for attachments**

In `src/api/attachments.rs`, add `pub client_op_id: Option<String>,` to `AttachmentOut` and
`a.client_op_id` to both `SELECT` column lists in `load_owned` and `for_object`.

Declare the field beside `activity_id` in `upload`:

```rust
    let mut client_op_id: Option<String> = None;
```

Read it in the multipart loop, next to the `"activity_id"` arm:

```rust
            "client_op_id" => {
                let t = field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?;
                client_op_id = Some(t.trim().to_string());
            }
```

Check it after the loop, directly below the `if let Some(aid) = activity_id { .. }` block —
before the hashing and blob write, so a replay costs nothing:

```rust
    // A retried upload must resolve to the attachment the first attempt made, rather than
    // hanging a second row off the same file.
    if let Some(op) = client_op_id.as_deref() {
        let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE client_op_id = ?")
            .bind(op)
            .fetch_optional(&state.db)
            .await?;
        if let Some((id,)) = existing {
            let out = load_owned(&state, user.id, id).await?;
            if out.object_id != object_id {
                return Err(AppError::Conflict("client_op_id already used for another object".into()));
            }
            return Ok((StatusCode::OK, Json(out)));
        }
    }
```

The handler already returns `(StatusCode, Json<AttachmentOut>)`, so the 200 needs no change
to its signature.

Extend the final `INSERT`:

```rust
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO attachments (object_id, activity_id, file_id, kind, caption, client_op_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(object_id).bind(activity_id).bind(file_id).bind(&kind).bind(caption.trim())
    .bind(&client_op_id).bind(db::now())
    .fetch_one(&state.db).await?;
```

Append to `tests/attachments.rs`:

```rust
#[tokio::test]
async fn a_replayed_upload_returns_the_first_attachment() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();

    let send = || async {
        let part = reqwest::multipart::Part::bytes(common::PNG_1PX.to_vec())
            .file_name("a.png").mime_str("image/png").unwrap();
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("client_op_id", "up-abc-123");
        app.client.post(app.url(&format!("/objects/{id}/attachments")))
            .multipart(form).send().await.unwrap()
    };

    let first = send().await;
    assert_eq!(first.status(), 201);
    let first: serde_json::Value = first.json().await.unwrap();
    let again = send().await;
    assert_eq!(again.status(), 200);
    let again: serde_json::Value = again.json().await.unwrap();
    assert_eq!(again["id"], first["id"]);
}
```

Use whatever fixture bytes `tests/attachments.rs` already defines for a tiny PNG instead of
`common::PNG_1PX` if the constant is named differently there.

- [ ] **Step 7: Run the whole suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add migrations src tests
git commit -m "feat: make creates idempotent through a client op id

Without it, a reply lost on a flaky connection makes the client replay a create
that in fact succeeded, and the same fill-up lands in the timeline twice. The
column is NULL for every write from an online client, so the index stays empty."
```

---

### Task 8: The offline outbox

**Files:**
- Create: `frontend/src/lib/outbox.ts`, `frontend/src/lib/idb.ts`, `frontend/tests/outbox.test.ts`, `frontend/tests-e2e/04-offline.spec.ts`
- Modify: `frontend/src/lib/api.ts`, `frontend/src/lib/TopBar.svelte`, `frontend/src/routes/ActivityForm.svelte`, `frontend/src/routes/Settings.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/outbox.test.ts`, `frontend/tests-e2e/04-offline.spec.ts`

**Interfaces:**
- Consumes: `client_op_id` from Task 7.
- Produces from `frontend/src/lib/outbox.ts`:
  - `interface OutboxStore { all(): Promise<QueuedOp[]>; put(op: QueuedOp): Promise<void>; remove(id: string): Promise<void>; }`
  - `type QueuedOp = { id: string; kind: 'activity.create' | 'attachment.upload' | 'reminder.done'; path: string; body: unknown; blob?: Blob; tempId?: number; attempts: number; dead?: boolean }`
  - `enqueue(store, op)`, `replay(store, send)`, `pendingCount(store)`
- The `send` parameter of `replay` is `(op: QueuedOp) => Promise<{ id: number } | null>`, so the tests can drive it without a network.

- [ ] **Step 1: Write the failing tests**

Create `frontend/tests/outbox.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { memoryStore, enqueue, replay, pendingCount, type QueuedOp } from '../src/lib/outbox';

const op = (id: string, over: Partial<QueuedOp> = {}): QueuedOp =>
  ({ id, kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0, ...over });

describe('outbox', () => {
  it('replays in the order the ops were queued', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    await enqueue(store, op('b'));
    const seen: string[] = [];
    await replay(store, async (o) => { seen.push(o.id); return { id: 1 }; });
    expect(seen).toEqual(['a', 'b']);
    expect(await pendingCount(store)).toBe(0);
  });

  it('rewrites a temp activity id into a later upload once the create lands', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1 }));
    await enqueue(store, op('upload', { kind: 'attachment.upload', path: '/objects/1/attachments', body: { activity_id: -1 } }));
    const sent: unknown[] = [];
    await replay(store, async (o) => { sent.push(structuredClone(o.body)); return { id: 42 }; });
    expect(sent[1]).toEqual({ activity_id: 42 });
  });

  it('keeps an op queued when the send fails', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    await replay(store, async () => { throw new Error('offline'); });
    expect(await pendingCount(store)).toBe(1);
  });

  it('marks an op dead after three failures instead of retrying it forever', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    for (let i = 0; i < 3; i++) await replay(store, async () => { throw new Error('offline'); });
    const all = await store.all();
    expect(all[0].dead).toBe(true);
    expect(await pendingCount(store)).toBe(0);
  });

  it('does not replay a dead op', async () => {
    const store = memoryStore();
    await enqueue(store, op('a', { dead: true, attempts: 3 }));
    let calls = 0;
    await replay(store, async () => { calls++; return { id: 1 }; });
    expect(calls).toBe(0);
  });
});
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npx vitest run tests/outbox.test.ts`
Expected: FAIL — "Failed to resolve import '../src/lib/outbox'".

- [ ] **Step 3: Implement the outbox**

Create `frontend/src/lib/outbox.ts`:

```ts
/**
 * The queue of writes made while offline.
 *
 * Creates only. An edit or a delete queued offline would have to be reconciled against
 * whatever the server did in the meantime; a create cannot disagree with anything, which
 * is why the offline story stops here rather than growing a merge algorithm.
 */
export type OpKind = 'activity.create' | 'attachment.upload' | 'reminder.done';

export interface QueuedOp {
  /** Also the `client_op_id` sent to the server, which is what makes a replay idempotent. */
  id: string;
  kind: OpKind;
  path: string;
  body: Record<string, unknown>;
  blob?: Blob;
  /** Negative placeholder id this op's created row is known by until the server answers. */
  tempId?: number;
  attempts: number;
  dead?: boolean;
}

export interface OutboxStore {
  all(): Promise<QueuedOp[]>;
  put(op: QueuedOp): Promise<void>;
  remove(id: string): Promise<void>;
}

const MAX_ATTEMPTS = 3;

/** An in-memory store, for tests. */
export function memoryStore(): OutboxStore {
  const rows: QueuedOp[] = [];
  return {
    async all() { return rows.map((r) => ({ ...r })); },
    async put(op) {
      const i = rows.findIndex((r) => r.id === op.id);
      if (i === -1) rows.push({ ...op }); else rows[i] = { ...op };
    },
    async remove(id) {
      const i = rows.findIndex((r) => r.id === id);
      if (i !== -1) rows.splice(i, 1);
    },
  };
}

export function newOpId(): string {
  return globalThis.crypto.randomUUID();
}

export async function enqueue(store: OutboxStore, op: QueuedOp): Promise<void> {
  await store.put(op);
}

export async function pendingCount(store: OutboxStore): Promise<number> {
  return (await store.all()).filter((o) => !o.dead).length;
}

/**
 * Send every live op in order, oldest first, stopping at the first failure so ops that
 * depend on an earlier one cannot overtake it.
 */
export async function replay(
  store: OutboxStore,
  send: (op: QueuedOp) => Promise<{ id: number } | null>,
): Promise<void> {
  const resolved = new Map<number, number>();
  for (const op of await store.all()) {
    if (op.dead) continue;
    const body = { ...op.body };
    const ref = body.activity_id;
    if (typeof ref === 'number' && resolved.has(ref)) body.activity_id = resolved.get(ref);
    try {
      const out = await send({ ...op, body });
      if (op.tempId !== undefined && out) resolved.set(op.tempId, out.id);
      await store.remove(op.id);
    } catch {
      const attempts = op.attempts + 1;
      await store.put({ ...op, attempts, dead: attempts >= MAX_ATTEMPTS });
      return;
    }
  }
}
```

- [ ] **Step 4: Run the tests**

Run: `cd frontend && npx vitest run tests/outbox.test.ts`
Expected: PASS, all five.

- [ ] **Step 5: Commit the module**

```bash
git add frontend/src/lib/outbox.ts frontend/tests/outbox.test.ts
git commit -m "feat: queue offline writes in an ordered outbox

Replay stops at the first failure rather than skipping past it, because an
attachment upload that overtook the activity create it hangs on would be sent
with a temp id the server has never heard of."
```

- [ ] **Step 6: Add the IndexedDB store**

Create `frontend/src/lib/idb.ts`:

```ts
import type { OutboxStore, QueuedOp } from './outbox';

const DB = 'memto-outbox';
const STORE = 'ops';

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE, { keyPath: 'id' });
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function run<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return open().then((db) => new Promise<T>((resolve, reject) => {
    const req = fn(db.transaction(STORE, mode).objectStore(STORE));
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  }));
}

/** Insertion order is preserved: keys are UUIDs, so the queue keeps its own `queued_at`. */
export function idbStore(): OutboxStore {
  return {
    async all() {
      const rows = await run<QueuedOp[]>('readonly', (s) => s.getAll() as IDBRequest<QueuedOp[]>);
      return rows.sort((a, b) => (a.queued_at ?? 0) - (b.queued_at ?? 0));
    },
    async put(op) { await run('readwrite', (s) => s.put({ queued_at: Date.now(), ...op })); },
    async remove(id) { await run('readwrite', (s) => s.delete(id)); },
  };
}
```

Add `queued_at?: number` to `QueuedOp` in `outbox.ts` so this compiles, and note in its
doc comment that the memory store does not need it because arrays already keep order.

- [ ] **Step 7: Wire it into the API client**

In `frontend/src/lib/api.ts`, add a queueing create path used by the activity form and the
reminder-done dialog. Only a genuine network failure queues — an HTTP 4xx is a rejection,
and queueing it would retry a request the server has already refused:

```ts
import { enqueue, newOpId, pendingCount, replay, type OutboxStore, type QueuedOp } from './outbox';
import { idbStore } from './idb';

const store: OutboxStore = idbStore();

/** True for "the request never reached the server", false for "the server said no". */
function isOffline(e: unknown): boolean {
  return !navigator.onLine || e instanceof TypeError;
}

/**
 * POST that survives a dead connection: on a network failure the op is queued and replayed
 * later. `tempId` is the placeholder the caller shows in the meantime.
 */
export async function createQueued<T>(path: string, body: Record<string, unknown>, tempId?: number): Promise<T | null> {
  const id = newOpId();
  try {
    return await api<T>('POST', path, { ...body, client_op_id: id });
  } catch (e) {
    if (!isOffline(e)) throw e;
    await enqueue(store, { id, kind: 'activity.create', path, body, tempId, attempts: 0 });
    return null;
  }
}

export function flushOutbox(): Promise<void> {
  return replay(store, async (op: QueuedOp) => {
    const out = await api<{ id: number }>('POST', op.path, { ...op.body, client_op_id: op.id });
    return out ?? null;
  });
}

globalThis.addEventListener?.('online', () => { void flushOutbox(); });
```

Call `void flushOutbox();` once from `frontend/src/main.ts` at startup.

- [ ] **Step 8: Show the queue in the UI**

First, three more exports from `frontend/src/lib/api.ts`, so no component reaches into the
store directly:

```ts
export function outboxPending(): Promise<number> {
  return pendingCount(store);
}

export async function deadOps(): Promise<QueuedOp[]> {
  return (await store.all()).filter((o) => o.dead);
}

/** Revive every parked op and try again — the user's "I fixed the wifi" button. */
export async function retryDead(): Promise<void> {
  for (const op of await store.all()) {
    if (op.dead) await store.put({ ...op, dead: false, attempts: 0 });
  }
  await flushOutbox();
}
```

`frontend/src/lib/TopBar.svelte` — the badge:

```svelte
<script lang="ts">
  import { outboxPending } from './api';
  let pending = $state(0);
  async function refresh() { pending = await outboxPending(); }
  $effect(() => { refresh(); });
  globalThis.addEventListener?.('online', refresh);
  globalThis.addEventListener?.('offline', refresh);
</script>

{#if pending > 0}<span class="chip pending">{$t('outbox.pending', { n: pending })}</span>{/if}
```

Place the badge inside the existing header row, before the settings button.

`frontend/src/routes/ActivityForm.svelte` — `submit()` queues instead of failing:

```ts
      if (saved) {
        await api('PATCH', `/activities/${saved.id}`, body);
      } else {
        // null means "queued, not sent": the row exists locally and will be replayed.
        await createQueued<Activity>(`/objects/${oid}/activities`, body as unknown as Record<string, unknown>);
      }
      autoDraft = false;
      go(`/objects/${oid}`, true);
```

with `import { api, createQueued, fileUrl } from '../lib/api';` replacing the current import.

`frontend/src/routes/Settings.svelte` — the parked ops:

```svelte
<script lang="ts">
  import { deadOps, retryDead } from '../lib/api';
  import type { QueuedOp } from '../lib/outbox';
  let dead = $state<QueuedOp[]>([]);
  $effect(() => { deadOps().then((d) => (dead = d)); });
</script>

{#if dead.length > 0}
  <h2>{$t('outbox.failed')}</h2>
  <div class="list">
    {#each dead as op (op.id)}
      <div class="card">
        <b>{String(op.body.title ?? op.kind)}</b>
        <span class="muted">{op.path}</span>
      </div>
    {/each}
  </div>
  <button onclick={async () => { await retryDead(); dead = await deadOps(); }}>{$t('outbox.retry')}</button>
{/if}
```

New i18n keys in `frontend/src/i18n/en.ts`:

```ts
  'outbox.pending': '{n} waiting to send',
  'outbox.failed': 'Could not be sent',
  'outbox.retry': 'Try again',
```

and in `frontend/src/i18n/de.ts`:

```ts
  'outbox.pending': '{n} noch zu senden',
  'outbox.failed': 'Nicht gesendet',
  'outbox.retry': 'Erneut versuchen',
```

- [ ] **Step 9: Write the offline e2e test**

Create `frontend/tests-e2e/04-offline.spec.ts`. Read `frontend/tests-e2e/02-lifecycle.spec.ts`
first and reuse its setup verbatim — the suite runs serially against one shared server
(`workers: 1`, `fullyParallel: false`), so the numbered prefix and the existing login /
object-creation flow are what make a new spec fit:

```ts
import { expect, test } from '@playwright/test';

test('an activity logged offline appears once after reconnecting', async ({ page, context }) => {
  await page.goto('/');
  // ...existing login + object-creation helpers from the current e2e spec
  await page.getByRole('button', { name: /Log/ }).first().click();

  await context.setOffline(true);
  await page.getByLabel('Title').fill('Fuel');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Fuel')).toBeVisible();

  await context.setOffline(false);
  await page.reload();
  await expect(page.getByText('Fuel')).toHaveCount(1);
});
```

- [ ] **Step 10: Verify everything**

Run: `cd frontend && npm test && npm run check && npm run e2e`
Expected: vitest PASS, `svelte-check` 0 errors, Playwright PASS including the new spec.

Run: `cargo test`
Expected: PASS.

- [ ] **Step 11: Commit**

```bash
git add frontend
git commit -m "feat: let a log survive a garage with no signal

Writes made offline queue in IndexedDB and replay on reconnect, keyed by the
client op id so a replay cannot duplicate a row. An op that fails three times
is parked in Settings rather than retried forever or dropped silently."
```

---

## Verification

After Task 8, the whole batch is verified by:

```bash
cargo test
cd frontend && npm test && npm run check && npm run e2e
docker compose up -d --build && curl -fsS http://localhost:8080/api/health
```

Expected: all suites green; `/api/health` returns `{"status":"ok","version":"0.1.0"}`.

The two migrations apply to an existing database on first boot. Verify explicitly against a
copy of a populated `data/memto.db` before deploying — a new column with a partial unique
index is cheap, but "the migration ran" is a claim that needs evidence, not confidence.
