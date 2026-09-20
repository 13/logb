-- no-transaction
-- Resource metadata makes electricity, heating fuel and water first-class capabilities instead
-- of inferring all three from the legacy `fuel_unit` name. The old column stays in place for
-- API/export compatibility; new code treats it as the resource's measurement unit.
ALTER TABLE objects ADD COLUMN resource_kind TEXT CHECK (resource_kind IN ('electricity','heating_fuel','vehicle_fuel','water'));
ALTER TABLE objects ADD COLUMN resource_unit TEXT CHECK (resource_unit IN ('l','gal','kwh','m3'));
ALTER TABLE objects ADD COLUMN measurement_mode TEXT CHECK (measurement_mode IN ('usage','meter'));
ALTER TABLE objects ADD COLUMN monthly_target_milli INTEGER CHECK (monthly_target_milli > 0);
ALTER TABLE objects ADD COLUMN low_level_pct INTEGER CHECK (low_level_pct BETWEEN 0 AND 100);
ALTER TABLE objects ADD COLUMN private INTEGER NOT NULL DEFAULT 0;

UPDATE objects SET resource_kind = CASE
  WHEN fuel_unit = 'kwh' THEN 'electricity'
  WHEN fuel_unit IN ('l','gal') AND type = 'home' THEN 'heating_fuel'
  WHEN fuel_unit IN ('l','gal') THEN 'vehicle_fuel'
END, resource_unit = fuel_unit,
measurement_mode = CASE WHEN fuel_unit IS NOT NULL THEN 'usage' END;

-- SQLite cannot widen the activity category CHECK in place. Rebuild once and add the water
-- reading/billing fields at the same time.
PRAGMA foreign_keys = off;
BEGIN;
CREATE TABLE activities_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN
      ('maintenance','repair','purchase','inspection','modification','fuel','usage','other','symptom','treatment','appointment','medication','reading','trip','weight','session')),
    title TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '', counter_value INTEGER, cost_cents INTEGER,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL, quantity_milli INTEGER, client_op_id TEXT,
    client_uuid TEXT, deleted_at TEXT, tags TEXT NOT NULL DEFAULT '[]', start_counter INTEGER,
    from_place TEXT, to_place TEXT, duration_minutes INTEGER, battery_used_pct INTEGER,
    charged_full INTEGER NOT NULL DEFAULT 0, weight_grams INTEGER CHECK (weight_grams > 0),
    fuel_level_pct INTEGER CHECK (fuel_level_pct BETWEEN 0 AND 100),
    meter_reading_milli INTEGER CHECK (meter_reading_milli >= 0), period_start TEXT, period_end TEXT,
    estimated INTEGER NOT NULL DEFAULT 0, meter_reset INTEGER NOT NULL DEFAULT 0
);
INSERT INTO activities_new (id, object_id, date, category, title, notes, counter_value, cost_cents,
  created_at, updated_at, quantity_milli, client_op_id, client_uuid, deleted_at, tags, start_counter,
  from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams, fuel_level_pct)
SELECT id, object_id, date, category, title, notes, counter_value, cost_cents,
  created_at, updated_at, quantity_milli, client_op_id, client_uuid, deleted_at, tags, start_counter,
  from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams, fuel_level_pct
FROM activities;
DROP TABLE activities;
ALTER TABLE activities_new RENAME TO activities;
CREATE INDEX idx_activities_object_date ON activities(object_id, date);
CREATE UNIQUE INDEX idx_activities_client_op ON activities(client_op_id) WHERE client_op_id IS NOT NULL;
CREATE UNIQUE INDEX idx_activities_uuid ON activities(client_uuid);
CREATE INDEX idx_activities_weight ON activities(object_id, date DESC, created_at DESC, id DESC) WHERE deleted_at IS NULL AND weight_grams IS NOT NULL;
CREATE INDEX idx_activities_meter ON activities(object_id, date DESC, id DESC) WHERE deleted_at IS NULL AND meter_reading_milli IS NOT NULL;
COMMIT;
PRAGMA foreign_keys = on;
