# Trip log (Fahrtenbuch)

Status: approved, not implemented. A personal trip log: how far, where, how long and how much
battery. Not a tax log (no locking, no business/private split, no GPS tracks, no drivers).

## Data

A trip is an activity with the new category `trip` (en "Trip", de "Fahrt"). Activities gain five
nullable columns, used only by trips:

| column | type | rule |
|---|---|---|
| `start_counter` | INTEGER | required on a trip, `>= 0`, `<= counter_value` |
| `from_place` | TEXT | optional, trimmed, max 80 characters, empty → null |
| `to_place` | TEXT | optional, trimmed, max 80 characters, empty → null |
| `duration_minutes` | INTEGER | optional, 1–10080 |
| `battery_used_pct` | INTEGER | optional, 0–100 |

The trip's end is the existing `counter_value`, so a trip moves the object's current counter
(`MAX(counter_value)`) and "last done", counter reminders and usage per month keep working
unchanged. A trip needs `counter_value` and `start_counter`; a trip is allowed only on an object whose
`counter_unit` is `km` or `mi` (400 otherwise). On any other category the five fields must be absent
or null (400 otherwise). Distance = `counter_value - start_counter`, never stored.

Migrations: SQLite `0015_trips.sql`, PostgreSQL `0006_trips.sql` (add columns; no data change).
`schema_parity` stays green. The `trip` category is accepted for every object type with a distance
counter, independent of the type's category list; the category picker offers it only there.

Everywhere activities travel, the five fields travel too: REST create/update/read/list, sync
whitelist (`start_counter`, `duration_minutes`, `battery_used_pct` Integer; `from_place`, `to_place`
Text) with field clocks, sync validation of a pushed `set` (same rules, checked against the row's
other current values), offline outbox bodies, export/import (`#[serde(default)]` so older archives
import), the DB switch copy, `docs/openapi.json`. Search matches `from_place` and `to_place` like
title and notes.

## Entering a trip

- Object page: "+ Log trip" / "+ Fahrt eintragen" next to "+ Log activity" on objects with a km/mi
  counter; it opens the entry form with category `trip`.
- Trip form fields: date (DateInput), title (optional; defaults to `$t('cat.trip')` when empty),
  start (prefilled with the object's current counter on a new trip, editable), end, distance,
  from, to, duration, battery used, notes, tags, photos/files (existing attachment flow).
- End and distance are linked: typing the end fills distance = end − start; typing the distance
  fills end = start + distance; changing the start keeps the distance and moves the end. Only
  end/start are sent.
- From / To suggest earlier places of this object (`GET /objects/{id}/trip-places` →
  `{ from: string[], to: string[] }`, most recent first, distinct, max 20 each). Title suggestions
  reuse the existing recent titles filtered to category `trip`.
- Duration input accepts `h:mm` or minutes (`1:15`, `75`); shown as `1:15 h`.
- Validation messages (en/de): end below start; missing start or end; battery outside 0–100;
  duration outside range; place too long.

## Timeline

A trip row: title (or "Trip"), date, `400 → 600 km · 200 km`, `Home → Office` when either place is
set (`→ Office` / `Home →` for one), `1:15 h · 32 %` when given, notes, tags and thumbnails as for
any entry. Trips are not folded into reading runs. The category chips gain "Trips" when the object
has a distance counter.

## Totals

**Info tab "Trips" / "Fahrten"** (hidden without trips): for this month, this year and all time —
number of trips, total distance, average distance per trip; average speed (distance / duration,
over trips that have a duration) and distance per 10 % battery (over trips that have battery), each
shown only when at least one trip carries the value.

Endpoint `GET /objects/{id}/trips/summary?today=YYYY-MM-DD` (owner only) →
`{ month, year, all }` each `{ trips, distance, avg_distance, speed_kmh_x10, distance_per_10pct }`
(integers; `null` where not computable). Periods use the trip `date`; `today` defaults to the
server date in the configured timezone. Portable SQL or Rust aggregation, tested on both backends.

**Statistics:** the object's Insights section shows "Trip distance per month" for the last twelve
months (bars, like usage per month) from `GET /objects/{id}/insights` gaining
`trip_distance_by_month: [{ month: 'YYYY-MM', distance }]` (all twelve months, zero when none).

## Out of scope

Tax rules, business/private split, GPS/GPX import, drivers, multi-leg trips, trips on objects
without a distance counter.
