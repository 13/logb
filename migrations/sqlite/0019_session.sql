-- no-transaction
-- Sessions reuse the existing place and duration columns; this migration only extends the category check.
PRAGMA foreign_keys = off;
BEGIN;
CREATE TABLE activities_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN
      ('maintenance','repair','purchase','inspection','modification','fuel','other','symptom','treatment','appointment','medication','reading','trip','weight','session')),
    title TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '', counter_value INTEGER, cost_cents INTEGER,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL, quantity_milli INTEGER, client_op_id TEXT,
    client_uuid TEXT, deleted_at TEXT, tags TEXT NOT NULL DEFAULT '[]', start_counter INTEGER,
    from_place TEXT, to_place TEXT, duration_minutes INTEGER, battery_used_pct INTEGER,
    charged_full INTEGER NOT NULL DEFAULT 0, weight_grams INTEGER CHECK (weight_grams > 0)
);
INSERT INTO activities_new SELECT * FROM activities;
DROP TABLE activities;
ALTER TABLE activities_new RENAME TO activities;
CREATE INDEX idx_activities_object_date ON activities(object_id, date);
CREATE UNIQUE INDEX idx_activities_client_op ON activities(client_op_id) WHERE client_op_id IS NOT NULL;
CREATE UNIQUE INDEX idx_activities_uuid ON activities(client_uuid);
CREATE INDEX idx_activities_weight ON activities(object_id, date DESC, created_at DESC, id DESC) WHERE deleted_at IS NULL AND weight_grams IS NOT NULL;
COMMIT;
PRAGMA foreign_keys = on;
