-- no-transaction
--
-- Reading reminders: "log the odometer every month". Two changes, for the reasons 0009 already
-- spells out in full -- foreign keys must be off for the rebuild, the `-- no-transaction` line
-- must stay the FIRST line, and each rebuild creates the new table, copies, drops the old and
-- renames the new one into place.
--
-- 1. activities.category grows `reading`: an entry that is nothing but a counter value. SQLite
--    cannot alter a CHECK in place, so the table is rebuilt, keeping its live column order.
-- 2. reminders grows `kind`, `every_n` and `every_unit`. A reading reminder is due when the last
--    reading is older than its interval (see `domain::reminder::reading_status`); it keeps its
--    start in `due_date`, so the existing "due_date or due_counter" CHECK still holds for it.
PRAGMA foreign_keys = off;

BEGIN;

CREATE TABLE activities_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id     INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date          TEXT NOT NULL,
    category      TEXT NOT NULL CHECK (category IN
                    ('maintenance','repair','purchase','inspection','modification','fuel','other',
                     'symptom','treatment','appointment','medication','reading')),
    title         TEXT NOT NULL,
    notes         TEXT NOT NULL DEFAULT '',
    counter_value INTEGER,
    cost_cents    INTEGER,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    quantity_milli INTEGER,
    client_op_id  TEXT,
    client_uuid   TEXT,
    deleted_at    TEXT
);
INSERT INTO activities_new (id, object_id, date, category, title, notes, counter_value, cost_cents,
                            created_at, updated_at, quantity_milli, client_op_id, client_uuid, deleted_at)
SELECT id, object_id, date, category, title, notes, counter_value, cost_cents,
       created_at, updated_at, quantity_milli, client_op_id, client_uuid, deleted_at
FROM activities;
DROP TABLE activities;
ALTER TABLE activities_new RENAME TO activities;
CREATE INDEX idx_activities_object_date ON activities(object_id, date);
CREATE UNIQUE INDEX idx_activities_client_op ON activities(client_op_id) WHERE client_op_id IS NOT NULL;
CREATE UNIQUE INDEX idx_activities_uuid ON activities(client_uuid);

-- Every existing reminder is a service reminder, which is what the default says.
ALTER TABLE reminders ADD COLUMN kind TEXT NOT NULL DEFAULT 'service' CHECK (kind IN ('service', 'reading'));
ALTER TABLE reminders ADD COLUMN every_n INTEGER;
ALTER TABLE reminders ADD COLUMN every_unit TEXT CHECK (every_unit IN ('week', 'month'));

COMMIT;

PRAGMA foreign_keys = on;
