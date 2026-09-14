# Object Cost Depth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An object's Info tab shows total cost of ownership, spend per month, consumption per fill, and an "Include contents" switch that adds every descendant's costs.

**Architecture:** `GET /objects/{id}/insights` gains `?contents=true`. Every cost query is prefixed with a `scope` CTE that is either the object alone or the object plus its descendants (recursive, as `objects::ancestors` already does). New arithmetic lives as pure functions in `src/domain/stats.rs` (ownership, month window) and `src/domain/insights.rs` (consumption per fill). `Insights.svelte` renders the new blocks through `BarList` and a per-device switch.

**Tech Stack:** Rust (axum, sqlx `Any` on SQLite + PostgreSQL, chrono, serde), Svelte 5 runes, TypeScript, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-14-object-cost-depth-design.md`

## Global Constraints

- Work on branch `build-object-cost-depth` created from `main`; never commit to `main`.
- Money is integer cents; no floats on the server.
- Every SQL `SUM` is `CAST(SUM(...) AS BIGINT)`; placeholders are `$1`-style; only portable SQL (`substr`, `WITH RECURSIVE ... UNION`), so queries run unchanged on SQLite and PostgreSQL.
- Existing response fields keep their meaning when `contents` is absent.
- Counter figures (`counter_span`, `cost_per_counter_milli`, `fuel`, `counter_per_day_milli`, `usage_by_month`) are always the object's own, whatever `contents` says.
- Purchase prices appear only in `ownership`, never in `by_year`, `by_category` or `by_month`.
- Per-year figure omitted under 90 days owned.
- Every new UI string exists in `frontend/src/i18n/en.ts` and `de.ts`.
- Every route change is reflected in `docs/openapi.json` (`tests/openapi.rs`).
- Tests must not depend on the ICU/CLDR version for non-English month abbreviations (use regex for `de`).
- Commands run in the foreground; never end a turn with a background command pending.
- Comments explain *why*, as in `src/domain/insights.rs`.
- Commits end with:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DNUZLftSTtND7ABvN6eoGv
  ```

## File Map

| File | Status | Responsibility |
|---|---|---|
| `src/domain/stats.rs` | modify | `Ownership`, `ownership`, `day_of`, `months_ending`, shared month fill |
| `src/domain/insights.rs` | modify | `DatedFill`, `FillRate`, `consumption_per_fill` |
| `src/api/insights.rs` | modify | `contents` query, `scope` CTE, new response fields |
| `tests/insights.rs` | modify | Integration tests for the new fields |
| `docs/openapi.json` | modify | Parameter and new fields |
| `frontend/src/lib/insights.ts` | create | `insightsPath`, `sinceLabel`, `monthLabel`, `fillLabel` |
| `frontend/tests/insights.test.ts` | create | Vitest for helpers |
| `frontend/src/lib/types.ts` | modify | `Insights` new fields |
| `frontend/src/i18n/en.ts`, `de.ts` | modify | `insights.*` strings |
| `frontend/src/lib/Insights.svelte` | modify | Switch and new blocks |
| `frontend/src/routes/ObjectDetail.svelte` | modify | Pass `hasContents` |
| `frontend/tests-e2e/21-object-cost-depth.spec.ts` | create | End-to-end |

---

### Task 1: Domain — ownership, month window, consumption per fill

**Files:**
- Modify: `src/domain/stats.rs`, `src/domain/insights.rs`

**Interfaces:**
- Produces (Task 2 uses):
  ```rust
  // domain::stats
  pub const MIN_DAYS_FOR_PER_YEAR: i64 = 90;
  #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
  pub struct Ownership { pub total_cents: i64, pub purchase_cents: i64, pub since: String, pub per_year_cents: Option<i64> }
  pub fn day_of(s: &str) -> Option<NaiveDate>
  pub fn ownership(running_cents: i64, purchase_cents: i64, since: NaiveDate, until: NaiveDate) -> Ownership
  pub fn months_ending(today: NaiveDate, months: u32, totals: &[(String, i64)]) -> Vec<Amount>
  // domain::insights
  #[derive(Clone, Debug)] pub struct DatedFill { pub date: String, pub counter: i64, pub quantity_milli: i64 }
  #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)] pub struct FillRate { pub date: String, pub per_100_milli: i64 }
  pub const FILL_BARS: usize = 12;
  pub fn consumption_per_fill(fills: &[DatedFill]) -> Vec<FillRate>
  ```

- [ ] **Step 1: Create the branch**

```bash
git switch -c build-object-cost-depth
```

- [ ] **Step 2: Write failing tests**

In `src/domain/stats.rs`, add to the `use` lines at the top:

```rust
use chrono::{Datelike, Months, NaiveDate};
```

Add these stubs above `#[cfg(test)]`:

```rust
pub const MIN_DAYS_FOR_PER_YEAR: i64 = 90;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ownership {
    pub total_cents: i64,
    pub purchase_cents: i64,
    pub since: String,
    pub per_year_cents: Option<i64>,
}

pub fn day_of(_s: &str) -> Option<NaiveDate> { unimplemented!() }
pub fn ownership(_running_cents: i64, _purchase_cents: i64, _since: NaiveDate, _until: NaiveDate) -> Ownership { unimplemented!() }
pub fn months_ending(_today: NaiveDate, _months: u32, _totals: &[(String, i64)]) -> Vec<Amount> { unimplemented!() }
```

Append inside `mod tests` of `src/domain/stats.rs`:

