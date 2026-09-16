# Charges and Energy Cost Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Log a charge (or fill) with an optional amount and cost, see distance and cost per charge, an estimated energy cost per trip, and when to charge next.

**Architecture:** A charge stays a `fuel` activity; one new column marks it as full, one new column on the object holds a price per unit. The window maths lives in a pure `src/domain/energy.rs`, served by one new endpoint; the frontend adds a charge form, an Energy section and an estimate on trip rows.

**Tech Stack:** Rust (axum, sqlx `Any` on SQLite + PostgreSQL), Svelte 5 runes, TypeScript, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-16-charging-energy-design.md`

## Global Constraints

- Branch `build-charging-energy`; never commit to `main`. Before every commit `git branch --show-current` must print `build-charging-energy`. Stage only files the task changes.
- Portable SQL (SQLite and PostgreSQL); `CAST(... AS BIGINT)` for aggregates. PostgreSQL runs use a throwaway `postgres:17` container on port 55448 (`LOGB_TEST_DATABASE_URL=postgres://postgres:pg@127.0.0.1:55448/postgres`), always stopped afterwards.
- New columns exactly: `activities.charged_full` INTEGER NOT NULL DEFAULT 0 (1 only on a `fuel` entry), `objects.energy_price_milli` nullable integer (cents per unit × 1000, `>= 0`, only when `fuel_unit` is set). PostgreSQL uses the same integer type as its neighbouring columns (`BIGINT`).
- Error messages exactly: `"only a charge can be marked full"`, `"energy_price_milli must be >= 0"`, `"energy_price_milli needs a fuel unit"`.
- Money stays cents; quantities stay milli-units; rates stay "cents × 1000" like `cost_per_counter_milli`.
- Every UI string in en and de; dates via `fmtDate(iso, $dateFormat)`; date fields are `DateInput`.
- Playwright with project defaults (never `--workers`); run targeted specs only, one invocation at a time (the machine is RAM-tight). No parallel cargo and Playwright runs.
- Foreground commands (timeouts up to 600000 ms); never end a turn while one runs. Never weaken an existing assertion. Comments explain *why*. Stop and report if `df -h /home` exceeds 90%.
- Commits end with exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DNUZLftSTtND7ABvN6eoGv
  ```

---

### Task 1: Charge data — full flag, price per unit, validation, sync, export

**Files:**
- Create: `migrations/sqlite/0016_energy.sql`, `migrations/postgres/0007_energy.sql`
- Modify: `src/api/activities.rs` (`ActivityRow`, `ActivityInput`, `ActivityOut`, `validate`, every activities SELECT/INSERT/UPDATE), `src/api/objects.rs` (`ObjectRow`, `ObjectInput`, `ObjectOut`, `validate`, SELECT/INSERT/UPDATE, the change log in `update`), `src/sync/mod.rs` (whitelist), `src/sync/apply.rs` (`validate_value` + set-op cross-field checks), `src/api/export.rs` (`ActivityExport`, `ObjectExport`, export query, import insert), `docs/openapi.json`
- Modify: `frontend/src/lib/types.ts` (`Activity`/`ActivityInput` gain `charged_full?: number`, `MemObject`/`ObjectInput` gain `energy_price_milli?: number | null`)
- Test: create `tests/charging.rs`; extend `tests/sync.rs`, `tests/export.rs`, `tests/objects.rs`; `tests/schema_parity.rs` stays green

**Interfaces:**
- Produces: JSON field `charged_full: number` (0/1) on every activity response and accepted on create/PATCH; `energy_price_milli: number | null` on every object response and accepted on create/PATCH; sync whitelist entries `(Entity::Activity, "charged_full", Integer)` and `(Entity::Object, "energy_price_milli", Integer)`.

- [ ] **Step 1: Migrations.**

```sql
-- A charge that filled the battery (or tank) closes a window: the distance since the previous
-- full charge is what one charge carries, and the amount put in is what that distance cost.
-- A flag on the entry rather than a table: it travels through sync, export and offline edits
-- like every other field of an entry.
ALTER TABLE activities ADD COLUMN charged_full INTEGER NOT NULL DEFAULT 0;
-- Cents per unit times 1000 (0.30 EUR/kWh -> 30000), the same scale as `cost_per_counter_milli`,
-- so a charge with no cost of its own can still be priced.
ALTER TABLE objects ADD COLUMN energy_price_milli INTEGER;
```
PostgreSQL: same statements with the integer type its neighbours use (`BIGINT`).

- [ ] **Step 2: Failing tests** `tests/charging.rs` (follow `tests/trips.rs` for helpers):
  - object with `counter_unit: "km"`, `fuel_unit: "kwh"`, `energy_price_milli: 30000` → create, read and list return the price; PATCH to `null` clears it.
  - `energy_price_milli: -1` → 400 `"energy_price_milli must be >= 0"`; a price on an object without `fuel_unit` → 400 `"energy_price_milli needs a fuel unit"`; clearing `fuel_unit` while a price is set → 400 with the same message (unless the price is nulled in the same body).
  - POST activity `{ category: "fuel", charged_full: 1, counter_value: 3420, quantity_milli: 8500, cost_cents: 255 }` → 201, response has `charged_full: 1`; a `fuel` entry without the flag reads `charged_full: 0`.
  - `charged_full: 1` on a `maintenance` entry → 400 `"only a charge can be marked full"`; PATCH switching a full charge to `maintenance` → 400 unless the body also sets `charged_full: 0`.
  - PATCH that omits `charged_full` keeps the stored value.
  Run `cargo test --test charging` → FAIL.

- [ ] **Step 3: Implement REST.** Add the fields to the row/input/output structs and to every column list in `activities.rs` and `objects.rs`. `ActivityInput::validate`: `charged_full` is 0 or 1, and 1 only when `category == "fuel"`. `ObjectInput::validate`: price `>= 0` and only with a `fuel_unit`. Absent keys on PATCH keep stored values (follow how `tags` and the trip fields do it). The object `update` change log records `energy_price_milli` like the other fields.

- [ ] **Step 4: Sync.** Whitelist both fields. In `apply.rs`, a pushed `set` validates shape (`validate_value`: 0/1 for `charged_full`, `>= 0` for the price) and the cross-field rules against the stored row: `charged_full = 1` needs the stored category to be `fuel`; `set category` away from `fuel` needs the stored `charged_full` to be 0; `energy_price_milli` needs the object's stored `fuel_unit` to be set; `set fuel_unit = null` is rejected while a price is stored. Tests in `tests/sync.rs` for each, including one accepted case per field.

- [ ] **Step 5: Export/import.** `ActivityExport` gains `#[serde(default)] charged_full: i64`, `ObjectExport` gains `#[serde(default)] energy_price_milli: Option<i64>`; export selects them, import binds them (validated like REST). Test in `tests/export.rs`: round trip keeps both; an archive without the keys imports as `0` / `null`.

