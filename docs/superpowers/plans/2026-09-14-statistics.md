# Statistics Screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `/stats` screen showing spend across all of a user's objects: over time, by object (children rolled up), by type, by category, with a year filter and an include-purchase-prices toggle.

**Architecture:** `GET /stats` runs three small SQL queries and hands the rows to pure functions in `src/domain/stats.rs`, which build the whole response (filtering, roll-up, sorting). The Svelte page fetches it with the year and toggle as query parameters and draws bars through a new shared `BarList.svelte`, which `Insights.svelte` also adopts.

**Tech Stack:** Rust (axum, sqlx `Any` on SQLite + PostgreSQL, chrono, serde), Svelte 5 (runes), TypeScript, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-14-statistics-design.md`

## Global Constraints

- **Prerequisite:** the uncommitted notifications work (`git status` at plan time) must be committed first. Tasks here modify files that work also touches (`en.ts`, `de.ts`, `types.ts`, `Icon.svelte`, `App.svelte`, `docs/openapi.json`, `src/api/mod.rs`). Run `git status --short`; if any of those show `M` before Task 1, stop and ask.
- Money is integer cents everywhere. Never floats on the server.
- Every SQL `SUM` is wrapped `CAST(SUM(...) AS BIGINT)` (PostgreSQL widens to NUMERIC, which sqlx `Any` cannot decode).
- SQL uses `$1`-style placeholders and only portable functions (`substr`), so it runs unchanged on SQLite and PostgreSQL.
- Every new UI string exists in both `frontend/src/i18n/en.ts` and `de.ts` (`tests/i18n.test.ts` fails otherwise).
- No emoji as icons (`tests/icons.test.ts`).
- Every route under `src/api/` must be in `docs/openapi.json` (`tests/openapi.rs`).
- Match surrounding style: comments explain *why*, as in `src/domain/insights.rs`.
- Commits end with:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DNUZLftSTtND7ABvN6eoGv
  ```

## File Map

| File | Status | Responsibility |
|---|---|---|
| `src/domain/stats.rs` | create | Pure: spend rows + objects → `Stats` response |
| `src/domain/mod.rs` | modify | `pub mod stats;` |
| `src/api/stats.rs` | create | Route, query validation, three SQL queries |
| `src/api/mod.rs` | modify | Register router |
| `tests/stats.rs` | create | Integration tests (both backends) |
| `docs/openapi.json` | modify | `/stats` path + schemas |
| `frontend/src/lib/stats.ts` | create | Pure helpers: path, labels, share, tree flattening |
| `frontend/tests/stats.test.ts` | create | Vitest for helpers |
| `frontend/src/lib/types.ts` | modify | `Stats`, `StatsObject`, `Amount` |
| `frontend/src/lib/nav.ts` + `tests/nav.test.ts` | modify | Fourth destination |
| `frontend/src/lib/Icon.svelte` | modify | `chart` icon |
| `frontend/src/i18n/en.ts`, `de.ts` | modify | `nav.stats`, `stats.*` |
| `frontend/src/lib/BarList.svelte` | create | Shared bar rows |
| `frontend/src/lib/Insights.svelte` | modify | Use `BarList` |
| `frontend/src/routes/Stats.svelte` | create | The screen |
| `frontend/src/App.svelte` | modify | Route `/stats` |
| `frontend/tests-e2e/20-statistics.spec.ts` | create | End-to-end |

---

### Task 1: Domain — build the statistics from rows

**Files:**
- Create: `src/domain/stats.rs`
- Modify: `src/domain/mod.rs`

**Interfaces:**
- Consumes: nothing.
- Produces (used by Task 2):
  ```rust
  pub const PURCHASE_PRICE: &str = "purchase_price";
  pub struct ObjectRow { pub id: i64, pub parent_id: Option<i64>, pub name: String, pub kind: String,
      pub archived: bool, pub purchase_date: Option<String>, pub purchase_price_cents: Option<i64>, pub created_at: String }
  pub struct Spend { pub object_id: i64, pub month: String /* YYYY-MM */, pub category: String, pub cost_cents: i64 }
  pub struct Amount { pub bucket: String, pub cost_cents: i64 }               // Serialize
  pub struct ObjectNode { pub id: i64, pub name: String, #[serde(rename = "type")] pub kind: String,
      pub archived: bool, pub cost_cents: i64, pub children: Vec<ObjectNode> } // Serialize
  pub struct Stats { pub total_cents: i64, pub years: Vec<String>, pub over_time: Vec<Amount>,
      pub by_object: Vec<ObjectNode>, pub by_type: Vec<Amount>, pub by_category: Vec<Amount> } // Serialize
  pub fn purchase_spend(objects: &[ObjectRow], purchased: &HashSet<i64>) -> Vec<Spend>
  pub fn summarize(objects: &[ObjectRow], spend: &[Spend], year: Option<i32>) -> Stats
  ```

- [ ] **Step 1: Register the module and write the failing tests**

`src/domain/mod.rs` becomes:

```rust
pub mod insights;
pub mod reminder;
pub mod stats;
```

Create `src/domain/stats.rs` with only the types, stub functions, and tests:

```rust
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The `by_category` bucket for purchase prices. Not an activity category: an object's
/// `purchase_price_cents` is a field of the object, and it gets its own row so it is never
/// mistaken for an activity logged under `purchase`.
pub const PURCHASE_PRICE: &str = "purchase_price";

/// One of the user's non-deleted objects, as the statistics need it.
#[derive(Clone, Debug)]
pub struct ObjectRow {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    /// The object's `type` column.
    pub kind: String,
    pub archived: bool,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    /// RFC 3339, as `db::now` writes it.
    pub created_at: String,
}

/// Money spent on one object in one month under one category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spend {
    pub object_id: i64,
    /// `YYYY-MM`.
    pub month: String,
    pub category: String,
    pub cost_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Amount {
    pub bucket: String,
    pub cost_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObjectNode {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub archived: bool,
    /// This object's own spend plus every descendant's.
    pub cost_cents: i64,
    pub children: Vec<ObjectNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub total_cents: i64,
    pub years: Vec<String>,
    pub over_time: Vec<Amount>,
    pub by_object: Vec<ObjectNode>,
    pub by_type: Vec<Amount>,
    pub by_category: Vec<Amount>,
}

pub fn purchase_spend(_objects: &[ObjectRow], _purchased: &HashSet<i64>) -> Vec<Spend> {
    unimplemented!()
}

pub fn summarize(_objects: &[ObjectRow], _spend: &[Spend], _year: Option<i32>) -> Stats {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(id: i64, parent_id: Option<i64>, name: &str, kind: &str) -> ObjectRow {
        ObjectRow {
            id, parent_id, name: name.into(), kind: kind.into(), archived: false,
            purchase_date: None, purchase_price_cents: None, created_at: "2024-01-01T10:00:00Z".into(),
        }
    }

    fn spend(object_id: i64, month: &str, category: &str, cost_cents: i64) -> Spend {
        Spend { object_id, month: month.into(), category: category.into(), cost_cents }
    }

    fn amounts(list: &[Amount]) -> Vec<(&str, i64)> {
        list.iter().map(|a| (a.bucket.as_str(), a.cost_cents)).collect()
    }

    /// House (1) > Garage (2) > Bulb (3), plus a car (4).
    fn tree() -> Vec<ObjectRow> {
        vec![obj(1, None, "House", "home"), obj(2, Some(1), "Garage", "other"),
             obj(3, Some(2), "Bulb", "appliance"), obj(4, None, "Car", "car")]
    }

    #[test]
    fn a_parent_carries_every_descendants_spend() {
        let rows = [spend(1, "2026-01", "repair", 1_000), spend(2, "2026-02", "maintenance", 200),
                    spend(3, "2026-03", "repair", 30), spend(4, "2026-01", "fuel", 500)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(s.total_cents, 1_730);
        assert_eq!(s.by_object.len(), 2, "two roots");
        let house = &s.by_object[0];
        assert_eq!((house.name.as_str(), house.cost_cents), ("House", 1_230), "largest first");
        assert_eq!((house.children[0].name.as_str(), house.children[0].cost_cents), ("Garage", 230));
        assert_eq!((house.children[0].children[0].name.as_str(), house.children[0].children[0].cost_cents), ("Bulb", 30));
        assert_eq!((s.by_object[1].name.as_str(), s.by_object[1].cost_cents), ("Car", 500));
    }

    #[test]
    fn a_subtree_that_spent_nothing_is_left_out() {
        let rows = [spend(4, "2026-01", "fuel", 500)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(s.by_object.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Car"]);
    }

    #[test]
    fn an_object_whose_parent_is_not_in_the_list_is_a_root() {
        let objects = vec![obj(2, Some(99), "Garage", "other")];
        let s = summarize(&objects, &[spend(2, "2026-01", "repair", 70)], None);
        assert_eq!(s.by_object[0].name, "Garage");
    }

    #[test]
    fn type_is_each_objects_own_not_its_roots() {
        let rows = [spend(1, "2026-01", "repair", 1_000), spend(3, "2026-01", "repair", 30)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(amounts(&s.by_type), [("home", 1_000), ("appliance", 30)]);
    }

    #[test]
    fn all_years_draws_one_bar_per_year_oldest_first_and_lists_years_newest_first() {
        let rows = [spend(4, "2024-05", "fuel", 10), spend(4, "2026-01", "fuel", 20), spend(4, "2026-09", "fuel", 5)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(amounts(&s.over_time), [("2024", 10), ("2026", 25)], "no bar for a year with no spend");
        assert_eq!(s.years, ["2026", "2024"]);
    }

    #[test]
    fn one_year_draws_all_twelve_months_and_filters_every_block() {
        let rows = [spend(4, "2025-12", "fuel", 999), spend(4, "2026-01", "fuel", 20), spend(1, "2026-03", "repair", 7)];
        let s = summarize(&tree(), &rows, Some(2026));
        assert_eq!(s.over_time.len(), 12);
        assert_eq!(s.over_time[0], Amount { bucket: "2026-01".into(), cost_cents: 20 });
        assert_eq!(s.over_time[1], Amount { bucket: "2026-02".into(), cost_cents: 0 }, "known zero, not missing");
        assert_eq!(s.over_time[11].bucket, "2026-12");
        assert_eq!(s.total_cents, 27);
        assert_eq!(amounts(&s.by_category), [("fuel", 20), ("repair", 7)]);
        assert_eq!(s.years, ["2026", "2025"], "the year list ignores the filter");
    }

    #[test]
    fn categories_are_sorted_largest_first_and_zero_rows_dropped() {
        let rows = [spend(4, "2026-01", "fuel", 5), spend(4, "2026-01", "repair", 50), spend(4, "2026-01", "other", 0)];
        let s = summarize(&tree(), &rows, None);
        assert_eq!(amounts(&s.by_category), [("repair", 50), ("fuel", 5)]);
    }

    #[test]
    fn archived_is_carried_through() {
        let mut objects = tree();
        objects[3].archived = true;
        let s = summarize(&objects, &[spend(4, "2026-01", "fuel", 5)], None);
        assert!(s.by_object[0].archived);
    }

    #[test]
    fn nothing_spent_is_an_empty_but_complete_answer() {
        let s = summarize(&tree(), &[], Some(2026));
        assert_eq!(s.total_cents, 0);
        assert!(s.years.is_empty() && s.by_object.is_empty() && s.by_type.is_empty() && s.by_category.is_empty());
        assert_eq!(s.over_time.len(), 12);
    }

    #[test]
    fn a_purchase_price_is_dated_by_its_purchase_date() {
        let mut house = obj(1, None, "House", "home");
        house.purchase_date = Some("2019-06-30".into());
        house.purchase_price_cents = Some(30_000_000);
        assert_eq!(purchase_spend(&[house], &HashSet::new()), [spend(1, "2019-06", PURCHASE_PRICE, 30_000_000)]);
    }

    #[test]
    fn a_purchase_price_without_a_date_falls_back_to_when_the_object_was_created() {
        let mut car = obj(4, None, "Car", "car");
        car.purchase_price_cents = Some(1_500_000);
        car.created_at = "2023-11-02T08:00:00Z".into();
        assert_eq!(purchase_spend(&[car], &HashSet::new()), [spend(4, "2023-11", PURCHASE_PRICE, 1_500_000)]);
    }

    #[test]
    fn a_purchase_price_is_skipped_when_a_costed_purchase_entry_already_records_it() {
        let mut car = obj(4, None, "Car", "car");
        car.purchase_price_cents = Some(1_500_000);
        assert!(purchase_spend(&[car], &HashSet::from([4])).is_empty());
    }

    #[test]
    fn no_price_or_a_zero_price_adds_nothing() {
        let mut free = obj(5, None, "Gift", "tool");
        free.purchase_price_cents = Some(0);
        assert!(purchase_spend(&[obj(4, None, "Car", "car"), free], &HashSet::new()).is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib domain::stats`
Expected: compiles; every test FAILS with `not implemented`.

- [ ] **Step 3: Implement**

Replace the two stub functions with:

```rust
/// Purchase prices as spend, one entry per object that has a positive price.
///
/// Dated by `purchase_date`, or by the day the object was created when no purchase date was
/// entered -- the price was paid at some point, and the moment it was recorded is the best date
/// there is. An object in `purchased` already has a costed `purchase` activity: that entry is
/// the purchase, and adding the price as well would count the same money twice.
pub fn purchase_spend(objects: &[ObjectRow], purchased: &HashSet<i64>) -> Vec<Spend> {
    objects
        .iter()
        .filter(|o| !purchased.contains(&o.id))
        .filter_map(|o| {
            let cents = o.purchase_price_cents.filter(|c| *c > 0)?;
            let date = o.purchase_date.as_deref().unwrap_or(&o.created_at);
            Some(Spend { object_id: o.id, month: date.get(..7)?.to_string(), category: PURCHASE_PRICE.into(), cost_cents: cents })
        })
        .collect()
}

/// The whole statistics response for `spend`, restricted to `year` when one is given.
///
/// `years` is computed before the filter, so the year picker always offers every year that has
/// spend, whichever one is selected.
pub fn summarize(objects: &[ObjectRow], spend: &[Spend], year: Option<i32>) -> Stats {
    let mut years: Vec<String> = spend
        .iter()
        .filter(|s| s.cost_cents > 0)
        .filter_map(|s| s.month.get(..4).map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    years.reverse();

    let prefix = year.map(|y| format!("{y:04}-"));
    let selected: Vec<&Spend> = spend
        .iter()
        .filter(|s| s.cost_cents > 0)
        .filter(|s| prefix.as_deref().is_none_or(|p| s.month.starts_with(p)))
        .collect();

    let over_time = match year {
        Some(y) => (1..=12)
            .map(|m| {
                let bucket = format!("{y:04}-{m:02}");
                let cost_cents = selected.iter().filter(|s| s.month == bucket).map(|s| s.cost_cents).sum();
                Amount { bucket, cost_cents }
            })
            .collect(),
        None => {
            let mut per_year: BTreeMap<String, i64> = BTreeMap::new();
            for s in &selected {
                if let Some(y) = s.month.get(..4) {
                    *per_year.entry(y.to_string()).or_default() += s.cost_cents;
                }
            }
            per_year.into_iter().map(|(bucket, cost_cents)| Amount { bucket, cost_cents }).collect()
        }
    };

    let mut own: HashMap<i64, i64> = HashMap::new();
    let mut by_category: HashMap<String, i64> = HashMap::new();
    for s in &selected {
        *own.entry(s.object_id).or_default() += s.cost_cents;
        *by_category.entry(s.category.clone()).or_default() += s.cost_cents;
    }

    let mut by_type: HashMap<String, i64> = HashMap::new();
    for o in objects {
        if let Some(c) = own.get(&o.id) {
            *by_type.entry(o.kind.clone()).or_default() += c;
        }
    }

    Stats {
        total_cents: selected.iter().map(|s| s.cost_cents).sum(),
        years,
        over_time,
        by_object: roll_up(objects, &own),
        by_type: sorted(by_type),
        by_category: sorted(by_category),
    }
}

/// Largest first, then by name so equal amounts do not shuffle between requests.
fn sorted(map: HashMap<String, i64>) -> Vec<Amount> {
    let mut list: Vec<Amount> = map
        .into_iter()
        .filter(|(_, c)| *c > 0)
        .map(|(bucket, cost_cents)| Amount { bucket, cost_cents })
        .collect();
    list.sort_by(|a, b| b.cost_cents.cmp(&a.cost_cents).then_with(|| a.bucket.cmp(&b.bucket)));
    list
}

/// The object tree with each node carrying its subtree's spend.
///
/// An object whose parent is not among `objects` is a root: the list holds only non-deleted
/// objects, and a child must not vanish from the totals because of what happened to its parent.
/// Subtrees that spent nothing are dropped. The hierarchy API refuses cycles, so every object is
/// reached from exactly one root.
fn roll_up(objects: &[ObjectRow], own: &HashMap<i64, i64>) -> Vec<ObjectNode> {
    let ids: HashSet<i64> = objects.iter().map(|o| o.id).collect();
    let mut children: HashMap<Option<i64>, Vec<&ObjectRow>> = HashMap::new();
    for o in objects {
        let parent = o.parent_id.filter(|p| ids.contains(p));
        children.entry(parent).or_default().push(o);
    }

    fn build(parent: Option<i64>, children: &HashMap<Option<i64>, Vec<&ObjectRow>>, own: &HashMap<i64, i64>) -> Vec<ObjectNode> {
        let mut nodes: Vec<ObjectNode> = children
            .get(&parent)
            .into_iter()
            .flatten()
            .map(|o| {
                let kids = build(Some(o.id), children, own);
                let cost_cents = own.get(&o.id).copied().unwrap_or(0) + kids.iter().map(|k| k.cost_cents).sum::<i64>();
                ObjectNode { id: o.id, name: o.name.clone(), kind: o.kind.clone(), archived: o.archived, cost_cents, children: kids }
            })
            .filter(|n| n.cost_cents > 0)
            .collect();
        nodes.sort_by(|a, b| b.cost_cents.cmp(&a.cost_cents).then_with(|| a.name.cmp(&b.name)));
        nodes
    }

    build(None, &children, own)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib domain::stats`
Expected: 13 passed.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src/domain/mod.rs src/domain/stats.rs
git commit -m "feat: statistics domain -- spend by time, object tree, type and category"
```

---

### Task 2: API — `GET /stats`

**Files:**
- Create: `src/api/stats.rs`, `tests/stats.rs`
- Modify: `src/api/mod.rs`, `docs/openapi.json`

**Interfaces:**
- Consumes (Task 1): `domain::stats::{ObjectRow, Spend, Stats, purchase_spend, summarize}`.
- Produces (Task 3+): `GET /api/stats?year=YYYY&purchases=true|false` → JSON:
  ```json
  { "total_cents": 0, "years": ["2026"], "over_time": [{"bucket": "2026-01", "cost_cents": 0}],
    "by_object": [{"id": 1, "name": "House", "type": "home", "archived": false, "cost_cents": 0, "children": []}],
    "by_type": [{"bucket": "home", "cost_cents": 0}], "by_category": [{"bucket": "repair", "cost_cents": 0}] }
  ```
  Non-numeric `year`: 400 (axum `Query` rejection). `year` outside 1900..=9999: 400 `bad_request`.

- [ ] **Step 1: Write the failing integration tests**

Create `tests/stats.rs`:

```rust
mod common;
use serde_json::{json, Value};

async fn object(app: &common::TestApp, body: Value) -> i64 {
    let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}

async fn cost(app: &common::TestApp, object_id: i64, date: &str, category: &str, cents: i64) -> i64 {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities")))
        .json(&json!({ "date": date, "category": category, "title": category, "notes": "", "cost_cents": cents }))
        .send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
}

fn buckets(v: &Value) -> Vec<(String, i64)> {
    v.as_array().unwrap().iter()
        .map(|b| (b["bucket"].as_str().unwrap().to_string(), b["cost_cents"].as_i64().unwrap()))
        .collect()
}

/// House (bought 2024 for 3,000.00) > Boiler; a car. Costs in 2025 and 2026.
async fn seed(app: &common::TestApp) -> (i64, i64, i64) {
    let house = object(app, json!({ "name": "House", "type": "home", "description": "",
        "purchase_date": "2024-05-01", "purchase_price_cents": 300_000 })).await;
    let boiler = object(app, json!({ "name": "Boiler", "type": "appliance", "description": "", "parent_id": house })).await;
    let car = object(app, json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "" })).await;
    cost(app, house, "2025-03-10", "repair", 100_000).await;
    cost(app, boiler, "2026-02-01", "maintenance", 25_000).await;
    cost(app, car, "2026-06-01", "fuel", 8_000).await;
    cost(app, car, "2026-07-01", "repair", 42_000).await;
    (house, boiler, car)
}

#[tokio::test]
async fn all_years_roll_children_into_their_parent() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;

    let out = app.get_json("/stats").await;
    assert_eq!(out["total_cents"], 175_000);
    assert_eq!(out["years"], json!(["2026", "2025"]));
    assert_eq!(buckets(&out["over_time"]), [("2025".into(), 100_000), ("2026".into(), 75_000)]);
    let house = &out["by_object"][0];
    assert_eq!(house["name"], "House");
    assert_eq!(house["type"], "home");
    assert_eq!(house["cost_cents"], 125_000);
    assert_eq!(house["children"][0]["name"], "Boiler");
    assert_eq!(out["by_object"][1]["cost_cents"], 50_000);
    assert_eq!(buckets(&out["by_type"]), [("home".into(), 100_000), ("car".into(), 50_000), ("appliance".into(), 25_000)]);
}

#[tokio::test]
async fn a_year_gives_twelve_months_and_keeps_the_full_year_list() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;

    let out = app.get_json("/stats?year=2026").await;
    assert_eq!(out["total_cents"], 75_000);
    assert_eq!(out["over_time"].as_array().unwrap().len(), 12);
    assert_eq!(out["over_time"][1], json!({ "bucket": "2026-02", "cost_cents": 25_000 }));
    assert_eq!(out["over_time"][0], json!({ "bucket": "2026-01", "cost_cents": 0 }));
    assert_eq!(out["years"], json!(["2026", "2025"]));
    assert_eq!(buckets(&out["by_category"]), [("repair".into(), 42_000), ("maintenance".into(), 25_000), ("fuel".into(), 8_000)]);
}