```rust
    fn day(s: &str) -> NaiveDate { NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap() }

    #[test]
    fn the_month_window_ends_with_this_month_and_crosses_a_year() {
        let totals = vec![("2025-11".to_string(), 500), ("2026-02".to_string(), 70), ("2026-02".to_string(), 30), ("2025-10".to_string(), 999)];
        let w = months_ending(day("2026-02-14"), 4, &totals);
        assert_eq!(amounts(&w), [("2025-11", 500), ("2025-12", 0), ("2026-01", 0), ("2026-02", 100)]);
    }

    #[test]
    fn ownership_adds_the_purchase_price_and_measures_up_to_until() {
        let o = ownership(100_000, 300_000, day("2024-05-01"), day("2026-05-01"));
        assert_eq!((o.total_cents, o.purchase_cents, o.since.as_str()), (400_000, 300_000, "2024-05-01"));
        // 2024-05-01 to 2026-05-01 is 730 days.
        assert_eq!(o.per_year_cents, Some(400_000 * 365 / 730));
    }

    #[test]
    fn no_per_year_figure_under_ninety_days_owned() {
        assert_eq!(ownership(5_000, 0, day("2026-01-01"), day("2026-03-31")).per_year_cents, None, "89 days");
        assert_eq!(ownership(5_000, 0, day("2026-01-01"), day("2026-04-01")).per_year_cents, Some(5_000 * 365 / 90));
        assert_eq!(ownership(5_000, 0, day("2026-05-01"), day("2026-04-01")).per_year_cents, None, "since after until");
    }

    #[test]
    fn day_of_reads_dates_and_timestamps() {
        assert_eq!(day_of("2024-05-01"), Some(day("2024-05-01")));
        assert_eq!(day_of("2023-11-02T08:00:00Z"), Some(day("2023-11-02")));
        assert_eq!(day_of("2023-11"), None);
        assert_eq!(day_of("not a date"), None);
    }
```

In `src/domain/insights.rs`, add above `#[cfg(test)]`:

```rust
/// One fuel entry with its date, as the per-fill trend needs it.
#[derive(Clone, Debug)]
pub struct DatedFill {
    pub date: String,
    pub counter: i64,
    pub quantity_milli: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FillRate {
    pub date: String,
    pub per_100_milli: i64,
}

/// How many fills the trend draws.
pub const FILL_BARS: usize = 12;

pub fn consumption_per_fill(_fills: &[DatedFill]) -> Vec<FillRate> { unimplemented!() }
```

Append inside its `mod tests`:

```rust
    fn df(date: &str, counter: i64, quantity_milli: i64) -> DatedFill {
        DatedFill { date: date.into(), counter, quantity_milli }
    }

    fn rate(date: &str, per_100_milli: i64) -> FillRate {
        FillRate { date: date.into(), per_100_milli }
    }

    #[test]
    fn each_fill_is_measured_from_the_one_before_it() {
        // 30 L over 500 km, then 25 L over 500 km. The first fill only opens the window.
        let fills = [df("2026-01-01", 10_000, 40_000), df("2026-02-01", 10_500, 30_000), df("2026-03-01", 11_000, 25_000)];
        assert_eq!(consumption_per_fill(&fills), [rate("2026-02-01", 6_000), rate("2026-03-01", 5_000)]);
    }

    #[test]
    fn fills_are_put_in_date_order_first() {
        let fills = [df("2026-03-01", 11_000, 25_000), df("2026-01-01", 10_000, 40_000), df("2026-02-01", 10_500, 30_000)];
        assert_eq!(consumption_per_fill(&fills), [rate("2026-02-01", 6_000), rate("2026-03-01", 5_000)]);
    }

    #[test]
    fn a_distance_that_is_not_positive_gives_no_bar_but_starts_the_next_interval() {
        // February repeats the counter; March is after an odometer replacement; April is 500 km on.
        let fills = [df("2026-01-01", 10_000, 40_000), df("2026-02-01", 10_000, 30_000),
                     df("2026-03-01", 500, 20_000), df("2026-04-01", 1_000, 25_000)];
        assert_eq!(consumption_per_fill(&fills), [rate("2026-04-01", 5_000)]);
    }

    #[test]
    fn only_the_newest_twelve_are_kept() {
        let fills: Vec<DatedFill> = (0..20).map(|i| df(&format!("2026-01-{:02}", i + 1), 10_000 + i * 100, 5_000)).collect();
        let rates = consumption_per_fill(&fills);
        assert_eq!(rates.len(), FILL_BARS);
        assert_eq!(rates[0].date, "2026-01-09");
        assert_eq!(rates[11].date, "2026-01-20");
    }

    #[test]
    fn fewer_than_two_fills_give_nothing() {
        assert!(consumption_per_fill(&[]).is_empty());
        assert!(consumption_per_fill(&[df("2026-01-01", 10_000, 40_000)]).is_empty());
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib domain::stats domain::insights` (if cargo accepts only one filter, run `cargo test --lib domain::` instead)
Expected: the 9 new tests FAIL with `not implemented`; existing tests pass.

- [ ] **Step 4: Implement**

In `src/domain/stats.rs`, replace the stubs with:

```rust
/// Fewer days owned than this and there is no per-year figure: a few weeks' spend multiplied out
/// to a year is not a number anyone can plan with.
pub const MIN_DAYS_FOR_PER_YEAR: i64 = 90;

/// What owning an object has cost: running costs plus the purchase price actually counted, and
/// that spread over the years owned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ownership {
    pub total_cents: i64,
    pub purchase_cents: i64,
    /// `YYYY-MM-DD`: the purchase date, or the day the object was created.
    pub since: String,
    pub per_year_cents: Option<i64>,
}

/// The day at the start of a stored date (`YYYY-MM-DD`) or timestamp (RFC 3339), or None when it
/// does not parse.
pub fn day_of(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// Ownership from `since` up to `until` -- the archive date for an object no longer in use, today
/// otherwise. Integer arithmetic: cents × 365 ÷ days.
pub fn ownership(running_cents: i64, purchase_cents: i64, since: NaiveDate, until: NaiveDate) -> Ownership {
    let total_cents = running_cents + purchase_cents;
    let days = (until - since).num_days();
    let per_year_cents = (days >= MIN_DAYS_FOR_PER_YEAR).then(|| total_cents * 365 / days);
    Ownership { total_cents, purchase_cents, since: since.to_string(), per_year_cents }
}

/// `months` calendar months ending with today's, oldest first, each with the sum of `totals`
/// for that `YYYY-MM` -- 0 for a month without spend, since no cost entry does mean nothing spent.
pub fn months_ending(today: NaiveDate, months: u32, totals: &[(String, i64)]) -> Vec<Amount> {
    let Some(this_month) = NaiveDate::from_ymd_opt(today.year(), today.month(), 1) else { return Vec::new() };
    let buckets = (0..months)
        .rev()
        .filter_map(|back| this_month.checked_sub_months(Months::new(back)))
        .map(|d| d.format("%Y-%m").to_string())
        .collect();
    fill(buckets, |b| totals.iter().filter(|(m, _)| m == b).map(|(_, c)| c).sum())
}

/// One `Amount` per bucket, in the order given, zeros included.
fn fill(buckets: Vec<String>, amount_of: impl Fn(&str) -> i64) -> Vec<Amount> {
    buckets
        .into_iter()
        .map(|bucket| {
            let cost_cents = amount_of(&bucket);
            Amount { bucket, cost_cents }
        })
        .collect()
}
```

