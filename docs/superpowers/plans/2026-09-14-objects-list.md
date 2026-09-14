# Objects List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The objects list gets Active/Archived tabs, instant search at any depth, five sort orders kept in the URL, and cards that show usage per month and last activity.

**Architecture:** The server adds `last_activity_date` and `counter_per_day_milli` to every object's stats. The dashboard loads active and archived objects once (`all=true`) and does tabs, search and sorting in the browser through a pure `object-list.ts` module.

**Tech Stack:** Rust (axum, sqlx `Any` on SQLite + PostgreSQL), Svelte 5 runes, TypeScript, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-14-objects-list-design.md`

## Global Constraints

- Work on branch `build-objects-list` (created from `main` before Task 1); never commit to `main`. Before every commit run `git branch --show-current` and commit only if it prints `build-objects-list` -- other sessions have used this repository through separate worktrees.
- Portable SQL only (`$1` placeholders, `MAX`, text date comparison); runs unchanged on SQLite and PostgreSQL.
- No schema change and no new endpoint.
- Every new UI string exists in `frontend/src/i18n/en.ts` and `de.ts`; German Intl output is asserted by pattern, not exact text.
- Playwright uses the project's config (`workers: 1`); do not pass `--workers`. Commands run in the foreground (timeouts up to 600000 ms); never end a turn while a command runs.
- Never weaken an assertion; re-run a failing pre-existing spec alone once and report both runs.
- Comments explain *why*.
- Commits end with exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DNUZLftSTtND7ABvN6eoGv
  ```

---

### Task 1: Server — last activity and usage rate in object stats

**Files:**
- Modify: `src/api/objects.rs`, `src/api/insights.rs`, `src/api/reminders.rs`, `docs/openapi.json`
- Create: `tests/object_list.rs`

**Interfaces:**
- Produces: `ObjectStats` JSON gains `last_activity_date: string | null` (`YYYY-MM-DD`) and `counter_per_day_milli: integer | null`, on `GET /objects`, `GET /objects/{id}`, create and update responses.
- `insights::usage_by_object(state: &App, user_id: Option<i64>, only: Option<i64>)`.

- [ ] **Step 1: Failing integration tests**

Create `tests/object_list.rs`:

```rust
mod common;
use chrono::{Duration, NaiveDate};
use serde_json::{json, Value};

fn today() -> NaiveDate {
    NaiveDate::parse_from_str(&logb::db::today(), "%Y-%m-%d").unwrap()
}

async fn entry(app: &common::TestApp, id: i64, date: NaiveDate, category: &str, counter: Option<i64>) {
    let res = app.client.post(app.url(&format!("/objects/{id}/activities"))).json(&json!({
        "date": date.to_string(), "category": category, "title": category, "notes": "", "counter_value": counter
    })).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
}

fn find(list: &Value, id: i64) -> Value {
    list.as_array().unwrap().iter().find(|o| o["id"] == id).expect("object in list").clone()
}

#[tokio::test]
async fn stats_carry_last_activity_and_the_usage_rate() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    let id = car["id"].as_i64().unwrap();
    let t = today();
    // 2,700 km over the 90 days between the readings: 30 km a day.
    entry(&app, id, t - Duration::days(100), "reading", Some(10_000)).await;
    entry(&app, id, t - Duration::days(10), "reading", Some(12_700)).await;
    entry(&app, id, t - Duration::days(3), "maintenance", None).await;
    // A planned expense next month is not recent activity.
    entry(&app, id, t + Duration::days(30), "maintenance", None).await;

    let list = app.get_json("/objects?all=true").await;
    let listed = find(&list, id);
    assert_eq!(listed["stats"]["last_activity_date"], (t - Duration::days(3)).to_string());
    assert_eq!(listed["stats"]["counter_per_day_milli"], 30_000);

    let read = app.get_json(&format!("/objects/{id}")).await;
    assert_eq!(read["stats"]["last_activity_date"], listed["stats"]["last_activity_date"]);
    assert_eq!(read["stats"]["counter_per_day_milli"], 30_000);
}

#[tokio::test]
async fn an_object_without_entries_has_neither() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = app.create_object(&app.client, "Golf", Some("km")).await;
    assert!(car["stats"]["last_activity_date"].is_null());
    assert!(car["stats"]["counter_per_day_milli"].is_null());
    let id = car["id"].as_i64().unwrap();
    let listed = find(&app.get_json("/objects?all=true").await, id);
    assert!(listed["stats"]["last_activity_date"].is_null());
    assert!(listed["stats"]["counter_per_day_milli"].is_null());
}

#[tokio::test]
async fn another_users_readings_never_set_my_rate() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let anna = app.create_user_client("anna", "password123").await;
    let mine = app.create_object(&app.client, "Mine", Some("km")).await;
    let theirs = app.create_object(&anna, "Theirs", Some("km")).await;
    let t = today();
    for (date, counter) in [(t - Duration::days(100), 0), (t, 5_000)] {
        let res = anna.post(app.url(&format!("/objects/{}/activities", theirs["id"]))).json(&json!({
            "date": date.to_string(), "category": "reading", "title": "r", "notes": "", "counter_value": counter
        })).send().await.unwrap();
        assert_eq!(res.status(), 201);
    }
    let listed = find(&app.get_json("/objects?all=true").await, mine["id"].as_i64().unwrap());
    assert!(listed["stats"]["counter_per_day_milli"].is_null());
    assert_eq!(app.get_json("/objects?all=true").await.as_array().unwrap().len(), 1);
}
```