#[tokio::test]
async fn purchases_add_their_own_row_unless_a_purchase_entry_already_has_the_cost() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, _, car) = seed(&app).await;

    let out = app.get_json("/stats?purchases=true").await;
    assert_eq!(out["total_cents"], 475_000);
    assert_eq!(out["years"], json!(["2026", "2025", "2024"]));
    assert!(buckets(&out["by_category"]).contains(&("purchase_price".into(), 300_000)));

    // A car whose purchase is logged as an entry: its price field must not count again.
    let res = app.client.patch(app.url(&format!("/objects/{car}")))
        .json(&json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "", "purchase_price_cents": 900_000 }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    cost(&app, car, "2023-01-15", "purchase", 900_000).await;
    let out = app.get_json("/stats?purchases=true").await;
    assert_eq!(out["total_cents"], 475_000 + 900_000);
    assert!(buckets(&out["by_category"]).contains(&("purchase".into(), 900_000)));
    assert!(buckets(&out["by_category"]).contains(&("purchase_price".into(), 300_000)), "only the house's price");
}

#[tokio::test]
async fn deleted_rows_are_gone_archived_objects_stay() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let (_, _, car) = seed(&app).await;

    let fuel = app.get_json(&format!("/objects/{car}/activities")).await;
    let fuel_id = fuel.as_array().unwrap().iter().find(|a| a["category"] == "fuel").unwrap()["id"].as_i64().unwrap();
    assert!(app.client.delete(app.url(&format!("/activities/{fuel_id}"))).send().await.unwrap().status().is_success());
    let res = app.client.patch(app.url(&format!("/objects/{car}")))
        .json(&json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "", "archived": true }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());

    let out = app.get_json("/stats").await;
    assert_eq!(out["total_cents"], 167_000);
    let car_row = out["by_object"].as_array().unwrap().iter().find(|o| o["name"] == "Car").unwrap().clone();
    assert_eq!(car_row["archived"], true);
    assert_eq!(car_row["cost_cents"], 42_000);

    let house = out["by_object"][0]["id"].as_i64().unwrap();
    assert!(app.client.delete(app.url(&format!("/objects/{house}"))).send().await.unwrap().status().is_success());
    let out = app.get_json("/stats").await;
    assert_eq!(out["total_cents"], 42_000, "a deleted house takes its boiler with it");
}