And in `summarize`, replace the `Some(y) => ...` arm of `over_time` with:

```rust
        Some(y) => fill(
            (1..=12).map(|m| format!("{y:04}-{m:02}")).collect(),
            |b| selected.iter().filter(|s| s.month == b).map(|s| s.cost_cents).sum(),
        ),
```

In `src/domain/insights.rs`, replace the `consumption_per_fill` stub with:

```rust
/// Quantity per 100 counter units for each fill, measured from the fill before it; the newest
/// `FILL_BARS`, oldest first.
///
/// The tank method one interval at a time: a fill's fuel was burned over the distance since the
/// previous fill, so the first fill has no figure. Fills are taken in date order, not counter
/// order, so a replaced odometer shows up as a distance that is not positive -- and that, like
/// the same counter typed twice, gives no figure rather than a spike. The fill still starts the
/// next interval.
pub fn consumption_per_fill(fills: &[DatedFill]) -> Vec<FillRate> {
    let mut sorted = fills.to_vec();
    sorted.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.counter.cmp(&b.counter)));
    let mut rates: Vec<FillRate> = sorted
        .windows(2)
        .filter_map(|w| {
            let distance = w[1].counter - w[0].counter;
            (distance > 0).then(|| FillRate { date: w[1].date.clone(), per_100_milli: w[1].quantity_milli * 100 / distance })
        })
        .collect();
    let skip = rates.len().saturating_sub(FILL_BARS);
    rates.drain(..skip);
    rates
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib domain::` then `cargo clippy --all-targets -- -D warnings`
Expected: all domain tests pass (the 15 existing stats tests included), no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/domain/stats.rs src/domain/insights.rs
git commit -m "feat: ownership, month window and consumption per fill in the domain"
```

---

### Task 2: API — `?contents=true` and the new insights fields

**Files:**
- Modify: `src/api/insights.rs`, `tests/insights.rs`, `docs/openapi.json`

**Interfaces:**
- Consumes (Task 1): `domain::stats::{self, day_of, months_ending, purchase_spend, Amount, Ownership, ObjectRow}`, `domain::insights::{consumption_per_fill, DatedFill, FillRate}`.
- Produces (Tasks 3–4): `GET /api/objects/{id}/insights?contents=true|false` adds to the existing JSON:
  ```json
  { "has_contents": true,
    "ownership": { "total_cents": 0, "purchase_cents": 0, "since": "YYYY-MM-DD", "per_year_cents": null },
    "by_month": [{ "bucket": "YYYY-MM", "cost_cents": 0 }],
    "fuel": { "...": "...", "fills": [{ "date": "YYYY-MM-DD", "per_100_milli": 0 }] } }
  ```
  `by_year`, `by_category`, `by_month`, `ownership` include descendants with `contents=true`. Non-boolean `contents`: 400.

- [ ] **Step 1: Write failing integration tests**

Append to `tests/insights.rs` (check first that `logb::db::today` is reachable from integration tests — `tests/common/mod.rs` already uses `logb::` paths; if `db` is not `pub`, read today's date with `chrono::Local::now()` in the configured test timezone instead and say so in the report):

```rust
use chrono::NaiveDate;

async fn object(app: &common::TestApp, body: serde_json::Value) -> i64 {
    let res = app.client.post(app.url("/objects")).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    res.json::<serde_json::Value>().await.unwrap()["id"].as_i64().unwrap()
}

async fn entry(app: &common::TestApp, object_id: i64, body: serde_json::Value) {
    let res = app.client.post(app.url(&format!("/objects/{object_id}/activities"))).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
}

#[tokio::test]
async fn contents_add_every_descendants_costs_but_leave_counter_figures_alone() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = object(&app, json!({ "name": "House", "type": "home", "description": "",
        "purchase_date": "2024-05-01", "purchase_price_cents": 300_000 })).await;
    let boiler = object(&app, json!({ "name": "Boiler", "type": "appliance", "description": "", "parent_id": house,
        "purchase_date": "2025-01-01", "purchase_price_cents": 50_000 })).await;
    let bulb = object(&app, json!({ "name": "Bulb", "type": "appliance", "description": "", "parent_id": boiler })).await;
    let today = logb::db::today();
    entry(&app, house, json!({ "date": "2025-03-10", "category": "repair", "title": "Roof", "notes": "", "cost_cents": 100_000 })).await;
    entry(&app, boiler, json!({ "date": "2026-02-01", "category": "maintenance", "title": "Service", "notes": "", "cost_cents": 25_000 })).await;
    entry(&app, bulb, json!({ "date": today, "category": "repair", "title": "Swap", "notes": "", "cost_cents": 3_000 })).await;

    let own = app.get_json(&format!("/objects/{house}/insights")).await;
    assert_eq!(own["has_contents"], true);
    assert_eq!(own["ownership"]["total_cents"], 400_000);
    assert_eq!(own["ownership"]["purchase_cents"], 300_000);
    assert_eq!(own["ownership"]["since"], "2024-05-01");
    let days = (NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap() - NaiveDate::from_ymd_opt(2024, 5, 1).unwrap()).num_days();
    assert_eq!(own["ownership"]["per_year_cents"], 400_000 * 365 / days);
    let months = own["by_month"].as_array().unwrap();
    assert_eq!(months.len(), 12);
    assert_eq!(months[11], json!({ "bucket": &today[..7], "cost_cents": 0 }), "the bulb is not the house's own");
    assert_eq!(own["by_year"].as_array().unwrap().len(), 1, "only the house's own 2025");

    let all = app.get_json(&format!("/objects/{house}/insights?contents=true")).await;
    assert_eq!(all["ownership"]["total_cents"], 478_000);
    assert_eq!(all["ownership"]["purchase_cents"], 350_000);
    assert_eq!(all["ownership"]["since"], "2024-05-01", "since stays the house's own");
    assert_eq!(all["by_month"][11]["cost_cents"], 3_000);
    let cats = all["by_category"].as_array().unwrap();
    assert!(cats.iter().any(|c| c["bucket"] == "maintenance" && c["cost_cents"] == 25_000), "{cats:?}");
    for field in ["counter_span", "cost_per_counter_milli", "fuel", "counter_per_day_milli", "usage_by_month"] {
        assert_eq!(all[field], own[field], "{field} must ignore contents");
    }

    let leaf = app.get_json(&format!("/objects/{bulb}/insights")).await;
    assert_eq!(leaf["has_contents"], false);
}