Check before running: `app.get_json` asserts 200 and parses JSON (`tests/common/mod.rs`); `logb::db::today` is public (used by `tests/insights.rs`).

Run: `cargo test --test object_list`
Expected: FAIL — the two stats fields are missing (null where values are expected in the first test).

- [ ] **Step 2: `usage_by_object` takes an optional user**

In `src/api/insights.rs`, change the signature to

```rust
pub async fn usage_by_object(state: &App, user_id: Option<i64>, only: Option<i64>) -> Result<HashMap<i64, Usage>, AppError> {
```

update its doc comment's first line to say "for every object of `user_id` (or of anyone, for a caller that has already checked ownership of `only`)", and change the SQL condition `o.user_id = $1` to `($1 IS NULL OR o.user_id = $1)`. Update every caller to pass `Some(...)`: `src/api/insights.rs` (two calls), `src/api/reminders.rs` (three calls).

- [ ] **Step 3: Derive the two fields**

In `src/api/objects.rs`:

`ObjectStats` gains, after `last_reading_date`:

```rust
    /// The newest non-deleted activity dated today or earlier. A future-dated entry -- a planned
    /// expense -- is not recent activity.
    pub last_activity_date: Option<String>,
    /// Counter units per day over recent readings, scaled by 1000 -- the Info tab's figure, from
    /// `insights::usage_by_object`. Null until there is enough history.
    pub counter_per_day_milli: Option<i64>,
```

`DerivedRow` gains:

```rust
    last_activity_date: Option<String>,
    /// Filled in after the query, from `usage_by_object`, not read from a column.
    #[sqlx(default)]
    counter_per_day_milli: Option<i64>,
```

In `derived`'s SQL, add before the `cover_file_id` subquery:

```sql
           (SELECT MAX(date) FROM activities WHERE object_id = o.id AND deleted_at IS NULL \
              AND date <= $2) AS last_activity_date, \
```

(`$2` is already bound to `db::today()`.)

After the `due_readings` loop in `derived`, add:

```rust
    // One query for every object in scope, the same rate reminders and the Info tab use.
    for (object_id, usage) in super::insights::usage_by_object(state, user_id, only).await? {
        if let Some(row) = rows.get_mut(&object_id) {
            row.counter_per_day_milli = Some(usage.rate_milli);
        }
    }
```

In `DerivedRow::stats`, add:

```rust
            last_activity_date: self.last_activity_date.clone(),
            counter_per_day_milli: self.counter_per_day_milli,
```

If `#[sqlx(default)]` is not supported by this sqlx version's `FromRow` derive for a missing column, instead select `CAST(NULL AS BIGINT) AS counter_per_day_milli` in the SQL and drop the attribute; say which in the report.

- [ ] **Step 4: OpenAPI**

In `docs/openapi.json`, `components.schemas.ObjectStats.properties`: give the existing `last_activity_date` a `"description": "The newest entry dated today or earlier"`, and add:

```json
"counter_per_day_milli": {
  "type": ["integer", "null"],
  "format": "int64",
  "description": "Counter units per day, x1000, from recent readings; null without enough history"
}
```

- [ ] **Step 5: Verify**

1. `cargo test --test object_list` — 3 passed.
2. `cargo test --test objects --test reminders --test insights --test usage --test openapi` — all pass.
3. PostgreSQL:
   ```bash
   docker run -d --rm --name logb-list-pg -e POSTGRES_PASSWORD=pg -p 55436:5432 postgres:17
   until docker exec logb-list-pg pg_isready -U postgres; do sleep 1; done
   LOGB_TEST_DATABASE_URL=postgres://postgres:pg@127.0.0.1:55436/postgres cargo test --test object_list --test objects --test reminders
   docker stop logb-list-pg
   ```
   All pass; always stop the container.
4. `cargo test` and `cargo clippy --all-targets -- -D warnings` — all pass, clean.

- [ ] **Step 6: Commit**

```bash
git add tests/object_list.rs src/api/objects.rs src/api/insights.rs src/api/reminders.rs docs/openapi.json
git commit -m "feat: object stats carry the last activity date and the usage rate"
```

---

### Task 2: Frontend logic — types, list module, last-activity label, strings

