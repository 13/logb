-- no-transaction
--
-- Foreign keys must be off for this: `objects` and `activities` are both parents of cascading
-- children, and a DROP TABLE with enforcement on performs an implicit DELETE FROM that takes
-- every attachment with it. SQLite refuses to toggle the pragma inside a transaction, which is
-- why this migration manages its own. The `-- no-transaction` line above must stay the FIRST
-- line of the file: sqlx matches it with `starts_with`, so a blank line ahead of it disables
-- the behaviour silently and the pragma below becomes a no-op.
--
-- Note the order in each rebuild: create the new table, copy, DROP the old, then rename the new
-- one into place. Renaming the OLD table out of the way instead would make SQLite rewrite every
-- child's REFERENCES clause to follow it, quietly repointing them at the table being dropped.
--
-- Both rebuilds keep the column order the live tables actually have -- `fuel_unit`,
-- `client_uuid` and `deleted_at` arrived by ALTER TABLE in 0003 and 0007, so they sit at the
-- end rather than where a tidy declaration would put them. `sync::feed` reads these tables with
-- `SELECT *` and names the columns from the result set, so reordering would not break it, but a
-- rebuild that silently reshuffles a table is a difference nobody asked for.
PRAGMA foreign_keys = off;

BEGIN;

-- objects: free-text `category` becomes a constrained `type`.
CREATE TABLE objects_new (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id              INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    type                 TEXT NOT NULL CHECK (type IN
                           ('car','e_bike','bike','motorcycle','home','appliance','tool','body','other')),
    counter_unit         TEXT CHECK (counter_unit IN ('km', 'mi', 'h')),
    description          TEXT NOT NULL DEFAULT '',
    purchase_date        TEXT,
    purchase_price_cents INTEGER,
    archived_at          TEXT,
    cover_attachment_id  INTEGER,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    fuel_unit            TEXT CHECK (fuel_unit IN ('l', 'gal', 'kwh')),
    client_uuid          TEXT,
    deleted_at           TEXT
);

-- The CASE below is mirrored by `object_type::from_legacy`; `the_sql_mapping_matches_the_rust_one`
-- fails if the two drift. Exact matches only -- no substring guessing.
--
-- `lower()` in SQLite is ASCII-only: it leaves 'Ä' and 'Ö' untouched, while Rust's
-- `to_lowercase()` folds them to 'ä'/'ö'. `gerät`, `haushaltsgerät` and `körper` are the only
-- legacy words that contain either umlaut, so `category_key` folds just those two characters,
-- once, and both CASE expressions below key off it instead of off `lower(trim(category))` -- one
-- decides the type, the other decides whether the original text is preserved, and they must
-- keep agreeing on what counts as a match.
INSERT INTO objects_new (id, user_id, name, type, counter_unit, description, purchase_date,
                         purchase_price_cents, archived_at, cover_attachment_id, created_at,
                         updated_at, fuel_unit, client_uuid, deleted_at)
SELECT id, user_id, name,
       CASE category_key
         WHEN 'car' THEN 'car' WHEN 'auto' THEN 'car' WHEN 'pkw' THEN 'car' WHEN 'wagen' THEN 'car'
         WHEN 'e-bike' THEN 'e_bike' WHEN 'ebike' THEN 'e_bike' WHEN 'e bike' THEN 'e_bike' WHEN 'pedelec' THEN 'e_bike'
         WHEN 'bike' THEN 'bike' WHEN 'fahrrad' THEN 'bike' WHEN 'velo' THEN 'bike' WHEN 'rad' THEN 'bike'
         WHEN 'motorcycle' THEN 'motorcycle' WHEN 'motorrad' THEN 'motorcycle' WHEN 'motorbike' THEN 'motorcycle'
         WHEN 'home' THEN 'home' WHEN 'haus' THEN 'home' WHEN 'wohnung' THEN 'home' WHEN 'flat' THEN 'home' WHEN 'apartment' THEN 'home'
         WHEN 'appliance' THEN 'appliance' WHEN 'gerät' THEN 'appliance' WHEN 'geraet' THEN 'appliance' WHEN 'haushaltsgerät' THEN 'appliance'
         WHEN 'tool' THEN 'tool' WHEN 'werkzeug' THEN 'tool' WHEN 'maschine' THEN 'tool'
         WHEN 'body' THEN 'body' WHEN 'körper' THEN 'body' WHEN 'koerper' THEN 'body' WHEN 'health' THEN 'body' WHEN 'gesundheit' THEN 'body'
         ELSE 'other'
       END,
       counter_unit,
       -- Nothing the user typed is destroyed by a migration they did not ask for: text that did
       -- not map is appended to the description, on its own line.
       CASE
         WHEN category_key IN
           ('car','auto','pkw','wagen','e-bike','ebike','e bike','pedelec','bike','fahrrad','velo','rad',
            'motorcycle','motorrad','motorbike','home','haus','wohnung','flat','apartment',
            'appliance','gerät','geraet','haushaltsgerät','tool','werkzeug','maschine',
            'body','körper','koerper','health','gesundheit')
           THEN description
         WHEN trim(category) = '' THEN description
         WHEN trim(description) = '' THEN trim(category)
         ELSE description || char(10) || trim(category)
       END,
       purchase_date, purchase_price_cents, archived_at, cover_attachment_id,
       created_at, updated_at, fuel_unit, client_uuid, deleted_at
FROM (SELECT *, REPLACE(REPLACE(lower(trim(category)), 'Ä', 'ä'), 'Ö', 'ö') AS category_key FROM objects);

DROP TABLE objects;
ALTER TABLE objects_new RENAME TO objects;
-- Dropping a table drops its indexes. `idx_objects_uuid` is not decoration: it is what stops
-- sync inserting the same offline-created row twice.
CREATE INDEX idx_objects_user ON objects(user_id);
CREATE UNIQUE INDEX idx_objects_uuid ON objects(client_uuid);

-- activities: the CHECK grows by four health categories. SQLite cannot alter a CHECK in place.
CREATE TABLE activities_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id     INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date          TEXT NOT NULL,
    category      TEXT NOT NULL CHECK (category IN
                    ('maintenance','repair','purchase','inspection','modification','fuel','other',
                     'symptom','treatment','appointment','medication')),
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
-- `idx_activities_client_op` is what makes a retried write idempotent; losing it would let a
-- client that resends an op create a duplicate activity with nothing reporting an error.
CREATE INDEX idx_activities_object_date ON activities(object_id, date);
CREATE UNIQUE INDEX idx_activities_client_op ON activities(client_op_id) WHERE client_op_id IS NOT NULL;
CREATE UNIQUE INDEX idx_activities_uuid ON activities(client_uuid);

-- Field-level clocks for a column that no longer exists.
DELETE FROM field_clock WHERE entity = 'object' AND field = 'category';

-- Rows changed underneath every device without a single `changes` entry, so cursors must not
-- resume. A new epoch sends each device back to a full bootstrap, where it picks up `type`.
UPDATE settings SET value = lower(hex(randomblob(16))) WHERE key = 'sync_epoch';

COMMIT;

PRAGMA foreign_keys = on;
