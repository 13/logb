-- no-transaction
--
-- Each user's own object types. Two changes:
--
-- 1. `object_types` holds them. An object refers to one as `custom:<client_uuid>` -- the uuid,
--    not the id, so a type and the objects using it can be created offline in one go.
-- 2. `objects.type` loses its CHECK: a custom key cannot be listed in one, so validity moves to
--    `object_type::is_valid_for_user`. The column stays NOT NULL.
--
-- SQLite cannot drop a CHECK in place, so `objects` is rebuilt following 0009 exactly, for the
-- reasons it spells out in full: foreign keys off (a DROP with enforcement on cascades into every
-- activity, reminder and attachment), the `-- no-transaction` line FIRST, and create-copy-drop-
-- rename (renaming the old table away would repoint every child's REFERENCES at the table being
-- dropped). The live column order is kept, including `parent_id` (0010) and `tags` (0013), which
-- ALTER TABLE placed at the end. No row changes value, so unlike 0009 the sync epoch stays.
PRAGMA foreign_keys = off;

BEGIN;

CREATE TABLE object_types (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_uuid  TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL,
    icon         TEXT NOT NULL,
    categories   TEXT NOT NULL,
    counter_unit TEXT CHECK (counter_unit IN ('km','mi','h')),
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    deleted_at   TEXT
);
CREATE INDEX idx_object_types_user ON object_types(user_id);

-- `parent_id` below names `objects`, not `objects_new`: once the rename lands, that is the table
-- itself. (Comments stay outside the CREATE: SQLite stores its text verbatim in the schema.)
CREATE TABLE objects_new (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id              INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    type                 TEXT NOT NULL,
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
    deleted_at           TEXT,
    parent_id            INTEGER REFERENCES objects(id),
    tags                 TEXT NOT NULL DEFAULT '[]'
);
INSERT INTO objects_new (id, user_id, name, type, counter_unit, description, purchase_date,
                         purchase_price_cents, archived_at, cover_attachment_id, created_at,
                         updated_at, fuel_unit, client_uuid, deleted_at, parent_id, tags)
SELECT id, user_id, name, type, counter_unit, description, purchase_date,
       purchase_price_cents, archived_at, cover_attachment_id, created_at,
       updated_at, fuel_unit, client_uuid, deleted_at, parent_id, tags
FROM objects;
DROP TABLE objects;
ALTER TABLE objects_new RENAME TO objects;
-- Dropping a table drops its indexes; `idx_objects_uuid` is what stops sync inserting the same
-- offline-created row twice, and `idx_objects_parent` keeps child listing off a full scan.
CREATE INDEX idx_objects_user ON objects(user_id);
CREATE UNIQUE INDEX idx_objects_uuid ON objects(client_uuid);
CREATE INDEX idx_objects_parent ON objects(parent_id);

COMMIT;

PRAGMA foreign_keys = on;