#[tokio::test]
async fn fills_draw_a_trend_and_a_purchase_entry_is_the_purchase() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let car = object(&app, json!({ "name": "Car", "type": "car", "counter_unit": "km", "description": "",
        "purchase_price_cents": 900_000 })).await;
    entry(&app, car, json!({ "date": "2023-01-15", "category": "purchase", "title": "Bought", "notes": "", "cost_cents": 900_000 })).await;
    for (date, counter, qty) in [("2026-01-01", 10_000, 40_000), ("2026-02-01", 10_500, 30_000), ("2026-03-01", 11_000, 25_000)] {
        entry(&app, car, json!({ "date": date, "category": "fuel", "title": "Fuel", "notes": "",
            "counter_value": counter, "quantity_milli": qty })).await;
    }

    let out = app.get_json(&format!("/objects/{car}/insights?contents=true")).await;
    assert_eq!(out["has_contents"], false);
    assert_eq!(out["ownership"]["purchase_cents"], 0, "the purchase entry already counts it");
    assert_eq!(out["ownership"]["total_cents"], 900_000);
    assert!(out["ownership"]["per_year_cents"].is_null(), "created today: under 90 days owned");
    assert_eq!(out["fuel"]["fills"], json!([
        { "date": "2026-02-01", "per_100_milli": 6_000 },
        { "date": "2026-03-01", "per_100_milli": 5_000 },
    ]));
}

#[tokio::test]
async fn contents_must_be_a_boolean_and_another_users_object_stays_hidden() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let house = object(&app, json!({ "name": "House", "type": "home", "description": "" })).await;
    let res = app.client.get(app.url(&format!("/objects/{house}/insights?contents=maybe"))).send().await.unwrap();
    assert_eq!(res.status(), 400);
    let anna = app.create_user_client("anna", "password123").await;
    let res = anna.get(app.url(&format!("/objects/{house}/insights?contents=true"))).send().await.unwrap();
    assert_eq!(res.status(), 404);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test insights`
Expected: the 3 new tests FAIL (missing fields / `contents=maybe` returns 200); the 3 existing tests pass.

- [ ] **Step 3: Implement**

In `src/api/insights.rs`:

Imports become:

```rust
use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::db;
use crate::domain::insights::{
    consumption_per_100_milli, consumption_per_fill, cost_per_counter_milli, daily_rate_milli, default_fuel_unit,
    fuel_cost_per_counter_milli, latest_reading, monthly_usage, DatedFill, Fill, FillRate, MonthUsage, Reading, RATE_WINDOW_DAYS,
};
use crate::domain::stats::{self, day_of, months_ending, purchase_spend, Amount, Ownership};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
```

`FuelOut` gains a field:

```rust
    /// Consumption per fill, oldest first -- see `domain::insights::consumption_per_fill`.
    pub fills: Vec<FillRate>,
```

`InsightsOut` gains:

```rust
    /// Whether the object has any non-deleted child, so a client knows if `contents` would change anything.
    pub has_contents: bool,
    pub ownership: Ownership,
    /// The last twelve calendar months of spend, oldest first, zeros included.
    pub by_month: Vec<Amount>,
```

Add:

```rust
#[derive(Deserialize)]
pub struct InsightsQuery {
    #[serde(default)]
    pub contents: Option<bool>,
}

/// The objects a cost figure covers, as a `WITH` prefix that every cost query below reads through
/// `object_id IN (SELECT id FROM scope)`: the object alone, or with contents the object and every
/// non-deleted descendant. `UNION`, not `UNION ALL`, so even a corrupt parent loop terminates, as
/// in `objects::ancestors`. The root was already checked to be the caller's by
/// `load_owned_object`, and a parent can only ever be set to one of the caller's own objects.
fn scope(contents: bool) -> &'static str {
    if contents {
        "WITH RECURSIVE scope(id) AS ( \
           SELECT id FROM objects WHERE id = $1 \
           UNION \
           SELECT o.id FROM objects o JOIN scope s ON o.parent_id = s.id WHERE o.deleted_at IS NULL \
         ) "
    } else {
        "WITH scope(id) AS (SELECT id FROM objects WHERE id = $1) "
    }
}
```

Change the handler signature to:

```rust
async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
    Query(q): Query<InsightsQuery>,
) -> Result<Json<InsightsOut>, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    let scope = scope(q.contents.unwrap_or(false));
    let today = NaiveDate::parse_from_str(&db::today(), "%Y-%m-%d").expect("server-generated date is always valid");
```

Replace the `by_year` and `by_category` queries with scoped versions (same columns, same filters):

```rust
    let by_year_sql = format!(
        "{scope}SELECT substr(date, 1, 4) AS bucket, COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL \
         GROUP BY bucket ORDER BY bucket DESC"
    );
    let by_year = sqlx::query_as::<_, Bucket>(&by_year_sql).bind(object_id).fetch_all(&state.db).await?;

    // Readings are left out: they never carry a cost, and a monthly reading habit would put
    // an empty "Reading" bar at the bottom of every car's breakdown.
    let by_category_sql = format!(
        "{scope}SELECT category AS bucket, COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) AS cost_cents, \
         COUNT(*) AS count FROM activities WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL \
         AND category <> 'reading' GROUP BY category ORDER BY cost_cents DESC"
    );
    let by_category = sqlx::query_as::<_, Bucket>(&by_category_sql).bind(object_id).fetch_all(&state.db).await?;
