ALTER TABLE objects ADD COLUMN fuel_capacity_milli INTEGER CHECK (fuel_capacity_milli > 0);
ALTER TABLE activities ADD COLUMN fuel_level_pct INTEGER CHECK (fuel_level_pct BETWEEN 0 AND 100);
