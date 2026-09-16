# Charges and energy cost

Status: implemented. Builds on the trip log (0.10.0): what a charge (or fill) gives
you, what a trip costs in energy, and when to charge next.

## Data

**Charges stay `fuel` entries** (category `fuel`, labelled "Fuel / charge"): they already carry
`counter_value`, `quantity_milli` (kWh, l, gal) and `cost_cents`. One new column on `activities`:

| column | type | rule |
|---|---|---|
| `charged_full` | INTEGER NOT NULL DEFAULT 0 | 1 only on a `fuel` entry; 400 on any other category |

One new column on `objects`:

| column | type | rule |
|---|---|---|
| `energy_price_milli` | INTEGER, nullable | cents per unit × 1000 (0.30 €/kWh → 30000); `>= 0`; only when `fuel_unit` is set (400 otherwise) |

Both travel like every other field: REST create/update/read/list, the sync whitelist
(`charged_full`, `energy_price_milli`, both Integer) with field clocks and the same cross-field
rules on a pushed `set`, the offline outbox, export/import (`#[serde(default)]`, older archives
read as `0`/null), the DB switch copy and `docs/openapi.json`.

Migrations: SQLite `0016_energy.sql`, PostgreSQL `0007_energy.sql`; `schema_parity` stays green.

## Logging a charge

- **"+ Log charge" / "+ Laden eintragen"** on objects with a `fuel_unit`, beside "+ Log trip"; it
  opens the entry form with category `fuel`.
- Fields: date, counter (prefilled with the object's current counter on a new charge), **charged
  full** (ticked by default), amount (optional), cost (optional), notes, tags, files.
- Amount and cost stay optional: a charge with neither still marks distance, which is what the
  per-charge figures need.
- Timeline row: title (or "Charged" / "Geladen"), date, counter, "full" when ticked, amount and
  cost when given — e.g. `16.09.2026 · 3.420 km · full · 8,5 kWh · 2,55 €`.
- "Last done" excludes `fuel` entries too, the same way it already excludes `trip` (see the trip
  log spec): a charge is something logged at the current counter, not something "done", and the
  Energy section below is where its own figures belong -- a charge would otherwise show the
  meaningless "0 km ago" every single time.

## Figures (object Info tab, section "Energy" / "Energie")

All computed from charges of the object that carry a `counter_value`, ordered by date (ties
broken by counter) rather than by counter itself, so a replaced or reset counter cannot be read
as one huge -- or negative -- window; a window is the distance between two consecutive **full**
charges, or between consecutive charges when fewer than two charges are marked full (so older
data, and the first full charge after adopting the habit, still yield figures). A window whose
distance is zero or negative -- a duplicate counter reading, or a counter that went backwards --
is dropped before any figure is computed from it.

- **Distance per charge** — mean window distance. Shown when at least two qualifying charges exist.
- **Distance per unit** — window distance ÷ the closing charge's `quantity_milli`, averaged over
  windows whose closing charge has an amount ("≈ 7,3 km/kWh").
- **Energy cost per distance** — the closing charge's known cost ÷ window distance, averaged over
  windows where the cost is known; known cost = `cost_cents`, else `quantity_milli × energy_price`
  when both exist. Null when no window qualifies.
- **Charge due** — `used` = sum of `battery_used_pct` over trips dated after the last full charge,
  or dated the same day and starting at or after its counter (a trip on the charging day itself
  still counts, unless it ran before the charge was plugged in);
  `remaining = max(0, 100 - used)`; `km_per_pct` = distance ÷ battery percent summed over trips
  that carry both; `range_left = remaining × km_per_pct`. Shown as "≈ 35 % · ≈ 40 km"; at
  `remaining <= 20` the line adds "charge soon" / "bald laden". Hidden when no trip carries a
  battery percentage or no full charge exists yet.

Endpoint `GET /objects/{id}/energy` (owner only, 404 otherwise) →
`{ unit, price_milli, distance_per_charge, distance_per_unit_milli, cost_per_counter_milli,
battery: { remaining_pct, range_left, warn } | null }`, integers, null where not computable. The
maths lives in `src/domain/energy.rs` with its own unit tests; the endpoint only loads rows.

## Cost per trip

A trip row and the trip detail show `≈ €1,80` when `cost_per_counter_milli` exists: trip distance ×
that rate, always prefixed "≈" and never stored. The Info tab's "Trips" table gains an "Energy
cost" row per period (this month / this year / all time), computed the same way from each period's
distance. Nothing is shown when the rate is null.

## Wording by unit

For `fuel_unit = kwh` the existing fuel strings read wrong in German. Labels become, for kWh:
"Charged" / "Geladen", "Energy per distance" / "Verbrauch", "Energy cost per {unit}" /
"Stromkosten pro {unit}". For `l`/`gal` the current wording stays ("Getankt", "Spritkosten pro
{unit}"). One helper picks the key from the object's unit, used by both the Insights block and the
new Energy section.

## Price per unit

Settings for it live on the object form, next to the fuel unit: "Price per kWh" / "Preis pro kWh"
(the unit name follows `fuel_unit`), empty allowed. Parsed like other money input (comma or dot).

## Out of scope

Push notifications for charging, charging curves or per-session power, several batteries, prices
per charger or per session, tariffs by time of day.
