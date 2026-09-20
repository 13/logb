ALTER TABLE objects ADD COLUMN resource_kind TEXT CHECK (resource_kind IN ('electricity','heating_fuel','vehicle_fuel','water'));
ALTER TABLE objects ADD COLUMN resource_unit TEXT CHECK (resource_unit IN ('l','gal','kwh','m3'));
ALTER TABLE objects ADD COLUMN measurement_mode TEXT CHECK (measurement_mode IN ('usage','meter'));
ALTER TABLE objects ADD COLUMN monthly_target_milli BIGINT CHECK (monthly_target_milli > 0);
ALTER TABLE objects ADD COLUMN low_level_pct BIGINT CHECK (low_level_pct BETWEEN 0 AND 100);
ALTER TABLE objects ADD COLUMN private BIGINT NOT NULL DEFAULT 0;
UPDATE objects SET resource_kind = CASE
  WHEN fuel_unit = 'kwh' THEN 'electricity'
  WHEN fuel_unit IN ('l','gal') AND type = 'home' THEN 'heating_fuel'
  WHEN fuel_unit IN ('l','gal') THEN 'vehicle_fuel'
END, resource_unit = fuel_unit,
measurement_mode = CASE WHEN fuel_unit IS NOT NULL THEN 'usage' END;

ALTER TABLE activities DROP CONSTRAINT activities_category_check;
ALTER TABLE activities ADD CONSTRAINT activities_category_check CHECK (category IN
('maintenance','repair','purchase','inspection','modification','fuel','usage','other','symptom','treatment','appointment','medication','reading','trip','weight','session'));
ALTER TABLE activities ADD COLUMN meter_reading_milli BIGINT CHECK (meter_reading_milli >= 0);
ALTER TABLE activities ADD COLUMN period_start TEXT;
ALTER TABLE activities ADD COLUMN period_end TEXT;
ALTER TABLE activities ADD COLUMN estimated BIGINT NOT NULL DEFAULT 0;
ALTER TABLE activities ADD COLUMN meter_reset BIGINT NOT NULL DEFAULT 0;
CREATE INDEX idx_activities_meter ON activities(object_id, date DESC, id DESC) WHERE deleted_at IS NULL AND meter_reading_milli IS NOT NULL;