**Files:**
- Create: `frontend/src/lib/object-list.ts`, `frontend/tests/object-list.test.ts`
- Modify: `frontend/src/lib/types.ts`, `frontend/src/lib/format.ts`, `frontend/tests/format.test.ts`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`

**Interfaces:**
- Consumes: Task 1 JSON.
- Produces (Task 3):
  ```ts
  // types.ts ObjectStats gains: last_activity_date: string | null; counter_per_day_milli: number | null;
  // object-list.ts
  export const SORT_KEYS: readonly ['name', 'last-activity', 'changed', 'cost', 'counter'];
  export type SortKey; export type ListTab = 'active' | 'archived';
  export function parseSort(value: string | null | undefined): SortKey | null;
  export function parseTab(value: string | null | undefined): ListTab;
  export function matchesQuery(o: MemObject, query: string, typeLabel: (t: ObjectType) => string): boolean;
  export function sortObjects(list: MemObject[], key: SortKey, locale: string): MemObject[];
  export interface ListRow { object: MemObject; parentName: string | null }
  export function visibleRows(active: MemObject[], archived: MemObject[], tab: ListTab, query: string, sort: SortKey, typeLabel: (t: ObjectType) => string, locale: string): ListRow[];
  // format.ts
  export function lastActivityLabel(date: string | null, today: string, locale: string): string;
  // i18n: dash.tab-active, dash.tab-archived, dash.search, dash.sort, dash.sort-name,
  //   dash.sort-last-activity, dash.sort-changed, dash.sort-cost, dash.sort-counter, dash.no-match
  ```

- [ ] **Step 1: Failing tests**

Create `frontend/tests/object-list.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { matchesQuery, parseSort, parseTab, sortObjects, visibleRows } from '../src/lib/object-list';
import type { MemObject, ObjectType } from '../src/lib/types';

let nextId = 1;
function obj(over: Partial<MemObject> & { stats?: Partial<MemObject['stats']> } = {}): MemObject {
  const id = over.id ?? nextId++;
  return {
    id, user_id: 1, name: `Object ${id}`, type: 'other', counter_unit: null, fuel_unit: null, description: '',
    purchase_date: null, purchase_price_cents: null, archived_at: null, cover_attachment_id: null, cover_file_id: null,
    parent_id: null, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    ...over,
    stats: {
      total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0,
      last_reading_date: null, last_activity_date: null, counter_per_day_milli: null,
      ...over.stats,
    },
  } as MemObject;
}
const label = (t: ObjectType) => ({ e_bike: 'E-Bike', home: 'Zuhause' } as Record<string, string>)[t] ?? t;
const names = (list: { name: string }[]) => list.map((o) => o.name);

describe('parseSort / parseTab', () => {
  it('accepts only known values', () => {
    expect(parseSort('last-activity')).toBe('last-activity');
    expect(parseSort('nonsense')).toBeNull();
    expect(parseSort(null)).toBeNull();
    expect(parseTab('archived')).toBe('archived');
    expect(parseTab('whatever')).toBe('active');
  });
});

describe('matchesQuery', () => {
  const bike = obj({ name: 'Cube Kathmandu', type: 'e_bike', description: 'Pendeln zur Arbeit' });
  it('matches name, type label and description, ignoring case and accents', () => {
    expect(matchesQuery(bike, 'kathm', label)).toBe(true);
    expect(matchesQuery(bike, 'e-bike', label)).toBe(true);
    expect(matchesQuery(bike, 'ARBEIT', label)).toBe(true);
    expect(matchesQuery(obj({ name: 'Fahrräder' }), 'fahrrader', label)).toBe(true);
    expect(matchesQuery(bike, 'boiler', label)).toBe(false);
  });
  it('treats an empty or blank query as matching everything', () => {
    expect(matchesQuery(bike, '   ', label)).toBe(true);
  });
});

describe('sortObjects', () => {
  const a = obj({ name: 'alpha', updated_at: '2026-03-01T00:00:00Z', stats: { last_activity_date: '2026-02-01', total_cost_cents: 500, current_counter: 10 } });
  const b = obj({ name: 'Bravo', updated_at: '2026-05-01T00:00:00Z', stats: { last_activity_date: '2026-06-01', total_cost_cents: 0, current_counter: null } });
  const c = obj({ name: 'charlie', updated_at: '2026-04-01T00:00:00Z', stats: { last_activity_date: null, total_cost_cents: 900, current_counter: 20 } });
  const d = obj({ name: 'delta', updated_at: '2026-04-01T00:00:00Z', stats: { last_activity_date: null, total_cost_cents: 900, current_counter: 20 } });

  it('sorts names case-insensitively', () => {
    expect(names(sortObjects([c, b, a], 'name', 'en'))).toEqual(['alpha', 'Bravo', 'charlie']);
  });
  it('puts the newest activity first and objects without any last, by name', () => {
    expect(names(sortObjects([d, c, a, b], 'last-activity', 'en'))).toEqual(['Bravo', 'alpha', 'charlie', 'delta']);
  });
  it('puts the most recently changed first, ties by name', () => {
    expect(names(sortObjects([a, d, b, c], 'changed', 'en'))).toEqual(['Bravo', 'charlie', 'delta', 'alpha']);
  });
  it('puts the highest cost first and zero cost last', () => {
    expect(names(sortObjects([b, a, d, c], 'cost', 'en'))).toEqual(['charlie', 'delta', 'alpha', 'Bravo']);
  });
  it('puts the highest counter first and no counter last', () => {
    expect(names(sortObjects([b, a, d, c], 'counter', 'en'))).toEqual(['charlie', 'delta', 'alpha', 'Bravo']);
  });
  it('does not mutate its input', () => {
    const input = [c, b, a];
    sortObjects(input, 'name', 'en');
    expect(names(input)).toEqual(['charlie', 'Bravo', 'alpha']);
  });
});

