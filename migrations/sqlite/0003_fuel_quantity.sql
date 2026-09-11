-- Fuel amount, scaled by 1000 like cost is scaled by 100: the database stores no floats.
ALTER TABLE activities ADD COLUMN quantity_milli INTEGER;

-- Which unit that quantity is in. NULL derives from counter_unit (km -> l, mi -> gal, h -> l);
-- explicit so an e-bike in the same instance as a petrol car can record kwh.
ALTER TABLE objects ADD COLUMN fuel_unit TEXT CHECK (fuel_unit IN ('l', 'gal', 'kwh'));
