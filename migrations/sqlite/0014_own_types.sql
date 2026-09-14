-- no-transaction
--
-- Each user's own object types. Three changes:
--
-- 1. `object_types` holds them. An object refers to one as `custom:<client_uuid>` -- the uuid,
--    not the id, so a type and the objects using it can be created offline in one go.
-- 2. `objects.type` loses its CHECK: a custom key cannot be listed in one, so validity moves to
--    `object_type::is_valid_for_user`. The column stays NOT NULL.
-- 3. `changes.entity` also admits `object_type`, so type writes reach the sync log. Nothing
--    references `changes`, so it is rebuilt the same way; its AUTOINCREMENT counter is carried
--    over, because a device's cursor must never see a `seq` handed out twice, and an emptied log
--    would otherwise restart from 1.
--
-- SQLite cannot drop a CHECK in place, so `objects` is rebuilt following 0009 exactly (its
-- AUTOINCREMENT counter carried over the same way as `changes`'), for the
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
-- The copy above sets `objects_new`'s counter to the highest id still present; the old counter
-- also remembers hard-deleted rows, so it is carried over and no deleted object's id is reused.
DELETE FROM sqlite_sequence WHERE name = 'objects_new';
INSERT INTO sqlite_sequence (name, seq) SELECT 'objects_new', seq FROM sqlite_sequence WHERE name = 'objects';
DROP TABLE objects;
ALTER TABLE objects_new RENAME TO objects;
-- Dropping a table drops its indexes; `idx_objects_uuid` is what stops sync inserting the same
-- offline-created row twice, and `idx_objects_parent` keeps child listing off a full scan.
CREATE INDEX idx_objects_user ON objects(user_id);
CREATE UNIQUE INDEX idx_objects_uuid ON objects(client_uuid);
CREATE INDEX idx_objects_parent ON objects(parent_id);

CREATE TABLE changes_new (
    seq          INTEGER PRIMARY KEY AUTOINCREMENT,
    entity       TEXT NOT NULL CHECK (entity IN ('object','activity','reminder','attachment','file','object_type')),
    entity_uuid  TEXT NOT NULL,
    op           TEXT NOT NULL CHECK (op IN ('create','set','delete')),
    field        TEXT,
    value        TEXT,
    edited_at    TEXT NOT NULL,
    applied_at   TEXT NOT NULL,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id    TEXT NOT NULL,
    client_op_id TEXT NOT NULL
);
INSERT INTO changes_new (seq, entity, entity_uuid, op, field, value, edited_at, applied_at, user_id,
                         device_id, client_op_id)
SELECT seq, entity, entity_uuid, op, field, value, edited_at, applied_at, user_id, device_id,
       client_op_id
FROM changes;
DELETE FROM sqlite_sequence WHERE name = 'changes_new';
INSERT INTO sqlite_sequence (name, seq) SELECT 'changes_new', seq FROM sqlite_sequence WHERE name = 'changes';
DROP TABLE changes;
ALTER TABLE changes_new RENAME TO changes;
CREATE INDEX idx_changes_user_seq ON changes(user_id, seq);
CREATE UNIQUE INDEX idx_changes_user_op ON changes(user_id, client_op_id);

-- With foreign keys off, nothing checked that every child still points at a real object. A bare
-- `PRAGMA foreign_key_check` only returns rows, which the migrator ignores, so the count goes
-- into a CHECK instead: any violation fails this statement and the migration with it, before the
-- COMMIT, so the rebuild never lands. Scoped to the tables the rebuild touches, so an unrelated
-- old inconsistency elsewhere does not block an upgrade.
CREATE TEMP TABLE fk_guard_0014 (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO fk_guard_0014 (violations)
SELECT (SELECT count(*) FROM pragma_foreign_key_check('objects'))
     + (SELECT count(*) FROM pragma_foreign_key_check('activities'))
     + (SELECT count(*) FROM pragma_foreign_key_check('reminders'))
     + (SELECT count(*) FROM pragma_foreign_key_check('attachments'))
     + (SELECT count(*) FROM pragma_foreign_key_check('object_types'))
     + (SELECT count(*) FROM pragma_foreign_key_check('changes'));
DROP TABLE fk_guard_0014;

COMMIT;

PRAGMA foreign_keys = on;