describe('visibleRows', () => {
  const house = obj({ id: 100, name: 'House', type: 'home' });
  const boiler = obj({ id: 101, name: 'Boiler', parent_id: 100 });
  const shedArchived = obj({ id: 102, name: 'Shed', parent_id: 100, archived_at: '2026-01-01T00:00:00Z' });
  const mower = obj({ id: 103, name: 'Mower', parent_id: 102 }); // live child of an archived parent
  const car = obj({ id: 104, name: 'Car' });
  const active = [house, boiler, mower, car];
  const archived = [shedArchived];

  it('shows top-level objects on the active tab, counting a child of an archived parent as top-level', () => {
    const rows = visibleRows(active, archived, 'active', '', 'name', label, 'en');
    expect(rows.map((r) => [r.object.name, r.parentName])).toEqual([['Car', null], ['House', null], ['Mower', null]]);
  });
  it('searches every depth and names the parent of a nested match', () => {
    const rows = visibleRows(active, archived, 'active', 'boil', 'name', label, 'en');
    expect(rows.map((r) => [r.object.name, r.parentName])).toEqual([['Boiler', 'House']]);
  });
  it('names an archived parent too', () => {
    const rows = visibleRows(active, archived, 'active', 'mow', 'name', label, 'en');
    expect(rows.map((r) => r.parentName)).toEqual(['Shed']);
  });
  it('lists archived objects flat, with their parent', () => {
    const rows = visibleRows(active, archived, 'archived', '', 'name', label, 'en');
    expect(rows.map((r) => [r.object.name, r.parentName])).toEqual([['Shed', 'House']]);
  });
});
```

Append to `frontend/tests/format.test.ts` (merge the import with the existing one from `../src/lib/format`):

```ts
describe('lastActivityLabel', () => {
  const today = '2026-09-14';
  it('says today, yesterday and days ago within a month', () => {
    expect(lastActivityLabel('2026-09-14', today, 'en')).toBe('today');
    expect(lastActivityLabel('2026-09-13', today, 'en')).toBe('yesterday');
    expect(lastActivityLabel('2026-09-11', today, 'en')).toBe('3 days ago');
    expect(lastActivityLabel('2026-08-15', today, 'en')).toBe('30 days ago');
  });
  it('gives month and year beyond a month', () => {
    expect(lastActivityLabel('2025-03-02', today, 'en')).toBe('Mar 2025');
    expect(lastActivityLabel('2025-03-02', today, 'de')).toMatch(/2025$/);
  });
  it('speaks the reader\'s language', () => {
    expect(lastActivityLabel('2026-09-11', today, 'de')).toMatch(/3/);
  });
  it('is empty without a date', () => {
    expect(lastActivityLabel(null, today, 'en')).toBe('');
  });
});
```

Run: `cd frontend && npx vitest run tests/object-list.test.ts tests/format.test.ts`
Expected: FAIL — module / export missing.

- [ ] **Step 2: Implement**

`frontend/src/lib/types.ts`, inside `ObjectStats`, after `last_reading_date`:

```ts
  /** The newest entry dated today or earlier. */
  last_activity_date: string | null;
  /** Counter units per day over recent readings, ×1000; null until there is enough history. */
  counter_per_day_milli: number | null;
```

Create `frontend/src/lib/object-list.ts`:

```ts
import type { MemObject, ObjectType } from './types';

export const SORT_KEYS = ['name', 'last-activity', 'changed', 'cost', 'counter'] as const;
export type SortKey = (typeof SORT_KEYS)[number];
export type ListTab = 'active' | 'archived';

export function parseSort(value: string | null | undefined): SortKey | null {
  return (SORT_KEYS as readonly string[]).includes(value ?? '') ? (value as SortKey) : null;
}

export function parseTab(value: string | null | undefined): ListTab {
  return value === 'archived' ? 'archived' : 'active';
}

/** Lower case with accents removed, so "fahrrader" finds "Fahrräder". */
function fold(s: string): string {
  return s.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase();
}