- [ ] **Step 6: OpenAPI + TS types.** Document both fields (activity and object schemas); `tests/openapi.rs` passes; `frontend` types updated; `npm run check` clean.

- [ ] **Step 7: Verify.** `cargo test --test charging --test activities --test objects --test sync --test export --test openapi --test schema_parity` on SQLite and PostgreSQL (port 55448); `cargo clippy --all-targets -- -D warnings`; `cd frontend && npm run check && npx vitest run`.

- [ ] **Step 8: Commit** — `feat: a charge can be marked full, and an object can carry a price per unit`.

---

### Task 2: Energy figures — domain maths and endpoint

**Files:**
- Create: `src/domain/energy.rs` (registered in `src/domain/mod.rs`), `src/api/energy.rs` (router registered like `src/api/trips.rs`)
- Modify: `docs/openapi.json`, `frontend/src/lib/types.ts`
- Test: unit tests inside `src/domain/energy.rs`; extend `tests/charging.rs`

**Interfaces:**
- Consumes: Task 1's `charged_full`, `energy_price_milli`; trip fields `start_counter`, `battery_used_pct` from the trip log.
- Produces:
```rust
pub struct Charge { pub date: String, pub counter: i64, pub quantity_milli: Option<i64>, pub cost_cents: Option<i64>, pub full: bool }
pub struct Trip { pub date: String, pub distance: i64, pub battery_used_pct: Option<i64> }
pub struct Energy { pub distance_per_charge: Option<i64>, pub distance_per_unit_milli: Option<i64>, pub cost_per_counter_milli: Option<i64>, pub battery: Option<Battery> }
pub struct Battery { pub remaining_pct: i64, pub range_left: Option<i64>, pub warn: bool }
pub fn energy(charges: &[Charge], trips: &[Trip], price_milli: Option<i64>) -> Energy
```
  and `GET /objects/{id}/energy` → `{ unit: string|null, price_milli: number|null, distance_per_charge, distance_per_unit_milli, cost_per_counter_milli, battery: { remaining_pct, range_left, warn } | null }`. TS interfaces `EnergyOut`, `EnergyBattery`.

