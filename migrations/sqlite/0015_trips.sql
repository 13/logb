-- no-transaction
--
-- Trip log: a trip is an entry with the new category `trip`. Its end is the existing
-- `counter_value`, so the object's current counter, counter reminders and usage per month keep
-- working unchanged. The trip-only details are five nullable columns here, not a table, so they
-- travel through sync, export, offline edits and attachments like `notes` does.
--
-- Two changes, for the reasons 0009/0011 already spell out in full -- foreign keys must be off
-- for the rebuild, the `-- no-transaction` line must stay the FIRST line, and the rebuild
-- creates the new table, copies, drops the old and renames the new one into place:
--
-- 1. activities.category grows `trip`. SQLite cannot alter a CHECK in place, so the table is
--    rebuilt, keeping its live column order.
-- 2. activities gains start_counter, from_place, to_place, duration_minutes and
--    battery_used_pct -- all nullable, used only by a trip.
PRAGMA foreign_keys = off;

BEGIN;

CREATE TABLE activities_new (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id        INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date             TEXT NOT NULL,
    category         TEXT NOT NULL CHECK (category IN
                       ('maintenance','repair','purchase','inspection','modification','fuel','other',
                        'symptom','treatment','appointment','medication','reading','trip')),
    title            TEXT NOT NULL,
    notes            TEXT NOT NULL DEFAULT '',
    counter_value    INTEGER,
    cost_cents       INTEGER,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    quantity_milli   INTEGER,
    client_op_id     TEXT,
    client_uuid      TEXT,
    deleted_at       TEXT,
    tags             TEXT NOT NULL DEFAULT '[]',
    start_counter    INTEGER,
    from_place       TEXT,
    to_place         TEXT,
    duration_minutes INTEGER,
    battery_used_pct INTEGER
);
INSERT INTO activities_new (id, object_id, date, category, title, notes, counter_value, cost_cents,
                            created_at, updated_at, quantity_milli, client_op_id, client_uuid,
                            deleted_at, tags)
SELECT id, object_id, date, category, title, notes, counter_value, cost_cents,
       created_at, updated_at, quantity_milli, client_op_id, client_uuid, deleted_at, tags
FROM activities;
DROP TABLE activities;
ALTER TABLE activities_new RENAME TO activities;
CREATE INDEX idx_activities_object_date ON activities(object_id, date);
CREATE UNIQUE INDEX idx_activities_client_op ON activities(client_op_id) WHERE client_op_id IS NOT NULL;
CREATE UNIQUE INDEX idx_activities_uuid ON activities(client_uuid);

COMMIT;

PRAGMA foreign_keys = on;