export function matchesQuery(o: MemObject, query: string, typeLabel: (t: ObjectType) => string): boolean {
  const q = fold(query.trim());
  if (!q) return true;
  return [o.name, typeLabel(o.type), o.description].some((field) => fold(field).includes(q));
}

/** The value each non-name sort orders by, highest or newest first. Null means "not known", and
 *  goes last: an object never used is not more recent than one used last year. A cost of 0 is
 *  treated as unknown for the same reason. ISO dates and RFC 3339 timestamps compare as text. */
const VALUE: Record<Exclude<SortKey, 'name'>, (o: MemObject) => string | number | null> = {
  'last-activity': (o) => o.stats.last_activity_date,
  changed: (o) => o.updated_at,
  cost: (o) => (o.stats.total_cost_cents > 0 ? o.stats.total_cost_cents : null),
  counter: (o) => o.stats.current_counter,
};

export function sortObjects(list: MemObject[], key: SortKey, locale: string): MemObject[] {
  const byName = (a: MemObject, b: MemObject) => a.name.localeCompare(b.name, locale, { sensitivity: 'base' });
  if (key === 'name') return [...list].sort(byName);
  const value = VALUE[key];
  return [...list].sort((a, b) => {
    const va = value(a);
    const vb = value(b);
    if (va === null || vb === null) return va === vb ? byName(a, b) : va === null ? 1 : -1;
    if (va !== vb) return va < vb ? 1 : -1;
    return byName(a, b);
  });
}

export interface ListRow { object: MemObject; parentName: string | null }

/** What the list shows for a tab, a query and a sort.
 *
 *  The active tab without a query shows top-level objects, as the dashboard always has -- a
 *  child is reached through its parent. An object whose parent is not active counts as
 *  top-level there, or a live child of an archived parent would appear nowhere. A query searches
 *  every depth, and the archived tab is flat; both name each object's parent so two "Filter"s in
 *  different rooms can be told apart. */
export function visibleRows(
  active: MemObject[], archived: MemObject[], tab: ListTab, query: string, sort: SortKey,
  typeLabel: (t: ObjectType) => string, locale: string,
): ListRow[] {
  const names = new Map<number, string>([...active, ...archived].map((o) => [o.id, o.name]));
  const activeIds = new Set(active.map((o) => o.id));
  const searching = query.trim() !== '';
  const pool = tab === 'archived' ? archived : active;
  const shown = pool.filter((o) => {
    if (searching) return matchesQuery(o, query, typeLabel);
    if (tab === 'archived') return true;
    return o.parent_id === null || !activeIds.has(o.parent_id);
  });
  const withParent = searching || tab === 'archived';
  return sortObjects(shown, sort, locale).map((object) => ({
    object,
    parentName: withParent && object.parent_id !== null ? names.get(object.parent_id) ?? null : null,
  }));
}
```

In `frontend/src/lib/format.ts`, add:

```ts
/** When something last happened, as a person says it: "today", "yesterday", "3 days ago" within
 *  a month, then the month and year. Dates are whole days (`YYYY-MM-DD`), so the difference is
 *  counted in UTC days and no timezone can shift it by one. */
export function lastActivityLabel(date: string | null, today: string, locale: string): string {
  if (!date) return '';
  const day = (iso: string) => Date.UTC(Number(iso.slice(0, 4)), Number(iso.slice(5, 7)) - 1, Number(iso.slice(8, 10)));
  const days = Math.round((day(today) - day(date)) / 86_400_000);
  if (days >= 0 && days <= 30) return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(-days, 'day');
  const [y, m] = date.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { month: 'short', year: 'numeric', timeZone: 'UTC' }).format(new Date(Date.UTC(y, m - 1, 15, 12)));
}
```

`frontend/src/i18n/en.ts`, after `'dash.log': 'Log',`:

```ts
  'dash.tab-active': 'Active',
  'dash.tab-archived': 'Archived',
  'dash.search': 'Search objects',
  'dash.sort': 'Sort',
  'dash.sort-name': 'Name A–Z',
  'dash.sort-last-activity': 'Last activity',
  'dash.sort-changed': 'Recently changed',
  'dash.sort-cost': 'Highest cost',
  'dash.sort-counter': 'Highest counter',
  'dash.no-match': 'No objects match “{q}”.',
```

`frontend/src/i18n/de.ts`, same position:

```ts
  'dash.tab-active': 'Aktiv',
  'dash.tab-archived': 'Archiviert',
  'dash.search': 'Objekte durchsuchen',
  'dash.sort': 'Sortieren',
  'dash.sort-name': 'Name A–Z',
  'dash.sort-last-activity': 'Letzte Aktivität',
  'dash.sort-changed': 'Zuletzt geändert',
  'dash.sort-cost': 'Höchste Kosten',
  'dash.sort-counter': 'Höchster Zählerstand',
  'dash.no-match': 'Keine Objekte passen zu „{q}“.',
