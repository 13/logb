-- Reading reminders. A step of its own rather than an edit to 0001: PostgreSQL databases created
-- by a released build have already applied 0001, and sqlx refuses to start against a migration
-- whose checksum changed underneath it. SQLite's 0011 makes the same change.

-- An inline column CHECK is named `<table>_<column>_check`.
ALTER TABLE activities DROP CONSTRAINT activities_category_check;
ALTER TABLE activities ADD CONSTRAINT activities_category_check CHECK (category IN
    ('maintenance','repair','purchase','inspection','modification','fuel','other',
     'symptom','treatment','appointment','medication','reading'));

ALTER TABLE reminders ADD COLUMN kind TEXT NOT NULL DEFAULT 'service' CHECK (kind IN ('service', 'reading'));
ALTER TABLE reminders ADD COLUMN every_n BIGINT;
ALTER TABLE reminders ADD COLUMN every_unit TEXT CHECK (every_unit IN ('week', 'month'));