- [ ] **Step 1: Failing unit tests** in `src/domain/energy.rs`. Windows: charges sorted by counter; when any charge is `full`, windows run from one full charge to the next full charge; otherwise between consecutive charges. Cases:
  - charges (full at 1000, full at 1400 with 8000 milli-units and 240 cents, full at 1800 with 8000 and 260) → `distance_per_charge = 400`; `distance_per_unit_milli` = mean of `400*1000/8000` twice = 50 (0.05 distance per unit × 1000 — keep the milli scale and assert the exact integers your formula produces, documented in a comment); `cost_per_counter_milli` = mean of `240*1000/400` and `260*1000/400` = 625.
  - no charge marked full → windows between consecutive charges; same fields computed.
  - a closing charge without an amount is skipped for `distance_per_unit_milli` but still counts for `distance_per_charge`.
  - a closing charge without a cost and no price → skipped for `cost_per_counter_milli`; with `price_milli = 30000` and 8000 milli-units → cost = `8000 * 30000 / 1000 / 1000` cents; assert the exact value your formula yields and comment the scale.
  - fewer than two charges → all `None`.
  - battery: last full charge on 2026-09-01; trips after it with `battery_used_pct` 30 and 25, distances 60 and 50 → `remaining_pct = 45`; `km_per_pct` from all trips carrying both (110 / 55 = 2) → `range_left = 90`; `warn = false`; with used 85 → `remaining 15`, `warn true`; used 120 → `remaining 0`, `warn true`.
  - no full charge, or no trip with a battery percentage → `battery: None`.
  Run `cargo test --lib energy` → FAIL, then implement.

- [ ] **Step 2: Failing API tests** in `tests/charging.rs`: seed an object (km, kwh, price), charges and trips as above → `GET /objects/{id}/energy` returns the same numbers, `unit: "kwh"`; another user's object → 404; an object with no charges → every figure null and `battery: null`. Deleted charges and deleted trips are ignored.

- [ ] **Step 3: Implement the endpoint** with `load_owned_object`; load non-deleted `fuel` entries with a `counter_value` (oldest first) and non-deleted `trip` entries (with `start_counter`, `counter_value`, `battery_used_pct`, `date`), map to the domain types, return `energy(...)`. No SQL aggregation — the maths lives in the domain.

- [ ] **Step 4: OpenAPI + TS.** Document the endpoint and schemas; `tests/openapi.rs` passes; `npm run check` clean.

- [ ] **Step 5: Verify.** `cargo test --lib energy`, `cargo test --test charging --test openapi` on SQLite and PostgreSQL; clippy.

- [ ] **Step 6: Commit** — `feat: distance and cost per charge, and when to charge next`.

---

### Task 3: Charge form, Energy section and trip estimates

**Files:**
- Create: `frontend/src/lib/EnergyFigures.svelte`, `frontend/tests/energy.test.ts`, `frontend/tests-e2e/29-charging.spec.ts`
- Modify: `frontend/src/lib/trip.ts` or a new `frontend/src/lib/energy.ts` (formatting helpers), `frontend/src/routes/ActivityForm.svelte`, `frontend/src/routes/ObjectForm.svelte` (price field), `frontend/src/routes/ObjectDetail.svelte` (Energy section, "+ Log charge"), `frontend/src/lib/Timeline.svelte` (charge row, trip estimate), `frontend/src/lib/TripTotals.svelte` (energy cost row), `frontend/src/lib/Insights.svelte` (wording by unit), `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/energy.test.ts`, extend `frontend/tests-e2e/28-trips.spec.ts` only if needed