#[tokio::test]
async fn another_users_spend_never_appears() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    seed(&app).await;
    let anna = app.create_user_client("anna", "password123").await;
    let out: Value = anna.get(app.url("/stats")).send().await.unwrap().json().await.unwrap();
    assert_eq!(out["total_cents"], 0);
    assert!(out["by_object"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_bad_year_is_400() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    for q in ["year=abc", "year=12", "year=10000"] {
        let res = app.client.get(app.url(&format!("/stats?{q}"))).send().await.unwrap();
        assert_eq!(res.status(), 400, "{q}");
    }
}
```

> Before running: check `get_json` in `tests/common/mod.rs:709` asserts a 200 and parses JSON; the list endpoint `GET /objects/{id}/activities` returns a JSON array. If either differs, adjust the two call sites, not the assertions.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test stats`
Expected: FAIL — every test gets 404 from `/stats` (`get_json` panics / status assertions fail).

- [ ] **Step 3: Implement the route**

Create `src/api/stats.rs`:

```rust
use crate::auth::AuthUser;
use crate::domain::stats::{purchase_spend, summarize, ObjectRow, Spend, Stats};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use std::collections::HashSet;

pub fn router() -> Router<App> {
    Router::new().route("/stats", get(read))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub purchases: Option<bool>,
}

/// Spend across every object the user owns. The database groups; `domain::stats` does the rest,
/// because a household has tens of objects and the tree roll-up is plainer in Rust than in SQL
/// that has to run on two databases.
async fn read(user: AuthUser, State(state): State<App>, Query(q): Query<StatsQuery>) -> Result<Json<Stats>, AppError> {
    if let Some(y) = q.year {
        if !(1900..=9999).contains(&y) {
            return Err(AppError::BadRequest("year must be between 1900 and 9999".into()));
        }
    }

    // Readings are excluded as in insights: they never carry a cost.
    let rows: Vec<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT a.object_id, substr(a.date, 1, 7), a.category, CAST(SUM(a.cost_cents) AS BIGINT) \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
           AND a.cost_cents IS NOT NULL AND a.category <> 'reading' \
         GROUP BY a.object_id, substr(a.date, 1, 7), a.category",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let mut spend: Vec<Spend> = rows
        .into_iter()
        .map(|(object_id, month, category, cost_cents)| Spend { object_id, month, category, cost_cents })
        .collect();

    #[allow(clippy::type_complexity)]
    let object_rows: Vec<(i64, Option<i64>, String, String, Option<String>, Option<String>, Option<i64>, String)> = sqlx::query_as(
        "SELECT id, parent_id, name, type, archived_at, purchase_date, purchase_price_cents, created_at \
         FROM objects WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let objects: Vec<ObjectRow> = object_rows
        .into_iter()
        .map(|(id, parent_id, name, kind, archived_at, purchase_date, purchase_price_cents, created_at)| ObjectRow {
            id, parent_id, name, kind, archived: archived_at.is_some(), purchase_date, purchase_price_cents, created_at,
        })
        .collect();

    if q.purchases.unwrap_or(false) {
        // A purchase entry with a cost is the purchase; see `purchase_spend`.
        let purchased: Vec<(i64,)> = sqlx::query_as(
            "SELECT DISTINCT a.object_id FROM activities a JOIN objects o ON o.id = a.object_id \
             WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
               AND a.category = 'purchase' AND a.cost_cents > 0",
        )
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
        let purchased: HashSet<i64> = purchased.into_iter().map(|(id,)| id).collect();
        spend.extend(purchase_spend(&objects, &purchased));
    }

    Ok(Json(summarize(&objects, &spend, q.year)))
}
```

In `src/api/mod.rs`: add `pub mod stats;` after `pub mod settings;`, and `.merge(stats::router())` after `.merge(search::router())`.

- [ ] **Step 4: Run tests**

Run: `cargo test --test stats`
Expected: 6 passed.

If `LOGB_TEST_DATABASE_URL` is available locally, also run: `LOGB_TEST_DATABASE_URL=$LOGB_TEST_DATABASE_URL cargo test --test stats` — expected 6 passed on PostgreSQL.

- [ ] **Step 5: Document in OpenAPI**

Run `cargo test --test openapi` first — expected FAIL naming `/stats` as undocumented.

In `docs/openapi.json`, add under `"paths"` (after `"/search"`, keeping the file's alphabetical-ish order as found):

```json
"/stats": {
  "get": {
    "summary": "Spend across all of the caller's objects: over time, by object (children rolled up), by type and by category.",
    "parameters": [
      { "name": "year", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 1900, "maximum": 9999 },
        "description": "Restrict to one year; over_time then has twelve YYYY-MM buckets. Omitted: all years, one YYYY bucket each." },
      { "name": "purchases", "in": "query", "required": false, "schema": { "type": "boolean", "default": false },
        "description": "Count objects' purchase prices, as by_category bucket purchase_price. Skipped for an object with a costed purchase entry." }
    ],
    "responses": {
      "200": { "description": "Success", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Stats" } } } },
      "400": { "description": "year is not a number or out of range" }
    }
  }
},
```

And under `"components"."schemas"` (next to `"Insights"`):

```json
"StatsAmount": {
  "type": "object",
  "required": ["bucket", "cost_cents"],
  "properties": { "bucket": { "type": "string" }, "cost_cents": { "type": "integer", "format": "int64" } }
},
"StatsObject": {
  "type": "object",
  "required": ["id", "name", "type", "archived", "cost_cents", "children"],
  "properties": {
    "id": { "type": "integer", "format": "int64" },
    "name": { "type": "string" },
    "type": { "type": "string" },
    "archived": { "type": "boolean" },
    "cost_cents": { "type": "integer", "format": "int64", "description": "Own spend plus every descendant's." },
    "children": { "type": "array", "items": { "$ref": "#/components/schemas/StatsObject" } }
  }
},
"Stats": {
  "type": "object",
  "required": ["total_cents", "years", "over_time", "by_object", "by_type", "by_category"],
  "properties": {
    "total_cents": { "type": "integer", "format": "int64" },
    "years": { "type": "array", "items": { "type": "string" }, "description": "Every year with spend, newest first; ignores the year filter." },
    "over_time": { "type": "array", "items": { "$ref": "#/components/schemas/StatsAmount" } },
    "by_object": { "type": "array", "items": { "$ref": "#/components/schemas/StatsObject" } },
    "by_type": { "type": "array", "items": { "$ref": "#/components/schemas/StatsAmount" } },
    "by_category": { "type": "array", "items": { "$ref": "#/components/schemas/StatsAmount" } }
  }
},
```

Run: `python3 -m json.tool docs/openapi.json > /dev/null && cargo test --test openapi`
Expected: valid JSON; PASS.

- [ ] **Step 6: Full backend check and commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass, no warnings.

```bash
git add src/api/stats.rs src/api/mod.rs tests/stats.rs docs/openapi.json
git commit -m "feat: GET /stats -- spend across all objects, by year or month"
```

---

### Task 3: Frontend foundations — types, helpers, nav, icon, strings

**Files:**
- Create: `frontend/src/lib/stats.ts`, `frontend/tests/stats.test.ts`
- Modify: `frontend/src/lib/types.ts`, `frontend/src/lib/nav.ts`, `frontend/tests/nav.test.ts`, `frontend/src/lib/Icon.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`

**Interfaces:**
- Consumes (Task 2): the `/stats` JSON shape.
- Produces (Tasks 4–5):
  ```ts
  // types.ts
  export interface Amount { bucket: string; cost_cents: number }
  export interface StatsObject { id: number; name: string; type: ObjectType; archived: boolean; cost_cents: number; children: StatsObject[] }
  export interface Stats { total_cents: number; years: string[]; over_time: Amount[]; by_object: StatsObject[]; by_type: Amount[]; by_category: Amount[] }
  // stats.ts
  export const PURCHASE_PRICE = 'purchase_price';
  export function statsPath(year: string | null, purchases: boolean): string
  export function periodLabel(bucket: string, locale: string): string
  export function sharePct(part: number, total: number): number
  export interface TreeRow { node: StatsObject; depth: number; hasChildren: boolean; expanded: boolean }
  export function flattenTree(nodes: StatsObject[], expanded: ReadonlySet<number>, depth?: number): TreeRow[]
  // Icon.svelte: IconName gains 'chart'
  // nav.ts: Destination gains 'stats'; DESTINATIONS order objects, search, stats, settings
  // i18n keys: nav.stats, stats.title, stats.year, stats.all-years, stats.purchases, stats.total,
  //   stats.over-time, stats.by-object, stats.by-type, stats.by-category, stats.purchase-price,
  //   stats.archived, stats.none, stats.expand, stats.collapse
  ```

- [ ] **Step 1: Write failing tests**

Create `frontend/tests/stats.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { flattenTree, periodLabel, sharePct, statsPath } from '../src/lib/stats';
import type { StatsObject } from '../src/lib/types';

const node = (id: number, cost_cents: number, children: StatsObject[] = []): StatsObject =>
  ({ id, name: `o${id}`, type: 'other', archived: false, cost_cents, children });

describe('statsPath', () => {
  it('sends nothing it does not need', () => {
    expect(statsPath(null, false)).toBe('/stats');
  });
  it('sends the year and the toggle', () => {
    expect(statsPath('2026', true)).toBe('/stats?year=2026&purchases=true');
    expect(statsPath(null, true)).toBe('/stats?purchases=true');
  });
});

describe('periodLabel', () => {
  it('shows a year as itself', () => {
    expect(periodLabel('2026', 'en')).toBe('2026');
  });
  it('shows a month as its short name, in the reader\'s language', () => {
    expect(periodLabel('2026-03', 'en')).toBe('Mar');
    expect(periodLabel('2026-03', 'de')).toBe('März');
  });
});

describe('sharePct', () => {
  it('rounds to a whole percent', () => {
    expect(sharePct(1, 3)).toBe(33);
  });
  it('is 0 of nothing rather than NaN', () => {
    expect(sharePct(0, 0)).toBe(0);
  });
});

describe('flattenTree', () => {
  const tree = [node(1, 300, [node(2, 200, [node(3, 50)])]), node(4, 100)];

  it('shows only roots until something is expanded', () => {
    const rows = flattenTree(tree, new Set());
    expect(rows.map((r) => [r.node.id, r.depth, r.hasChildren, r.expanded])).toEqual([[1, 0, true, false], [4, 0, false, false]]);
  });

  it('shows the children of an expanded node, one level at a time', () => {
    expect(flattenTree(tree, new Set([1])).map((r) => [r.node.id, r.depth])).toEqual([[1, 0], [2, 1], [4, 0]]);
    expect(flattenTree(tree, new Set([1, 2])).map((r) => [r.node.id, r.depth])).toEqual([[1, 0], [2, 1], [3, 2], [4, 0]]);
  });

  it('hides a grandchild when its grandparent is collapsed, even if its parent is marked expanded', () => {
    expect(flattenTree(tree, new Set([2])).map((r) => r.node.id)).toEqual([1, 4]);
  });
});
```

In `frontend/tests/nav.test.ts`, add after the Search test:

```ts
  it('marks Statistics', () => {
    expect(activeDestination('/stats')).toBe('stats');
  });
```

add `expect(activeDestination('/statsx')).toBeNull();` inside the "merely starts with" test, and replace the last test with:

```ts
  it('lists exactly the four top-level destinations, in order', () => {
    expect(DESTINATIONS.map((d) => d.id)).toEqual(['objects', 'search', 'stats', 'settings']);
    expect(DESTINATIONS.map((d) => d.path)).toEqual(['/', '/search', '/stats', '/settings']);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd frontend && npx vitest run tests/stats.test.ts tests/nav.test.ts`
Expected: FAIL — `stats.ts` cannot be resolved; nav expects four destinations.

- [ ] **Step 3: Implement**

Append to `frontend/src/lib/types.ts` (after the `Insights` interface):

```ts
/** `GET /stats`. `bucket` is `YYYY`, `YYYY-MM`, an object type, a category, or `purchase_price`. */
export interface Amount { bucket: string; cost_cents: number }
/** `cost_cents` includes every descendant's spend. */
export interface StatsObject { id: number; name: string; type: ObjectType; archived: boolean; cost_cents: number; children: StatsObject[] }
export interface Stats {
  total_cents: number;
  /** Every year with spend, newest first, whichever year is selected. */
  years: string[];
  over_time: Amount[]; by_object: StatsObject[]; by_type: Amount[]; by_category: Amount[];
}
```

Create `frontend/src/lib/stats.ts`:

```ts
import type { StatsObject } from './types';

/** The `by_category` bucket for objects' purchase prices -- not an activity category. */
export const PURCHASE_PRICE = 'purchase_price';

/** The request for a selection. Defaults are left out, so the common case is a plain `/stats`. */
export function statsPath(year: string | null, purchases: boolean): string {
  const q = new URLSearchParams();
  if (year) q.set('year', year);
  if (purchases) q.set('purchases', 'true');
  const s = q.toString();
  return s ? `/stats?${s}` : '/stats';
}

/** `2026` stays `2026`; `2026-03` becomes the reader's short month name. The year is already
 *  on screen in the picker, so repeating it on twelve bars is noise. */
export function periodLabel(bucket: string, locale: string): string {
  if (bucket.length === 4) return bucket;
  const [y, m] = bucket.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { month: 'short' }).format(new Date(Date.UTC(y, m - 1, 15)));
}

export function sharePct(part: number, total: number): number {
  return total > 0 ? Math.round((part / total) * 100) : 0;
}

export interface TreeRow { node: StatsObject; depth: number; hasChildren: boolean; expanded: boolean }

/** The rows on screen: roots, plus the children of every expanded node whose ancestors are all
 *  expanded too. Collapsing a parent hides its whole subtree without forgetting what was open. */
export function flattenTree(nodes: StatsObject[], expanded: ReadonlySet<number>, depth = 0): TreeRow[] {
  return nodes.flatMap((node) => {
    const open = expanded.has(node.id);
    const row: TreeRow = { node, depth, hasChildren: node.children.length > 0, expanded: open };
    return open ? [row, ...flattenTree(node.children, expanded, depth + 1)] : [row];
  });
}
```

Edit `frontend/src/lib/nav.ts`:

```ts
export type Destination = 'objects' | 'search' | 'stats' | 'settings';
```

Replace the `DESTINATIONS` doc comment's first sentence and array:

```ts
/** The whole of LogB's top-level navigation. Four is few enough that all of them are always
 *  visible: no drawer, no overflow menu, no hamburger hiding two items behind a tap. */
export const DESTINATIONS: NavDestination[] = [
  { id: 'objects', path: '/', icon: 'object', label: 'nav.objects' },
  { id: 'search', path: '/search', icon: 'search', label: 'search.title' },
  { id: 'stats', path: '/stats', icon: 'chart', label: 'nav.stats' },
  { id: 'settings', path: '/settings', icon: 'settings', label: 'nav.settings' },
];
```

and in `activeDestination`, after the search line:

```ts
  if (within(p, '/stats')) return 'stats';
```

Edit `frontend/src/lib/Icon.svelte`: the type's last line becomes

```ts
    | 'palette' | 'person' | 'key' | 'box' | 'people' | 'database' | 'chevron' | 'logout' | 'bell' | 'chart';
```

and before `{:else}` add:

```svelte
  {:else if name === 'chart'}
    <path d="M3 3v18h18" />
    <path d="M8 17v-5" />
    <path d="M13 17V8" />
    <path d="M18 17v-9" />
```

In `frontend/src/i18n/en.ts`, add after `'nav.objects': 'Objects',`:

```ts
  'nav.stats': 'Statistics',
```

and before the closing `} as Record<string, string>;`:

```ts

  'stats.title': 'Statistics',
  'stats.year': 'Year',
  'stats.all-years': 'All years',
  'stats.purchases': 'Include purchase prices',
  'stats.total': 'Total',
  'stats.over-time': 'Spend over time',
  'stats.by-object': 'By object',
  'stats.by-type': 'By type',
  'stats.by-category': 'By category',
  'stats.purchase-price': 'Purchase price',
  'stats.archived': 'archived',
  'stats.none': 'No costs recorded for this period.',
  'stats.expand': 'Show what is inside {name}',
  'stats.collapse': 'Hide what is inside {name}',
```

In `frontend/src/i18n/de.ts`, the same positions:

```ts
  'nav.stats': 'Statistik',
```

```ts

  'stats.title': 'Statistik',
  'stats.year': 'Jahr',
  'stats.all-years': 'Alle Jahre',
  'stats.purchases': 'Kaufpreise einrechnen',
  'stats.total': 'Gesamt',
  'stats.over-time': 'Ausgaben im Zeitverlauf',
  'stats.by-object': 'Nach Objekt',
  'stats.by-type': 'Nach Typ',
  'stats.by-category': 'Nach Kategorie',
  'stats.purchase-price': 'Kaufpreis',
  'stats.archived': 'archiviert',
  'stats.none': 'Für diesen Zeitraum sind keine Kosten erfasst.',
  'stats.expand': 'Inhalt von {name} zeigen',
  'stats.collapse': 'Inhalt von {name} ausblenden',
```

- [ ] **Step 4: Run tests**

Run: `cd frontend && npx vitest run && npm run check`
Expected: all Vitest suites pass (including `i18n`, `icons`, `nav`, `stats`); `svelte-check` 0 errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/stats.ts frontend/tests/stats.test.ts frontend/src/lib/types.ts frontend/src/lib/nav.ts frontend/tests/nav.test.ts frontend/src/lib/Icon.svelte frontend/src/i18n/en.ts frontend/src/i18n/de.ts
git commit -m "feat: statistics types, helpers, strings, chart icon and nav entry"
```

(The nav button now leads to a 404 page until Task 5; nothing is released between tasks.)

---

### Task 4: Shared `BarList`, adopted by Insights

**Files:**
- Create: `frontend/src/lib/BarList.svelte`
- Modify: `frontend/src/lib/Insights.svelte`

**Interfaces:**
- Consumes: nothing new.
- Produces (Task 5):
  ```ts
  export interface Bar {
    key: string | number; label: string; value: number; display: string;
    /** Tree depth; 0 or absent is a root. */ depth?: number;
    /** Muted text after the label, e.g. "archived". */ note?: string;
    /** Makes the label a button (e.g. open the object). */ onLabel?: () => void;
    /** Present when the row can expand; `true` when open. */ expanded?: boolean;
    onToggle?: () => void; toggleLabel?: string;
  }
  // <BarList items={Bar[]} />
  ```
  Bars scale to the largest `value` among `items`. `Bar` is exported from the component's `<script module>`.

- [ ] **Step 1: Create `BarList.svelte`**

```svelte
<script lang="ts" module>
  export interface Bar {
    key: string | number;
    label: string;
    value: number;
    display: string;
    /** Tree depth; 0 or absent is a root. */
    depth?: number;
    /** Muted text after the label, such as "archived". */
    note?: string;
    /** Makes the label a button. */
    onLabel?: () => void;
    /** Present only on a row that can expand; `true` while open. */
    expanded?: boolean;
    onToggle?: () => void;
    toggleLabel?: string;
  }
</script>

<script lang="ts">
  import Icon from './Icon.svelte';

  let { items }: { items: Bar[] } = $props();

  /** Bar width as a percentage of the largest value, so the widest bar always fills its track.
   *  Values are never negative: the API rejects negative costs at the boundary. */
  const max = $derived(Math.max(...items.map((b) => b.value), 1));
</script>

{#each items as b (b.key)}
  <div class="bar-row" style={b.depth ? `padding-left: calc(${b.depth} * var(--space-4))` : undefined}>
    <span class="label">
      {#if b.expanded !== undefined}
        <button class="toggle" class:open={b.expanded} aria-expanded={b.expanded} aria-label={b.toggleLabel} onclick={b.onToggle}>
          <Icon name="chevron" size={14} />
        </button>
      {/if}
      {#if b.onLabel}
        <button class="link" onclick={b.onLabel}>{b.label}</button>
      {:else}
        {b.label}
      {/if}
      {#if b.note}<span class="muted note">{b.note}</span>{/if}
    </span>
    <span class="track"><span class="fill" style={`width:${Math.round((b.value / max) * 100)}%`}></span></span>
    <span class="value tnum">{b.display}</span>
  </div>
{/each}

<style>
  .bar-row { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2); }
  .label { flex: none; width: 90px; font-size: var(--text-sm); display: flex; align-items: center; gap: var(--space-1); min-width: 0; }
  /* Half the track's height, spelled as a literal, is a pill -- and a pill is a token. */
  .track { flex: 1; height: 10px; background: var(--surface-2); border-radius: var(--radius-full); overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
  .value { flex: none; font-size: var(--text-sm); }
  .note { font-size: var(--text-xs, var(--text-sm)); }
  .toggle, .link { background: none; border: 0; padding: 0; color: inherit; font: inherit; cursor: pointer; }
  .link { text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .toggle { display: inline-flex; transition: transform 120ms; }
  .toggle.open { transform: rotate(90deg); }
</style>
```

> Check `frontend/src/app.css` for the spacing/text tokens used (`--space-1`, `--text-xs`). If `--space-1` does not exist, use the smallest `--space-*` that does. The fallback on `--text-xs` already covers its absence.

- [ ] **Step 2: Switch `Insights.svelte` to it**

In the `<script>`: add `import BarList from './BarList.svelte';`, delete the `pct` function and its comment.

Replace the usage-by-month `{#each}` block (inside `{#if data && data.usage_by_month.length > 0 && unit}`) with:

```svelte
  <h3>{$t('insights.usage-by-month')}</h3>
  <!-- A month the readings cannot measure says so, rather than drawing a zero it does not know. -->
  <BarList items={data.usage_by_month.map((m) => ({
    key: m.month, label: monthLabel(m.month), value: m.amount ?? 0,
    display: m.amount === null ? '—' : counter(m.amount, unit, $locale),
  }))} />
```

(and delete the now-unused `{@const measured = ...}` line).

Replace the by-year `{#each}` with:

```svelte
    <BarList items={data.by_year.map((b) => ({ key: b.bucket, label: b.bucket, value: b.cost_cents, display: money(b.cost_cents, $currency, $locale) }))} />
```

Replace the by-category `{#each}` with:

```svelte
    <BarList items={data.by_category.map((b) => ({ key: b.bucket, label: $t(`cat.${b.bucket}`), value: b.cost_cents, display: money(b.cost_cents, $currency, $locale) }))} />
```

In `<style>`, delete `.bar-row`, `.label`, `.track`, `.fill`, `.value` (keep `h3`).

- [ ] **Step 3: Verify no regression**

Run: `cd frontend && npm run check && npx vitest run`
Expected: 0 errors; all pass.

Run: `cd frontend && npm run e2e -- 02-lifecycle 12-object-hierarchy`
Expected: PASS (these open the Info tab where Insights renders). If a spec asserts on Insights text, it still finds it.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/BarList.svelte frontend/src/lib/Insights.svelte
git commit -m "refactor: bar rows move to a shared BarList, Insights uses it"
```

---

### Task 5: The Statistics screen

**Files:**
- Create: `frontend/src/routes/Stats.svelte`, `frontend/tests-e2e/20-statistics.spec.ts`
- Modify: `frontend/src/App.svelte`, `docs/superpowers/specs/2026-09-14-statistics-design.md`

**Interfaces:**
- Consumes: `Stats` types, `stats.ts` helpers, i18n keys (Task 3); `BarList`/`Bar` (Task 4); `GET /stats` (Task 2); `persisted` from `stores/persisted.ts`; `TopBar`, `go`, `money`, `currency`, `locale`, `t`.
- Produces: route `/stats`.

- [ ] **Step 1: Write the failing end-to-end spec**

Create `frontend/tests-e2e/20-statistics.spec.ts`:

```ts
import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function cost(page: Page, id: number, date: string, category: string, cost_cents: number) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { date, category, title: category, notes: '', cost_cents } });
  expect(res.ok()).toBe(true);
}

test('statistics total everything, roll a boiler into its house, and remember the purchase toggle', async ({ page }) => {
  await signInFresh(page, '20-statistics');
  const house = await object(page, { name: 'Stats House', type: 'home', purchase_date: '2024-05-01', purchase_price_cents: 300_000 });
  const boiler = await object(page, { name: 'Stats Boiler', type: 'appliance', parent_id: house });
  const car = await object(page, { name: 'Stats Car', type: 'car', counter_unit: 'km' });
  await cost(page, house, '2025-03-10', 'repair', 100_000);
  await cost(page, boiler, '2026-02-01', 'maintenance', 25_000);
  await cost(page, car, '2026-06-01', 'fuel', 8_000);
  await cost(page, car, '2026-07-01', 'repair', 42_000);

  await page.goto('/');
  await page.getByRole('button', { name: 'Statistics' }).click();
  await page.waitForURL('**/stats');

  const total = page.getByTestId('stats-total');
  await expect(total).toContainText('1,750.00');

  // The boiler is inside the house: hidden until the house is expanded, and counted in it.
  const byObject = page.getByTestId('stats-by-object');
  await expect(byObject).toContainText('Stats House');
  await expect(byObject).toContainText('1,250.00');
  await expect(byObject.getByText('Stats Boiler')).toHaveCount(0);
  await byObject.getByRole('button', { name: 'Show what is inside Stats House' }).click();
  await expect(byObject.getByText('Stats Boiler')).toBeVisible();

  // One year: twelve months, and only that year's money.
  await page.getByLabel('Year').selectOption('2026');
  // "750.00" is also inside the all-years "1,750.00", so wait for that to go first.
  await expect(total).not.toContainText('1,750.00');
  await expect(total).toContainText('750.00');
  await expect(page.getByTestId('stats-over-time').locator('.bar-row')).toHaveCount(12);

  // Purchase prices: off by default, on survives a reload.
  await page.getByLabel('Year').selectOption({ label: 'All years' });
  await page.getByLabel('Include purchase prices').check();
  await expect(total).toContainText('4,750.00');
  await expect(page.getByTestId('stats-by-category')).toContainText('Purchase price');
  await page.reload();
  await expect(page.getByLabel('Include purchase prices')).toBeChecked();
  await expect(page.getByTestId('stats-total')).toContainText('4,750.00');

  // Tapping an object opens it.
  await page.getByTestId('stats-by-object').getByRole('button', { name: 'Stats Car' }).click();
  await page.waitForURL(`**/objects/${car}`);
});

test('a new user sees the empty state', async ({ page }) => {
  await signInFresh(page, '20-statistics-empty');
  await page.goto('/stats');
  await expect(page.getByText('No costs recorded for this period.')).toBeVisible();
});
```

> The money assertions assume the e2e instance's currency renders with `,` thousands and `.` decimals in the `en` locale (Playwright's default). Check an existing spec that asserts on money (`grep -rn "00'" frontend/tests-e2e`) and match its format if it differs.

- [ ] **Step 2: Run to verify failure**

Run: `cd frontend && npm run e2e -- 20-statistics`
Expected: FAIL — `/stats` renders the 404 page; `stats-total` not found.

- [ ] **Step 3: Implement the screen**

Create `frontend/src/routes/Stats.svelte`:

```svelte
<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import BarList, { type Bar } from '../lib/BarList.svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { money } from '../lib/format';
  import { currency } from '../stores/session';
  import { persisted } from '../stores/persisted';
  import { locale, t } from '../i18n';
  import { PURCHASE_PRICE, flattenTree, periodLabel, sharePct, statsPath } from '../lib/stats';
  import type { Amount, Stats } from '../lib/types';

  /** Per device and not synced: whether to count purchase prices is a way of looking, not data. */
  const includePurchases = persisted('logb.stats.purchases', false);

  let year = $state<string | null>(null);
  let data = $state<Stats | null>(null);
  /** The last year list seen. Kept apart from `data`, which is cleared on every fetch, so the
   *  picker does not empty and reset itself while the next selection loads. */
  let years = $state<string[]>([]);
  let error = $state('');
  let expanded = $state<Set<number>>(new Set());

  $effect(() => {
    const path = statsPath(year, $includePurchases);
    // Cleared first: a stale total under a new selection would be a wrong number on screen.
    data = null;
    error = '';
    let current = true;
    api<Stats>('GET', path)
      .then((d) => { if (current) { data = d; years = d.years; } })
      .catch((e) => { if (current) error = (e as Error).message; });
    return () => { current = false; };
  });

  /** A selected year that has no spend any more (the toggle was turned off, say) stays offered,
   *  so the picker never shows a blank. */
  const yearOptions = $derived(year && !years.includes(year) ? [year, ...years] : years);

  function toggle(id: number) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id); else next.add(id);
    expanded = next;
  }

  const fmt = (cents: number) => money(cents, $currency, $locale);
  const share = (cents: number) => `${fmt(cents)} · ${sharePct(cents, data?.total_cents ?? 0)}%`;
  const bars = (list: Amount[], label: (bucket: string) => string): Bar[] =>
    list.map((a) => ({ key: a.bucket, label: label(a.bucket), value: a.cost_cents, display: share(a.cost_cents) }));

  const objectBars = $derived<Bar[]>(
    data
      ? flattenTree(data.by_object, expanded).map((r) => ({
          key: r.node.id,
          label: r.node.name,
          value: r.node.cost_cents,
          display: share(r.node.cost_cents),
          depth: r.depth,
          note: r.node.archived ? $t('stats.archived') : undefined,
          onLabel: () => go(`/objects/${r.node.id}`),
          expanded: r.hasChildren ? r.expanded : undefined,
          onToggle: () => toggle(r.node.id),
          toggleLabel: $t(r.expanded ? 'stats.collapse' : 'stats.expand', { name: r.node.name }),
        }))
      : [],
  );
</script>

<main>
  <TopBar title={$t('stats.title')} icon="chart" />

  <div class="controls">
    <label>
      {$t('stats.year')}
      <select value={year ?? ''} onchange={(e) => (year = (e.currentTarget as HTMLSelectElement).value || null)}>
        <option value="">{$t('stats.all-years')}</option>
        {#each yearOptions as y (y)}<option value={y}>{y}</option>{/each}
      </select>
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={$includePurchases} />
      {$t('stats.purchases')}
    </label>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {:else if !data}
    <p class="muted">{$t('nav.loading')}</p>
  {:else if data.total_cents === 0}
    <div class="empty"><p>{$t('stats.none')}</p></div>
  {:else}
    <p class="total" data-testid="stats-total">{$t('stats.total')}: <b class="tnum">{fmt(data.total_cents)}</b></p>

    <section data-testid="stats-over-time">
      <h2>{$t('stats.over-time')}</h2>
      <!-- Months show the amount alone: a share of the year on every one of twelve bars is noise. -->
      <BarList items={data.over_time.map((a) => ({ key: a.bucket, label: periodLabel(a.bucket, $locale), value: a.cost_cents, display: fmt(a.cost_cents) }))} />
    </section>

    <section data-testid="stats-by-object">
      <h2>{$t('stats.by-object')}</h2>
      <BarList items={objectBars} />
    </section>

    <section data-testid="stats-by-type">
      <h2>{$t('stats.by-type')}</h2>
      <BarList items={bars(data.by_type, (b) => $t(`type.${b}`))} />
    </section>

    <section data-testid="stats-by-category">
      <h2>{$t('stats.by-category')}</h2>
      <BarList items={bars(data.by_category, (b) => (b === PURCHASE_PRICE ? $t('stats.purchase-price') : $t(`cat.${b}`)))} />
    </section>
  {/if}
</main>

<style>
  .controls { display: flex; flex-wrap: wrap; gap: var(--space-3); align-items: end; margin-bottom: var(--space-4); }
  .controls label { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); }
  .controls .check { flex-direction: row; align-items: center; }
  .total { font-size: var(--text-lg, var(--text-base)); }
  h2 { margin: var(--space-4) 0 var(--space-2); font-size: var(--text-base); }
  section :global(.label) { width: 140px; }
</style>
```

> Before writing: open `frontend/src/routes/Search.svelte`'s `<style>` and one settings screen to confirm how `.empty`, `.error`, `.muted`, form `label`/`select` are styled globally; reuse those classes and drop any local rule that duplicates a global one. Confirm `TopBar` accepts `icon` without `backTo` (it does: both optional).

In `frontend/src/App.svelte`, add the import after `import Search from './routes/Search.svelte';`:

```ts
  import Stats from './routes/Stats.svelte';
```

and the route after `['/search', Search],`:

```ts
    ['/stats', Stats],
```

- [ ] **Step 4: Run everything**

Run: `cd frontend && npm run check && npx vitest run`
Expected: 0 errors; all pass.

Run: `cd frontend && npm run e2e -- 20-statistics`
Expected: 2 passed.

Run: `cd frontend && npm run e2e`
Expected: whole suite passes (the fourth nav button must not break specs that click nav buttons by name).

Manual check at phone width (Playwright or devtools, 390px): four nav buttons fit in the bottom bar without overflow; bar labels truncate rather than wrap the row.

- [ ] **Step 5: Mark the spec implemented and commit**

In `docs/superpowers/specs/2026-09-14-statistics-design.md`, change the status line to:

```
Status: phase 1 implemented. Phase 2 not started.
```

```bash
git add frontend/src/routes/Stats.svelte frontend/src/App.svelte frontend/tests-e2e/20-statistics.spec.ts docs/superpowers/specs/2026-09-14-statistics-design.md
git commit -m "feat: Statistics screen -- spend over time, by object, type and category"
```

---

## Self-Review Notes

- Spec coverage: navigation (T3, T5), year selector + persisted toggle (T5), four blocks (T5), archived label (T1, T5), zero rows omitted / months zero-filled (T1), spend rules incl. no double count and `created_at` fallback (T1, T2), API shape + 400s (T2), OpenAPI (T2), `BarList` extraction (T4), i18n (T3), errors + stale clear (T5), all test layers (T1–T5).
- Spec says "with a cost" for the double-count rule; plan pins it to `cost_cents > 0`.
- Phase 2 is intentionally absent.