```

(`dash.show-archived` stays until Task 3 removes its last use.)

- [ ] **Step 3: Verify**

`cd frontend && npx vitest run && npm run check` — all pass; 0 errors. If `check` flags other test fixtures or code that construct an `ObjectStats` literal, add the two fields there (null) — nothing else.

If an English Intl assertion fails on this Node (e.g. RelativeTimeFormat wording), report NEEDS_CONTEXT with the actual output rather than changing it.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/object-list.ts frontend/tests/object-list.test.ts frontend/src/lib/types.ts frontend/src/lib/format.ts frontend/tests/format.test.ts frontend/src/i18n/en.ts frontend/src/i18n/de.ts
git commit -m "feat: object list sorting, search and last-activity labels as pure helpers"
```

(Add any fixture files Step 3 required to the same commit.)

---

### Task 3: Dashboard and cards

**Files:**
- Modify: `frontend/src/routes/Dashboard.svelte`, `frontend/src/lib/ObjectCard.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`, `frontend/tests-e2e/12-object-hierarchy.spec.ts`, `frontend/tests-e2e/13-shell.spec.ts`, `docs/superpowers/specs/2026-09-14-objects-list-design.md`
- Create: `frontend/tests-e2e/22-objects-list.spec.ts`

**Interfaces:**
- Consumes: Task 1 stats; Task 2 `object-list.ts`, `lastActivityLabel`, strings; existing `persisted`, `todayIso`, `counter`, `money`, `ObjectCard`, `.tabs` styles in `app.css`.

- [ ] **Step 1: Failing e2e**

Create `frontend/tests-e2e/22-objects-list.spec.ts`:

```ts
import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** A date `days` before today in the browser's calendar, as the API takes it. */
function daysAgo(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() - days);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function entry(page: Page, id: number, data: Record<string, unknown>) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', title: 'Entry', ...data } });
  expect(res.ok()).toBe(true);
}

test('tabs, search at any depth, sorting that survives a reload, and a card that says usage and last activity', async ({ page }) => {
  await signInFresh(page, '22-objects-list');
  const house = await object(page, { name: 'List House', type: 'home' });
  await object(page, { name: 'List Boiler', type: 'appliance', parent_id: house });
  const bike = await object(page, { name: 'List E-Bike', type: 'e_bike', counter_unit: 'km' });
  // 2,700 km over the 90 days between the readings: 30 km a day, about 913 a month.
  await entry(page, bike, { date: daysAgo(100), category: 'reading', counter_value: 10_000 });
  await entry(page, bike, { date: daysAgo(10), category: 'reading', counter_value: 12_700 });
  await entry(page, bike, { date: daysAgo(3), category: 'maintenance' });
  const old = await object(page, { name: 'List Old Bike', type: 'bike' });
  const res = await page.request.patch(`/api/objects/${old}`, { data: { name: 'List Old Bike', type: 'bike', description: '', archived: true } });
  expect(res.ok()).toBe(true);

  await page.goto('/');
  const tabActive = page.getByRole('button', { name: /^Active/ });
  const tabArchived = page.getByRole('button', { name: /^Archived/ });
  await expect(tabActive).toContainText('2');
  await expect(tabArchived).toContainText('1');

  // The card: counter, usage per month, last activity.
  const bikeCard = page.getByRole('button', { name: /List E-Bike/ });
  await expect(bikeCard).toContainText('12,700 km');
  await expect(bikeCard).toContainText('913 km a month');
  await expect(bikeCard).toContainText(/\d+ days ago/);

  // Top-level only until searching; a search finds the boiler inside the house and says so.
  await expect(page.getByRole('button', { name: /List Boiler/ })).toHaveCount(0);
  await page.getByLabel('Search objects').fill('boil');
  const boiler = page.getByRole('button', { name: /List Boiler/ });
  await expect(boiler).toBeVisible();
  await expect(boiler).toContainText('in List House');
  await expect(page.getByRole('button', { name: /List House/ })).toHaveCount(0);
  await page.getByLabel('Search objects').fill('zzz-nothing');
  await expect(page.getByText('No objects match “zzz-nothing”.')).toBeVisible();
  await page.getByLabel('Search objects').fill('');

  // Last activity puts the used bike first; the house has no entries and goes last.
  await page.getByLabel('Sort').selectOption('last-activity');
  const cards = page.locator('.list .list-card');
  await expect(cards.first()).toContainText('List E-Bike');
  await expect(page).toHaveURL(/[?&]sort=last-activity(&|$)/);
  await page.reload();
  await expect(page.getByLabel('Sort')).toHaveValue('last-activity');
  await expect(page.locator('.list .list-card').first()).toContainText('List E-Bike');

  // Archived tab.
  await page.getByRole('button', { name: /^Archived/ }).click();
  await expect(page.getByRole('button', { name: /List Old Bike/ })).toBeVisible();
  await expect(page).toHaveURL(/[?&]tab=archived(&|$)/);
});
```

