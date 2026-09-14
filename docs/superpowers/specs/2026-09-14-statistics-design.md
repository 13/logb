# Statistics

Status: phase 1 implemented. Phase 2 not started.

Today every number LogB shows is about one object: the Info tab's Insights (cost by year and
category, cost per unit, fuel, usage). Nothing answers "what did everything cost me this year" or
"which object eats the money". Phase 1 adds an overall Statistics screen. Phase 2 (own spec, later)
deepens the per-object view.

## Phase 1: the Statistics screen

### Navigation

A fourth top-level destination, **Statistics** (`/stats`, new `chart` icon), between Search and
Settings. `nav.ts`'s `DESTINATIONS` and `activeDestination` gain the entry; its comment is updated
from "three" to "four" -- still few enough that all are always visible.

### Controls

- **Year selector.** "All years" (default), then every year that has spend, newest first. The
  year list comes from the response, so it never offers an empty year.
- **Include purchase prices** toggle, default off, remembered per device via `persisted`
  (`logb.stats.purchases`). Not synced: it is a way of looking, not data.

Both are sent as query parameters; the server does the arithmetic.

### Blocks, top to bottom

1. **Spend over time.** Headline total for the selection. "All years": one bar per year, oldest
   first. One year chosen: twelve bars, January to December. A month or year without spend shows
   0, not "—": unlike a counter reading, a missing cost entry does mean nothing was spent.
2. **By object.** One row per top-level object; its figure includes all descendants' spend. Share
   of the selection's total as a percentage. A row with children has an expand control listing
   each direct child with its own rolled-up subtotal (and so on down). Tapping a name opens the
   object. Archived objects are included and carry a muted "archived" label.
3. **By type.** One row per object type (car, home, bike, ...), attributed by each object's own
   type -- a boiler (`appliance`) inside a house counts under appliance, not home.
4. **By category.** One row per activity category, `reading` excluded as in Insights. With the
   toggle on, purchase prices form an extra **Purchase price** row, distinct from the `purchase`
   activity category.

Rows are ordered by amount, largest first; rows of 0 are omitted. Empty state for a selection with
no spend at all: "No costs recorded for this period."

The bar rows reuse Insights' look. That markup and CSS move into `lib/BarList.svelte`
(`items: { key, label, value, display, depth?, note?, onLabel?, expanded?, onToggle?,
toggleLabel? }[]`), which `Insights.svelte` then uses too -- no visual change there. An object's
name is a button (`onLabel`) that navigates in-app, like the rest of the app's navigation, not a
link.

### What counts as spend

- Every non-deleted activity's `cost_cents`, dated by the activity's `date`, on non-deleted
  objects owned by the signed-in user. Archived objects count.
- With the toggle on, an object's `purchase_price_cents`, dated by `purchase_date`. An object
  with a price but no `purchase_date` is dated by its `created_at` date.
- **No double counting.** If an object already has a non-deleted activity in category
  `purchase` with a cost, its `purchase_price_cents` is not added: that activity is the purchase.
- Activities dated in the future count in their own year. `reminders::reading_horizon()` guards
  counter readings against typo'd years; costs have no such filter, so a scheduled expense
  logged ahead is not hidden.

### API

`GET /stats?year=2026&purchases=true` -- both optional; `year` absent means all years. Year outside
1900..=9999 or not a number: 400.

```json
{
  "total_cents": 123400,
  "years": ["2026", "2025", "2024"],
  "over_time": [{ "bucket": "2026-01", "cost_cents": 5000 }],
  "by_object": [{ "id": 3, "name": "House", "type": "home", "archived": false,
                  "cost_cents": 80000, "children": [ /* same shape */ ] }],
  "by_type": [{ "bucket": "car", "cost_cents": 40000 }],
  "by_category": [{ "bucket": "maintenance", "cost_cents": 20000 }]
}
```

`over_time` buckets are `YYYY` for all years, `YYYY-MM` (all twelve, zeros included) for one
year. `by_category` uses the bucket `purchase_price` for the toggle row. `years` ignores the
`year` filter so the selector stays complete, and honours `purchases`.

Money is integer cents throughout. Every `SUM` is `CAST(... AS BIGINT)` for PostgreSQL, as in
`api/insights.rs`.

### Backend structure

- `src/domain/stats.rs` -- pure functions, no SQL:
  - `summarize(objects: &[ObjectRow], spend: &[Spend], year: Option<i32>) -> Stats`: filters to
    `year` if one is given, fills `over_time` (twelve months, or one bucket per year), and rolls
    the rest up via `roll_up`.
  - `roll_up(objects, own) -> Vec<ObjectNode>` (private): builds the tree, sums descendants, sorts
    siblings by amount, drops zero subtrees. An object whose parent is deleted or missing is
    treated as top-level.
  - `purchase_spend(objects: &[ObjectRow], purchased: &HashSet<i64>) -> Vec<Spend>`: applies the
    dating and no-double-count rules.
- `src/api/stats.rs` -- the route, three queries: per-activity-cost grouped by `(object_id,
  period, category)` for the selection, the user's objects (`id, parent_id, name, type,
  archived_at, purchase_date, purchase_price_cents, created_at`), and the set of objects with a
  costed `purchase` activity. Everything else is combined in Rust from those rows: a household
  has tens of objects and hundreds of costed activities.
- Registered in `api/mod.rs`; `docs/openapi.json` gains the path; `tests/openapi.rs` keeps them
  in step.

### Frontend structure

- `routes/Stats.svelte` -- controls, fetch, blocks.
- `lib/stats.ts` -- pure helpers (labels for buckets, share percentage, flattening expanded
  tree rows) with `tests/stats.test.ts`.
- `lib/BarList.svelte` -- shared bars.
- `types.ts` gains `Stats`; `i18n/en.ts` and `de.ts` gain the `stats.*` strings.
- Online only, like Insights. Offline: the existing request error is shown in place.

### Errors

Fetch failure shows the error message where the blocks would be, and a stale result is cleared
before each new fetch (the bug `Insights.svelte` already guards against). Bad query: 400 with a
reason.

### Tests

- **Rust unit (`domain/stats.rs`):** roll-up of a three-level tree; orphan becomes top-level;
  zero subtrees dropped; twelve months filled across a year with gaps; purchase price dated by
  `purchase_date`, falling back to `created_at`; skipped when a costed `purchase` activity exists.
- **Rust integration (`tests/stats.rs`, SQLite and PostgreSQL via the harness):** another user's
  objects never appear; deleted activities and objects excluded; archived included; `year`
  filters `over_time` and blocks but not `years`; `purchases=true` adds the row and the total;
  bad `year` is 400.
- **Vitest:** `stats.ts` helpers; `nav.test.ts` for the fourth destination.
- **Playwright (`20-statistics.spec.ts`, `signInFresh`):** seed a house with a child boiler and a
  car, log costs in two years; check totals, year switch to months, expanding the house shows the
  boiler, toggle adds the purchase row and survives a reload.

## Phase 2: per-object depth (outline, own spec later)

Candidates, to be narrowed then: total cost of ownership including purchase price on the Info
tab; the house-level roll-up of children there too, reusing `domain::stats::roll_up`; cost per
month for the last twelve months; consumption per fill as a trend. Phase 2 will reuse
`BarList.svelte` and the spend rules above so both screens agree on every number.

## Out of scope

Forecasting future spend, CSV export of statistics, free date ranges, multi-user household totals.
