-- A charge that filled the battery (or tank) closes a window: the distance since the previous
-- full charge is what one charge carries, and the amount put in is what that distance cost.
-- A flag on the entry rather than a table: it travels through sync, export and offline edits
-- like every other field of an entry.
--
-- `BIGINT`, matching every other integer column on `activities`/`objects` in
-- migrations/postgres/0001_schema.sql.
ALTER TABLE activities ADD COLUMN charged_full BIGINT NOT NULL DEFAULT 0;
-- Cents per unit times 1000 (0.30 EUR/kWh -> 30000), the same scale as `cost_per_counter_milli`,
-- so a charge with no cost of its own can still be priced.
ALTER TABLE objects ADD COLUMN energy_price_milli BIGINT;