Run: `cd frontend && npm run e2e -- 22-objects-list`
Expected: FAIL (no tabs / search box).

- [ ] **Step 2: ObjectCard**

In `frontend/src/lib/ObjectCard.svelte`:
- imports: add `lastActivityLabel, todayIso` to the `./format` import.
- props: `let { object, parentName = null }: { object: MemObject; parentName?: string | null } = $props();`
- under the name `.row` div, add:
  ```svelte
      {#if parentName}<div class="muted small">{$t('search.in-parent', { name: parentName })}</div>{/if}
  ```
- replace the `type-row` div's content with (order: type · counter · usage · cost · last activity):
  ```svelte
        <Icon name={typeIcon(object.type)} size={16} />
        {$t(`type.${object.type}`)}
        {#if object.stats.current_counter !== null} · {counter(object.stats.current_counter, object.counter_unit, $locale)}{/if}
        <!-- A month is the unit people think in; rounded like the Info tab, since it is an average. -->
        {#if object.stats.counter_per_day_milli !== null && object.counter_unit} · {$t('insights.per-month', { amount: counter(Math.round(object.stats.counter_per_day_milli * 30.44 / 1000), object.counter_unit, $locale) })}{/if}
        {#if object.stats.total_cost_cents > 0} · {money(object.stats.total_cost_cents, $currency, $locale)}{/if}
        {#if object.stats.last_activity_date} · {lastActivityLabel(object.stats.last_activity_date, todayIso(), $locale)}{/if}
  ```

Check `insights.per-month` in en.ts reads `'≈ {amount} a month'`; the e2e asserts the substring `913 km a month`.

- [ ] **Step 3: Dashboard**

Rewrite the `<script>` of `frontend/src/routes/Dashboard.svelte` (keep the reminder banners, `snooze`, the empty state, the FAB and the `<style>` as they are) so that:

```ts
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import ObjectCard from '../lib/ObjectCard.svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { locale, t } from '../i18n';
  import { fmtDate } from '../lib/format';
  import { persisted } from '../stores/persisted';
  import { SORT_KEYS, parseSort, parseTab, visibleRows, type ListTab, type SortKey } from '../lib/object-list';
  import type { MemObject, ObjectType, Reminder } from '../lib/types';
  import Icon from '../lib/Icon.svelte';

  let active = $state<MemObject[]>([]);
  let archived = $state<MemObject[]>([]);
  let due = $state<Reminder[]>([]);
  let soon = $state<Reminder[]>([]);
  let loading = $state(true);
  let error = $state('');

  /** Per device, like the other view preferences; the address wins when it names a sort. */
  const rememberedSort = persisted<string>('logb.objects.sort', 'name');
  const params = new URLSearchParams(location.search);
  let tab = $state<ListTab>(parseTab(params.get('tab')));
  let sort = $state<SortKey>(parseSort(params.get('sort')) ?? parseSort($rememberedSort) ?? 'name');
  /** Session-only: a search is a moment's question, not a way of looking at the list. */
  let query = $state('');

  async function load() {
    loading = true; error = '';
    try {
      // Both tabs at every depth, once: switching tabs, searching and sorting then need no
      // request, and a search can find an object inside another. A household has tens of objects.
      [active, archived] = await Promise.all([
        api<MemObject[]>('GET', '/objects?all=true&archived=false'),
        api<MemObject[]>('GET', '/objects?all=true&archived=true'),
      ]);
      const all = await api<Reminder[]>('GET', '/reminders/due?within_days=30');
      due = all.filter((r) => r.due);
      soon = all.filter((r) => !r.due);
    } catch (e) { error = (e as Error).message; } finally { loading = false; }
  }
  onMount(load);

  function setSort(next: SortKey) { sort = next; rememberedSort.set(next); }

  // Tab and sort stay in the address, defaults left out, replaced only when it changes.
  $effect(() => {
    const url = new URL(location.href);
    if (tab === 'active') url.searchParams.delete('tab'); else url.searchParams.set('tab', tab);
    if (sort === 'name') url.searchParams.delete('sort'); else url.searchParams.set('sort', sort);
    const next = url.pathname + url.search;
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });

  const typeLabel = (ty: ObjectType) => $t(`type.${ty}`);
  const rows = $derived(visibleRows(active, archived, tab, query, sort, typeLabel, $locale));
  const activeCount = $derived(visibleRows(active, archived, 'active', '', 'name', typeLabel, $locale).length);
  const nothingYet = $derived(active.length === 0 && archived.length === 0);

  async function snooze(r: Reminder) {
    try { await api('POST', `/reminders/${r.id}/snooze`, { days: 7 }); await load(); }
    catch (e) { error = (e as Error).message; }
  }
```

Replace the archived `chips` block and the list/empty-state section in the markup with:

```svelte
  <nav class="tabs" aria-label={$t('dash.title')}>
    <button class:active={tab === 'active'} aria-pressed={tab === 'active'} onclick={() => (tab = 'active')}>
      {$t('dash.tab-active')} <span class="count muted">{activeCount}</span>
    </button>
    <button class:active={tab === 'archived'} aria-pressed={tab === 'archived'} onclick={() => (tab = 'archived')}>
      {$t('dash.tab-archived')} <span class="count muted">{archived.length}</span>
    </button>
  </nav>

  {#if error}<p class="error">{error}</p>{/if}
  {#if loading}
    <p class="muted">{$t('nav.loading')}</p>
  {:else if tab === 'active' && nothingYet}
    <!-- The one screen in the app that can say what LogB is for: it is what a new user sees
         the moment setup finishes. -->
    <div class="empty">
      <span class="empty-icon"><Icon name="object" size={40} /></span>
      <p>{$t('dash.empty')}</p>
      <button class="primary" onclick={() => go('/objects/new')}>+ {$t('dash.new')}</button>
    </div>
  {:else if tab === 'archived' && archived.length === 0}
    <div class="empty"><p>{$t('dash.none-archived')}</p></div>
  {:else}
    <div class="controls">
      <input type="search" aria-label={$t('dash.search')} placeholder={$t('dash.search')} bind:value={query} />
      <label class="sort">
        <span>{$t('dash.sort')}</span>
        <select value={sort} onchange={(e) => setSort(parseSort((e.currentTarget as HTMLSelectElement).value) ?? 'name')}>
          {#each SORT_KEYS as key (key)}<option value={key}>{$t(`dash.sort-${key}`)}</option>{/each}
        </select>
      </label>
    </div>
    {#if rows.length === 0}
      <p class="muted">{$t('dash.no-match', { q: query.trim() })}</p>
    {:else}
      <div class="list">
        {#each rows as row (row.object.id)}<ObjectCard object={row.object} parentName={row.parentName} />{/each}
      </div>
    {/if}
  {/if}

  <!-- Hidden while the first-run empty state carries the same action. -->
  {#if !nothingYet || tab === 'archived'}
    <button class="primary fab" onclick={() => go('/objects/new')}>+ {$t('dash.new')}</button>
  {/if}
```

Add to its `<style>`:

```css
  .controls { display: flex; gap: var(--space-2); flex-wrap: wrap; align-items: center; margin-bottom: var(--space-3); }
  .controls input[type='search'] { flex: 1 1 12rem; }
  .sort { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-sm); }
  .tabs .count { margin-left: var(--space-1); font-weight: normal; }
```

Check `app.css` for global `input`/`select` widths that would fight the flex layout (e.g. `width: 100%`); override only inside `.controls` if needed. Keep the markup's `class="list"` and `ObjectCard`'s `.list-card` class (the e2e uses them).

Remove `'dash.show-archived'` from `en.ts` and `de.ts`.

- [ ] **Step 4: Update existing specs**

- `frontend/tests-e2e/12-object-hierarchy.spec.ts`: replace `page.getByRole('button', { name: 'Show archived' })` with `page.getByRole('button', { name: /^Archived/ })`, and update the neighbouring comment if it says "chip".
- `frontend/tests-e2e/13-shell.spec.ts`: replace `/Show archived|Archivierte anzeigen/` with `/^(Archived|Archiviert)/`; its comment about the FAB condition: the FAB now shows on the Archived tab whatever the data, so switching to it still forces the FAB into existence — reword the comment to say that.

- [ ] **Step 5: Verify**

1. `cd frontend && npm run check && npx vitest run` — 0 errors, all pass.
2. `cd frontend && npm run e2e -- 22-objects-list 12-object-hierarchy 13-shell` — all pass.
3. Full suite: `cd frontend && npm run e2e` — all pass (project default `workers: 1`).
4. Screenshots at 390px of `/` for a user seeded as in spec 22 (active tab, and sorted by last activity), saved to `/home/ben/repo/logb/.superpowers/sdd/objects-list-390.png`; and at 1280px to `/home/ben/repo/logb/.superpowers/sdd/objects-list-1280.png`. Look at both: tabs readable with counts, search and sort on one line on desktop and wrapping cleanly on a phone, card line wraps without overflow.

- [ ] **Step 6: Spec status and commit**

Set the spec's status line to `Status: implemented.`

```bash
git add frontend/src/routes/Dashboard.svelte frontend/src/lib/ObjectCard.svelte frontend/src/i18n/en.ts frontend/src/i18n/de.ts frontend/tests-e2e/12-object-hierarchy.spec.ts frontend/tests-e2e/13-shell.spec.ts frontend/tests-e2e/22-objects-list.spec.ts docs/superpowers/specs/2026-09-14-objects-list-design.md
git commit -m "feat: objects list with active and archived tabs, search, sorting and richer cards"
```