**Interfaces:**
- Consumes: Task 2's `GET /objects/{id}/energy` (`EnergyOut`), `fuel_unit`, `energy_price_milli`.
- Produces: `formatPerUnit(milli, unit, counterUnit, locale)` → "7,3 km/kWh"; `energyLabelKey(fuelUnit)` → the i18n key stem (`energy.charged` vs `energy.filled`), used by the Energy section, the timeline row and Insights.

- [ ] **Step 1: Failing Vitest** `frontend/tests/energy.test.ts` for the two helpers (exact strings for en and de, kWh vs litres), plus a case that `energyLabelKey(null)` falls back to the fill wording.

- [ ] **Step 2: Charge form.** In `ActivityForm.svelte`, when `input.category === 'fuel'`: a "Charged full" / "Voll geladen" checkbox (ticked by default on a new entry), the existing amount field, cost, and the counter prefilled with the object's current counter on a new entry. `buildInput()` sends `charged_full: input.category === 'fuel' ? (chargedFull ? 1 : 0) : 0`, mirroring how `quantity_milli` is cleared for other categories.

- [ ] **Step 3: "+ Log charge".** On objects with a `fuel_unit`, a button beside "+ Log trip" (same `.fab-secondary` style) goes to `/objects/${oid}/activities/new?category=fuel`. Strings: en `energy.log` "Log charge" (for kWh) / "Log fill" (for l/gal) via `energyLabelKey`; de "Laden eintragen" / "Tanken eintragen".

- [ ] **Step 4: Timeline.** A `fuel` row shows, after the date and counter: "full" / "voll" when `charged_full`, the amount, and the cost. A `trip` row appends `≈ {money}` when the object's energy rate is known — pass the rate into `Timeline` from `ObjectDetail` (loaded with the Energy section) rather than fetching per row; nothing is shown when it is null.

- [ ] **Step 5: Energy section.** `EnergyFigures.svelte` on the Info tab, loaded when the tab opens with an oid sequence guard (copy `loadTripSummary`), hidden when every figure is null. Heading `energy.title` ("Energy" / "Energie"). Rows: distance per charge, distance per unit, energy cost per distance (all only when not null), and the battery line "≈ {pct} % · ≈ {range}" with `energy.charge-soon` ("charge soon" / "bald laden") appended when `warn`. `TripTotals.svelte` gains an "Energy cost" / "Energiekosten" row per period, computed as `distance × cost_per_counter_milli` (hidden when the rate is null).

- [ ] **Step 6: Price on the object form.** Next to the fuel unit, "Price per {unit}" / "Preis pro {unit}", parsed with the existing money parser, sent as `energy_price_milli` = cents × 1000, empty → null. Show it only when a `fuel_unit` is chosen.

- [ ] **Step 7: Wording by unit.** `Insights.svelte`'s fuel block uses `energyLabelKey`, so a kWh object reads "Geladen" and "Stromkosten pro km" while petrol keeps "Getankt" and "Spritkosten pro km".

- [ ] **Step 8: E2E** `29-charging.spec.ts`: e-bike (km, kWh, price 0.30); log a full charge at 1000 km with no amount; a trip 1000 → 1200 using 40 % battery; a full charge at 1400 with 8 kWh and €2.40; the Info tab's Energy section shows distance per charge, km/kWh and cost per km; the trip row shows an "≈" cost; the battery line appears after a trip following the last full charge; a second trip pushing the battery past 80 % adds "charge soon". Keep the clock pinned.

- [ ] **Step 9: Verify.** `cd frontend && npm run check && npx vitest run`; `npm run e2e -- 29-charging 28-trips 21-object-cost-depth`; a 390px screenshot of the Info tab with the Energy section saved to `/home/ben/repo/logb/.superpowers/sdd/energy-info-390.png` and looked at.

- [ ] **Step 10: Commit** — `feat: log charges, see energy figures, and what a trip costs`.
