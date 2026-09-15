-- Trip log: a trip is an entry with the new category `trip`. Its end is the existing
-- `counter_value`, so the object's current counter, counter reminders and usage per month keep
-- working unchanged. The trip-only details are five nullable columns here, not a table, so they
-- travel through sync, export, offline edits and attachments like `notes` does.
--
-- `BIGINT`, matching every other integer column on `activities` (counter_value, cost_cents,
-- quantity_milli) in migrations/postgres/0001_schema.sql.
ALTER TABLE activities ADD COLUMN start_counter BIGINT;
ALTER TABLE activities ADD COLUMN from_place TEXT;
ALTER TABLE activities ADD COLUMN to_place TEXT;
ALTER TABLE activities ADD COLUMN duration_minutes BIGINT;
ALTER TABLE activities ADD COLUMN battery_used_pct BIGINT;

-- An inline column CHECK is named `<table>_<column>_check`.
ALTER TABLE activities DROP CONSTRAINT activities_category_check;
ALTER TABLE activities ADD CONSTRAINT activities_category_check CHECK (category IN
    ('maintenance','repair','purchase','inspection','modification','fuel','other',
     'symptom','treatment','appointment','medication','reading','trip'));