```

Leave the `(min_counter, max_counter, total_cost)` query on `object_id = $1` (it feeds counter figures, which stay the object's own).

After it, add ownership, months and children:

```rust
    let running_sql = format!(
        "{scope}SELECT COALESCE(CAST(SUM(cost_cents) AS BIGINT), 0) FROM activities \
         WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL"
    );
    let (running_cents,): (i64,) = sqlx::query_as(&running_sql).bind(object_id).fetch_one(&state.db).await?;

    let scoped_objects_sql = format!(
        "{scope}SELECT id, parent_id, name, type, archived_at, purchase_date, purchase_price_cents, created_at \
         FROM objects WHERE id IN (SELECT id FROM scope)"
    );
    #[allow(clippy::type_complexity)]
    let object_rows: Vec<(i64, Option<i64>, String, String, Option<String>, Option<String>, Option<i64>, String)> =
        sqlx::query_as(&scoped_objects_sql).bind(object_id).fetch_all(&state.db).await?;
    let scoped: Vec<stats::ObjectRow> = object_rows
        .into_iter()
        .map(|(id, parent_id, name, kind, archived_at, purchase_date, purchase_price_cents, created_at)| stats::ObjectRow {
            id, parent_id, name, kind, archived: archived_at.is_some(), purchase_date, purchase_price_cents, created_at,
        })
        .collect();
    // A purchase entry with a cost is the purchase; see `stats::purchase_spend`.
    let purchased_sql = format!(
        "{scope}SELECT DISTINCT object_id FROM activities WHERE object_id IN (SELECT id FROM scope) \
         AND deleted_at IS NULL AND category = 'purchase' AND cost_cents > 0"
    );
    let purchased: Vec<(i64,)> = sqlx::query_as(&purchased_sql).bind(object_id).fetch_all(&state.db).await?;
    let purchased: HashSet<i64> = purchased.into_iter().map(|(id,)| id).collect();
    let purchase_cents: i64 = purchase_spend(&scoped, &purchased).iter().map(|s| s.cost_cents).sum();

    // "Since" and "until" are always the object's own: a boiler bought later does not shorten
    // how long the house has been owned.
    let since = object.purchase_date.as_deref().and_then(day_of).or_else(|| day_of(&object.created_at)).unwrap_or(today);
    let until = object.archived_at.as_deref().and_then(day_of).unwrap_or(today);
    let ownership = stats::ownership(running_cents, purchase_cents, since, until);

    let months_sql = format!(
        "{scope}SELECT substr(date, 1, 7), CAST(SUM(cost_cents) AS BIGINT) FROM activities \
         WHERE object_id IN (SELECT id FROM scope) AND deleted_at IS NULL AND cost_cents IS NOT NULL \
         GROUP BY substr(date, 1, 7)"
    );
    let month_totals: Vec<(String, i64)> = sqlx::query_as(&months_sql).bind(object_id).fetch_all(&state.db).await?;
    let by_month = months_ending(today, USAGE_MONTHS, &month_totals);

    let (children,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects WHERE parent_id = $1 AND deleted_at IS NULL")
        .bind(object_id)
        .fetch_one(&state.db)
        .await?;
```

Change the fill query to also read the date:

```rust
    let fill_rows: Vec<(String, i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT date, counter_value, quantity_milli, cost_cents FROM activities \
         WHERE object_id = $1 AND deleted_at IS NULL AND category = 'fuel' AND counter_value IS NOT NULL \
         AND quantity_milli IS NOT NULL ORDER BY counter_value",
    )
```

and in the `fuel` block map `fill_rows.iter().map(|(_, counter, quantity_milli, cost_cents)| Fill { .. })`, then add to `FuelOut { .. }`:

```rust
            fills: consumption_per_fill(
                &fill_rows
                    .iter()
                    .map(|(date, counter, quantity_milli, _)| DatedFill { date: date.clone(), counter: *counter, quantity_milli: *quantity_milli })
                    .collect::<Vec<_>>(),
            ),
```

In the `usage_by_month` block, delete its own `let today = ...` line (the one at the top of the handler is used).

Return:

```rust
    Ok(Json(InsightsOut {
        by_year,
        by_category,
        counter_span,
        cost_per_counter_milli: overall_cost_per_counter_milli,
        fuel,
        counter_per_day_milli,
        usage_by_month,
        has_contents: children > 0,
        ownership,
        by_month,
    }))
```

`HashMap` stays imported for `usage_by_object`.

- [ ] **Step 4: Run tests**

Run: `cargo test --test insights`
Expected: 6 passed.

- [ ] **Step 5: OpenAPI**

In `docs/openapi.json`, `"/objects/{id}/insights"."get"."parameters"` gains after the `id` parameter:

```json
{ "name": "contents", "in": "query", "required": false, "schema": { "type": "boolean", "default": false },
  "description": "Include every non-deleted descendant in by_year, by_category, by_month and ownership. Counter and fuel figures always stay the object's own." }
```

In `"components"."schemas"."Insights"."properties"` add:

```json
"has_contents": { "type": "boolean", "description": "Whether the object has any non-deleted child" },
"ownership": {
  "type": "object",
  "required": ["total_cents", "purchase_cents", "since", "per_year_cents"],
  "properties": {
    "total_cents": { "type": "integer", "format": "int64", "description": "Running costs plus purchase_cents" },
    "purchase_cents": { "type": "integer", "format": "int64", "description": "Purchase prices counted; an object with a costed purchase entry contributes 0" },
    "since": { "type": "string", "description": "YYYY-MM-DD: purchase date, else creation date" },
    "per_year_cents": { "type": ["integer", "null"], "format": "int64", "description": "total_cents per 365 days owned, until the archive date or today; null under 90 days" }
  }
},
"by_month": { "type": "array", "description": "Twelve calendar months ending with the current one, oldest first, zeros included", "items": { "$ref": "#/components/schemas/StatsAmount" } },
```

and inside the `fuel` property's `properties` object add:

```json
"fills": { "type": "array", "description": "Consumption per fill for up to the twelve newest fills, oldest first",
  "items": { "type": "object", "required": ["date", "per_100_milli"],
    "properties": { "date": { "type": "string" }, "per_100_milli": { "type": "integer", "format": "int64" } } } }
```

(If `fuel` in the schema is a `$ref` or has no `properties` object, add `fills` where its other fields — `unit`, `quantity_milli` — are described.)

Run: `python3 -m json.tool docs/openapi.json > /dev/null && cargo test --test openapi`
Expected: valid; PASS.

- [ ] **Step 6: PostgreSQL, full suite, commit**

Run the insights tests on PostgreSQL:

```bash
docker run -d --rm --name logb-cost-pg -e POSTGRES_PASSWORD=pg -p 55433:5432 postgres:17
until docker exec logb-cost-pg pg_isready -U postgres; do sleep 1; done
LOGB_TEST_DATABASE_URL=postgres://postgres:pg@127.0.0.1:55433/postgres cargo test --test insights
docker stop logb-cost-pg
```

Expected: 6 passed. Always stop the container, even on failure.

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass, no warnings.

```bash
git add src/api/insights.rs tests/insights.rs docs/openapi.json
git commit -m "feat: insights include contents on request, with ownership, spend per month and consumption per fill"
```

---

### Task 3: Frontend — types, helpers, strings

**Files:**
- Create: `frontend/src/lib/insights.ts`, `frontend/tests/insights.test.ts`
- Modify: `frontend/src/lib/types.ts`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`

**Interfaces:**
- Consumes (Task 2): the JSON above.
- Produces (Task 4):
  ```ts
  // types.ts — Insights gains:
  has_contents: boolean;
  ownership: { total_cents: number; purchase_cents: number; since: string; per_year_cents: number | null };
  by_month: Amount[];
  // and Insights.fuel gains: fills: { date: string; per_100_milli: number }[]
  // insights.ts
  export function insightsPath(objectId: number, contents: boolean): string
  export function sinceLabel(date: string, locale: string): string
  export function monthLabel(month: string, locale: string): string
  export function fillLabel(date: string, locale: string): string
  // i18n: insights.contents, insights.ownership, insights.per-year-since, insights.since,
  //   insights.spend-by-month, insights.by-fill, insights.by-fill-hint
  ```

- [ ] **Step 1: Write failing tests**

Create `frontend/tests/insights.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { fillLabel, insightsPath, monthLabel, sinceLabel } from '../src/lib/insights';

describe('insightsPath', () => {
  it('asks for the object alone by default', () => {
    expect(insightsPath(7, false)).toBe('/objects/7/insights');
  });
  it('asks for contents only when on', () => {
    expect(insightsPath(7, true)).toBe('/objects/7/insights?contents=true');
  });
});

describe('sinceLabel', () => {
  it('names the month and year the object has been owned since', () => {
    expect(sinceLabel('2024-05-01', 'en')).toBe('May 2024');
    expect(sinceLabel('2024-05-01', 'de')).toBe('Mai 2024');
  });
});

describe('monthLabel', () => {
  it('keeps the year, because a twelve-month window crosses one', () => {
    expect(monthLabel('2026-09', 'en')).toBe('Sep 26');
    expect(monthLabel('2025-09', 'en')).toBe('Sep 25');
    expect(monthLabel('2026-09', 'de')).toMatch(/26$/);
  });
});

describe('fillLabel', () => {
  it('shows day and month', () => {
    expect(fillLabel('2026-01-10', 'en')).toBe('Jan 10');
    expect(fillLabel('2026-01-10', 'de')).toMatch(/^10\. Jan/);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd frontend && npx vitest run tests/insights.test.ts`
Expected: FAIL — cannot resolve `../src/lib/insights`.

- [ ] **Step 3: Implement**

Create `frontend/src/lib/insights.ts`:

```ts
/** The request for an object's insights. `contents` is left out when off, so the common case is the plain path. */
export function insightsPath(objectId: number, contents: boolean): string {
  return contents ? `/objects/${objectId}/insights?contents=true` : `/objects/${objectId}/insights`;
}

/** Noon UTC on the day, so no timezone moves the date across midnight. */
function utc(y: number, m: number, d: number): Date {
  return new Date(Date.UTC(y, m - 1, d, 12));
}

/** `2024-05-01` as "May 2024". */
export function sinceLabel(date: string, locale: string): string {
  const [y, m] = date.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { month: 'short', year: 'numeric', timeZone: 'UTC' }).format(utc(y, m, 1));
}

/** `2026-09` as "Sep 26". The year stays, unlike on the Statistics screen: the last twelve months
 *  cross a year, and two bars both labelled "Sep" would be ambiguous. */
export function monthLabel(month: string, locale: string): string {
  const [y, m] = month.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { month: 'short', year: '2-digit', timeZone: 'UTC' }).format(utc(y, m, 15));
}

/** `2026-01-10` as "Jan 10". */
export function fillLabel(date: string, locale: string): string {
  const [y, m, d] = date.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', timeZone: 'UTC' }).format(utc(y, m, d));
}
```

In `frontend/src/lib/types.ts`, inside `export interface Insights { ... }`:
- change the `fuel` line to
  ```ts
  fuel: { unit: string; quantity_milli: number; per_100_milli: number | null; cost_per_counter_milli: number | null;
    /** Consumption per fill, oldest first, up to twelve. */
    fills: { date: string; per_100_milli: number }[] } | null;
  ```
- add before the closing `}`:
  ```ts
  /** Whether the object has any non-deleted child. */
  has_contents: boolean;
  /** Running costs plus purchase prices counted; `per_year_cents` null under 90 days owned. */
  ownership: { total_cents: number; purchase_cents: number; since: string; per_year_cents: number | null };
  /** Twelve months ending with the current one, oldest first, zeros included. */
  by_month: Amount[];
  ```
(`Amount` is already declared in this file, below `Insights`; TypeScript interfaces may reference later declarations.)

In `frontend/src/i18n/en.ts`, after `'insights.usage-by-month': 'Per month',` add:

```ts
  'insights.contents': 'Include contents',
  'insights.ownership': 'Total cost of ownership',
  'insights.per-year-since': '≈ {amount} a year since {since}',
  'insights.since': 'since {since}',
  'insights.spend-by-month': 'Spend per month',
  'insights.by-fill': 'Consumption per fill',
  'insights.by-fill-hint': 'Assumes every fill tops the tank up.',
```

In `frontend/src/i18n/de.ts`, same position:

```ts
  'insights.contents': 'Inhalt einrechnen',
  'insights.ownership': 'Gesamtkosten',
  'insights.per-year-since': '≈ {amount} pro Jahr seit {since}',
  'insights.since': 'seit {since}',
  'insights.spend-by-month': 'Ausgaben pro Monat',
  'insights.by-fill': 'Verbrauch pro Tankfüllung',
  'insights.by-fill-hint': 'Setzt voraus, dass jedes Mal vollgetankt wurde.',
```

- [ ] **Step 4: Run tests**

Run: `cd frontend && npx vitest run && npm run check`
Expected: all pass (new insights tests included); check may report errors in `ReadingForm.svelte` or others only if they construct an `Insights` literal — fix by adding the new fields there; otherwise 0 errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/insights.ts frontend/tests/insights.test.ts frontend/src/lib/types.ts frontend/src/i18n/en.ts frontend/src/i18n/de.ts
git commit -m "feat: insights types, labels and strings for ownership, months and fills"
```

---

### Task 4: Info tab UI and end-to-end

**Files:**
- Modify: `frontend/src/lib/Insights.svelte`, `frontend/src/routes/ObjectDetail.svelte`, `docs/superpowers/specs/2026-09-14-object-cost-depth-design.md`, `README.md`
- Create: `frontend/tests-e2e/21-object-cost-depth.spec.ts`

**Interfaces:**
- Consumes: Task 2 JSON; Task 3 types, helpers, strings; `BarList` (`items: Bar[]`); `persisted` from `stores/persisted.ts`.
- Produces: `<Insights objectId unit hasContents />`.

- [ ] **Step 1: Write the failing e2e spec**

Create `frontend/tests-e2e/21-object-cost-depth.spec.ts`:

```ts
import { test, expect, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

async function entry(page: Page, id: number, data: Record<string, unknown>) {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', title: 'Entry', ...data } });
  expect(res.ok()).toBe(true);
}

async function openInfo(page: Page, id: number) {
  await page.goto(`/objects/${id}`);
  await page.getByRole('button', { name: 'Info', exact: true }).click();
}

test('a car shows consumption per fill and no contents switch', async ({ page }) => {
  await signInFresh(page, '21-cost-car');
  const car = await object(page, { name: 'Depth Car', type: 'car', counter_unit: 'km' });
  for (const [date, counter_value, quantity_milli] of [['2026-01-01', 10_000, 40_000], ['2026-02-01', 10_500, 30_000], ['2026-03-01', 11_000, 25_000]]) {
    await entry(page, car as number, { date, category: 'fuel', counter_value, quantity_milli });
  }

  await openInfo(page, car);
  await expect(page.getByTestId('insights-by-fill').locator('.bar-row')).toHaveCount(2);
  await expect(page.getByLabel('Include contents')).toHaveCount(0);
});

test('a house includes its boiler on request, and remembers the choice', async ({ page }) => {
  await signInFresh(page, '21-cost-house');
  const house = await object(page, { name: 'Depth House', type: 'home', purchase_date: '2024-05-01', purchase_price_cents: 300_000 });
  const boiler = await object(page, { name: 'Depth Boiler', type: 'appliance', parent_id: house });
  await entry(page, house, { date: '2025-03-10', category: 'repair', cost_cents: 100_000 });
  await entry(page, boiler, { date: '2026-02-01', category: 'maintenance', cost_cents: 25_000 });

  await openInfo(page, house);
  const ownership = page.getByTestId('insights-ownership');
  await expect(ownership).toContainText('4,000.00');
  const toggle = page.getByLabel('Include contents');
  await expect(toggle).not.toBeChecked();
  await toggle.check();
  await expect(ownership).toContainText('4,250.00');

  await page.reload();
  await page.getByRole('button', { name: 'Info', exact: true }).click();
  await expect(page.getByLabel('Include contents')).toBeChecked();
  await expect(page.getByTestId('insights-ownership')).toContainText('4,250.00');
});
```

Run: `cd frontend && npm run e2e -- 21-object-cost-depth` (foreground, timeout up to 600000 ms)
Expected: FAIL — `insights-by-fill` / `insights-ownership` not found.

- [ ] **Step 2: Rewrite `Insights.svelte`**

Replace `frontend/src/lib/Insights.svelte` entirely with:

```svelte
<script lang="ts">
  import { api } from './api';
  import BarList from './BarList.svelte';
  import { counter, money, perCounter, quantity } from './format';
  import { fillLabel, insightsPath, monthLabel, sinceLabel } from './insights';
  import { persisted } from '../stores/persisted';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, Insights } from './types';

  let { objectId, unit, hasContents = false }: { objectId: number; unit: CounterUnit; hasContents?: boolean } = $props();

  /** Per device and not synced, like the Statistics screen's purchase switch: a way of looking, not data. */
  const includeContents = persisted('logb.insights.contents', false);

  let data = $state<Insights | null>(null);
  let error = $state('');

  $effect(() => {
    // An object without children always asks for its own figures, whatever the switch last said
    // on a house.
    const path = insightsPath(objectId, hasContents && $includeContents);
    // Reset before the fetch, not just on success: without this, switching to another object
    // shows the previous one's cost breakdown under the new object's name for however long the
    // request takes -- and indefinitely if it fails, since neither `data` nor `error` was ever
    // touched for the new id.
    data = null;
    error = '';
    let current = true;
    api<Insights>('GET', path)
      .then((d) => { if (current) data = d; })
      .catch((e) => { if (current) error = (e as Error).message; });
    return () => { current = false; };
  });

  const fmt = (cents: number) => money(cents, $currency, $locale);
</script>

{#if hasContents}
  <label class="row toggle">
    <input type="checkbox" bind:checked={$includeContents} />
    {$t('insights.contents')}
  </label>
{/if}
{#if error}<p class="error">{error}</p>{/if}
{#if data}
  {#if data.ownership.total_cents > 0}
    {@const o = data.ownership}
    <p data-testid="insights-ownership">
      {$t('insights.ownership')}: <b class="tnum">{fmt(o.total_cents)}</b>
      <span class="muted">· {o.per_year_cents !== null
        ? $t('insights.per-year-since', { amount: fmt(o.per_year_cents), since: sinceLabel(o.since, $locale) })
        : $t('insights.since', { since: sinceLabel(o.since, $locale) })}</span>
    </p>
  {/if}
  {#if data.by_year.length === 0}
    <p class="muted">{$t('insights.none')}</p>
  {:else}
    <h3>{$t('insights.spend-by-month')}</h3>
    <BarList items={data.by_month.map((b) => ({ key: b.bucket, label: monthLabel(b.bucket, $locale), value: b.cost_cents, display: fmt(b.cost_cents) }))} />

    <h3>{$t('insights.by-year')}</h3>
    <BarList items={data.by_year.map((b) => ({ key: b.bucket, label: b.bucket, value: b.cost_cents, display: fmt(b.cost_cents) }))} />

    <h3>{$t('insights.by-category')}</h3>
    <BarList items={data.by_category.map((b) => ({ key: b.bucket, label: $t(`cat.${b.bucket}`), value: b.cost_cents, display: fmt(b.cost_cents) }))} />

    {#if data.cost_per_counter_milli !== null && unit}
      <p class="muted">{$t('insights.per-counter', { unit })}: <b>{perCounter(data.cost_per_counter_milli, $currency, $locale)}</b></p>
    {/if}
  {/if}
  {#if data.fuel}
    {@const fuel = data.fuel}
    <p class="muted">{$t('insights.fuel-total')}: <b>{quantity(fuel.quantity_milli, fuel.unit, $locale)}</b></p>
    {#if fuel.per_100_milli !== null && unit}
      <p class="muted">{$t('insights.consumption')}: <b>{quantity(fuel.per_100_milli, fuel.unit, $locale)}/100 {unit}</b></p>
      <!-- `cost_per_counter_milli` is null under exactly the same condition as `per_100_milli`
           (both need >= 2 fills spanning a positive counter distance -- see
           `fuel_cost_per_counter_milli` / `consumption_per_100_milli` in
           src/domain/insights.rs), so this guard covers both. Same scale as the overall
           per-counter figure above (milli-cents per unit), hence the same `perCounter`
           formatter. -->
      <p class="muted">{$t('insights.fuel-per-counter', { unit })}: <b>{perCounter(fuel.cost_per_counter_milli, $currency, $locale)}</b></p>
    {/if}
  {/if}
  {#if data.counter_per_day_milli !== null && unit}
    <!-- A month is the unit people think in for mileage; 30.44 days is the average one. Rounded
         to a whole unit, since the rate is an average and more digits would claim precision it
         does not have. -->
    <p class="muted">{$t('insights.usage')}: <b>{$t('insights.per-month', { amount: counter(Math.round(data.counter_per_day_milli * 30.44 / 1000), unit, $locale) })}</b></p>
  {/if}
  {#if data.usage_by_month.length > 0 && unit}
    <h3>{$t('insights.usage-by-month')}</h3>
    <!-- A month the readings cannot measure says so, rather than drawing a zero it does not know. -->
    <BarList items={data.usage_by_month.map((m) => ({
      key: m.month, label: monthLabel(m.month, $locale), value: m.amount ?? 0,
      display: m.amount === null ? '—' : counter(m.amount, unit, $locale),
    }))} />
  {/if}
  {#if data.fuel && data.fuel.fills.length > 0 && unit}
    {@const fuel = data.fuel}
    <section data-testid="insights-by-fill">
      <h3>{$t('insights.by-fill')}</h3>
      <p class="muted hint">{$t('insights.by-fill-hint')}</p>
      <BarList items={fuel.fills.map((f, i) => ({
        key: `${f.date}-${i}`, label: fillLabel(f.date, $locale), value: f.per_100_milli,
        display: `${quantity(f.per_100_milli, fuel.unit, $locale)}/100 ${unit}`,
      }))} />
    </section>
  {/if}
{/if}

<style>
  h3 { margin: var(--space-4) 0 var(--space-2); font-size: var(--text-base); }
  /* `.row > * { flex: 1 }` in app.css would otherwise stretch the checkbox across half the row. */
  .toggle input { flex: none; width: 20px; height: 20px; }
  .hint { font-size: var(--text-sm); margin: 0 0 var(--space-2); }
</style>
```

> Note two intentional label changes against the spec text: month bars use `monthLabel` (with a two-digit year) rather than `periodLabel`, and the existing usage-per-month bars keep the same format they had. Record both in the spec in Step 4.

- [ ] **Step 3: Pass `hasContents` from `ObjectDetail.svelte`**

Replace the line `<Insights objectId={oid} unit={object.counter_unit} />` with:

```svelte
      <Insights objectId={oid} unit={object.counter_unit} hasContents={children.length > 0} />
```

- [ ] **Step 4: Docs**

In `docs/superpowers/specs/2026-09-14-object-cost-depth-design.md`:
- Status line: `Status: implemented.`
- In "Frontend structure", replace "Month bars reuse `periodLabel` from `lib/stats.ts`." with "Month bars use `monthLabel` (short month and two-digit year): a twelve-month window crosses a year, and two bars both labelled "Sep" would be ambiguous."

In `README.md`, at the end of the "Statistics" section added earlier, add one sentence:

```
An object's Info tab shows the same rules for one object: total cost of ownership (≈ per year
once owned 90 days), spend per month, consumption per fill, and an "Include contents" switch on
objects that have others inside them.
```

- [ ] **Step 5: Verify**

Run (foreground):
1. `cd frontend && npm run check && npx vitest run` — expected 0 errors, all pass.
2. `cd frontend && npm run e2e -- 21-object-cost-depth` — expected 4 passed (2 tests × mobile + desktop).
3. `cd frontend && npm run e2e` — expected the whole suite passes. Re-run any failing pre-existing spec alone once and report both runs.
4. Screenshot at 390px width of the house's Info tab with the switch on (seed as in the spec), saved to `.superpowers/sdd/cost-depth-house-390.png`, and of the car's Info tab, saved to `.superpowers/sdd/cost-depth-car-390.png`. Look at both: switch beside its label, ownership line readable, bars aligned.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/Insights.svelte frontend/src/routes/ObjectDetail.svelte frontend/tests-e2e/21-object-cost-depth.spec.ts docs/superpowers/specs/2026-09-14-object-cost-depth-design.md README.md
git commit -m "feat: Info tab shows ownership, spend per month, consumption per fill and an include-contents switch"
```

---

## Self-Review Notes

- Spec coverage: switch + persistence (T4), ownership rules incl. purchase rule, since/until, 90 days, descendants (T1, T2), per month (T1, T2, T4), per year/category with contents (T2), counter figures untouched (T2 test loops over them), fuel per fill (T1, T2, T4), header stat unchanged (not touched), API fields and 400 (T2), OpenAPI (T2), frontend helpers (T3), errors/stale guard (T4), tests on all layers incl. PostgreSQL (T1–T4).
- Deviation recorded: `monthLabel` instead of `periodLabel` for month bars (T4 Step 4 updates the spec).
