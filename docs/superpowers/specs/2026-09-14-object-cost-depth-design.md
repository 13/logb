# Object cost depth (statistics phase 2)

Status: approved, not implemented. Follows `2026-09-14-statistics-design.md` (phase 1, the
Statistics screen).

An object's Info tab answers "what did this cost to run" but not "what has owning it cost me",
"what is inside it costing", "what did I spend lately" or "is it getting thirstier". This adds all
four to the existing Cost block, using the spend rules phase 1 already defined so the Statistics
screen and the Info tab never disagree about a number.

## Info tab, Cost block

Top to bottom:

1. **Include contents** switch. Shown only when the object has at least one non-deleted child.
   Off by default, remembered per device via `persisted` (`logb.insights.contents`). Not synced.
2. **Total cost of ownership**, e.g. "€4,000.00 · ≈ €1,300 a year since May 2024".
   - Total = running costs (every non-deleted activity's `cost_cents`) + purchase price.
   - The purchase price follows phase 1's rule: counted unless the object has a non-deleted
     `purchase` activity with `cost_cents > 0`, in which case that entry already is the purchase.
   - "Since" is the purchase date, or the object's creation date when there is none.
   - Per year = total ÷ years owned. Years owned run from "since" to the archive date if the object
     is archived, otherwise today. The per-year figure is omitted when fewer than 90 days are owned:
     a few weeks' spend multiplied out to a year is not a figure anyone can use.
   - With the switch on, every descendant's running costs and purchase price (same rule, per
     descendant) are added to the total. "Since" and years owned stay the object's own.
3. **Per month.** The last twelve calendar months ending with the current one, oldest first, every
   month present with 0 when nothing was spent. Running costs only.
4. **Per year** and **Per category**, as today. Running costs only.
5. **Cost per km, fuel totals, consumption, usage, usage per month**: unchanged and always the
   object's own. Counters belong to one object; adding a boiler's hours to a house makes no sense.
6. **Fuel per fill.** One bar per fill for the last twelve fills that have both a counter value and
   a quantity, oldest first, labelled with the fill's date. A fill's figure is its quantity divided
   by the distance since the previous such fill, per 100 counter units. The first fill ever has no
   previous fill and gets no bar; a fill whose distance is zero or negative (a typo, a replaced
   odometer) is skipped rather than drawn as a spike. A muted note says the figures assume each
   fill tops the tank up. The block is omitted when no fill produces a figure.

The switch changes blocks 2, 3 and 4 only. Purchase prices appear in block 2 only, never in the
bars, so the bars always mean "running costs".

The header stat "Total cost" is unchanged: it is part of the synced object record, and it stays the
object's own running cost.

## API

`GET /objects/{id}/insights?contents=true` -- `contents` optional, default false. Existing fields
keep their meaning when it is absent, so current clients see no change.

New fields:

```json
{
  "has_contents": true,
  "ownership": { "total_cents": 400000, "purchase_cents": 300000, "since": "2024-05-01",
                 "per_year_cents": 130000 },
  "by_month": [{ "bucket": "2025-10", "cost_cents": 0 }],
  "fuel": { "...existing fields": "...", "fills": [{ "date": "2026-01-10", "per_100_milli": 5000 }] }
}
```

- `ownership.purchase_cents` is the purchase price actually counted (0 when skipped or absent),
  including descendants' with `contents=true`. `per_year_cents` is null under 90 days owned.
- `by_month` always has twelve entries.
- `fuel.fills` has at most twelve entries; `fuel` itself stays null for an object with no fills.
- With `contents=true`, `by_year`, `by_category`, `by_month` and `ownership` include descendants.
  Everything else ignores the parameter.
- A non-boolean `contents`: 400 (axum `Query` rejection).

Descendants are the object's non-deleted children, their children, and so on. The objects API
already refuses cycles.

## Backend structure

- `src/domain/stats.rs` gains, pure and unit-tested:
  - `months_ending(today: NaiveDate, months: u32, spend) -> Vec<Amount>` -- the twelve-month
    window with zeros, sharing the month-filling code `summarize` uses for one year.
  - `days_owned(since, until) -> i64` and `per_year_cents(total, days) -> Option<i64>`
    (None under 90 days; integer arithmetic, `total * 365 / days`).
- `src/domain/insights.rs` gains `consumption_per_fill(fills: &[DatedFill]) -> Vec<FillRate>`,
  pure and unit-tested, next to the existing tank-method consumption.
- `src/api/insights.rs`: parses `contents`; when true, resolves the descendant ids once (one query
  for the user's non-deleted objects' `id, parent_id`, walked in Rust) and runs the cost queries
  with `object_id IN (...)` over that set; purchase prices come from `domain::stats::purchase_spend`
  with the same purchased-set query phase 1 uses. `by_year`/`by_category` SQL is unchanged apart
  from the id set. Every `SUM` is `CAST(... AS BIGINT)`.
- `docs/openapi.json`: the parameter and new fields.

## Frontend structure

- `lib/Insights.svelte`: the switch, the ownership line, and the new bar blocks through `BarList`.
  It already clears stale data before each fetch; the switch becomes part of that fetch's key.
- `lib/insights.ts` (new, pure), with `tests/insights.test.ts`:
  - `insightsPath(objectId, contents)` -- `/objects/{id}/insights`, plus `?contents=true` only
    when on.
  - `sinceLabel(date, locale)` -- "May 2024" from `YYYY-MM-DD`.
  - `fillLabel(date, locale)` -- a short day and month ("10 Jan") for a fill bar.
  Month bars reuse `periodLabel` from `lib/stats.ts`.
- `ObjectDetail.svelte` passes whether the object has children (it already loads them for the Info
  tab) so the switch can hide without a request.
- en/de strings for every new label.

## Errors

As today: a failed fetch shows its message in place of the Cost block. The switch stays usable so
a retry is one tap.

## Tests

- **Rust unit:** consumption per fill (first fill has no bar, zero/negative distance skipped, twelve
  most recent kept, out-of-order input sorted); twelve-month window across a year boundary with
  zeros; days owned to archive date vs today; per year omitted under 90 days and exact above.
- **Rust integration (`tests/insights.rs`, SQLite and PostgreSQL):** house with a boiler and a
  grandchild bulb, each with costs and a purchase price; `contents` absent returns the house's own
  figures exactly as before; `contents=true` adds both descendants to ownership, by_year,
  by_category and by_month but not to counter or fuel figures; a costed purchase entry suppresses
  that object's price; another user's object is 404 either way; `contents=maybe` is 400.
- **Vitest:** `insights.ts` helpers.
- **Playwright (`21-object-cost-depth.spec.ts`, `signInFresh`):** the switch is absent on a car
  and present on a house with a boiler; flipping it changes the ownership total; the choice
  survives a reload; a car with three fills shows two fill bars.

## Out of scope

Changing the header "Total cost" stat, putting the Statistics year in the URL, centralising the
checkbox style into `app.css`, per-fill partial-tank correction.
