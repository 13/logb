-- Identity a client can mint before the server has seen the row, so a create made offline can
-- be referenced by the activities and attachments created alongside it in the same session.
-- Backfilled for existing rows with SQLite's own randomness: the format only has to be unique
-- and stable, and these rows predate any client that could have named them.
ALTER TABLE objects     ADD COLUMN client_uuid TEXT;
ALTER TABLE activities  ADD COLUMN client_uuid TEXT;
ALTER TABLE reminders   ADD COLUMN client_uuid TEXT;
ALTER TABLE attachments ADD COLUMN client_uuid TEXT;
ALTER TABLE files       ADD COLUMN client_uuid TEXT;

UPDATE objects     SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE activities  SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE reminders   SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE attachments SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;
UPDATE files       SET client_uuid = lower(hex(randomblob(16))) WHERE client_uuid IS NULL;

CREATE UNIQUE INDEX idx_objects_uuid     ON objects(client_uuid);
CREATE UNIQUE INDEX idx_activities_uuid  ON activities(client_uuid);
CREATE UNIQUE INDEX idx_reminders_uuid   ON reminders(client_uuid);
CREATE UNIQUE INDEX idx_attachments_uuid ON attachments(client_uuid);
CREATE UNIQUE INDEX idx_files_uuid       ON files(client_uuid);

-- Tombstones. A hard delete is invisible to a client that was offline when it happened, so
-- deletes are recorded instead of applied. `tasks.rs` purges these past the retention window.
ALTER TABLE objects     ADD COLUMN deleted_at TEXT;
ALTER TABLE activities  ADD COLUMN deleted_at TEXT;
ALTER TABLE reminders   ADD COLUMN deleted_at TEXT;
ALTER TABLE attachments ADD COLUMN deleted_at TEXT;
ALTER TABLE files       ADD COLUMN deleted_at TEXT;

-- The append-only log. `seq` is the pull cursor: monotonic, gapless per database, and ordered
-- by the order the server accepted work rather than by any device's clock.
CREATE TABLE changes (
    seq          INTEGER PRIMARY KEY AUTOINCREMENT,
    entity       TEXT NOT NULL CHECK (entity IN ('object','activity','reminder','attachment','file')),
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
CREATE INDEX idx_changes_user_seq ON changes(user_id, seq);

-- Op ids are minted by clients, so they are only ever unique within the account that minted
-- them. A global UNIQUE on `client_op_id` would make one account's id collide with another's,
-- and the push handler's idempotency lookup would then report the second account's op as
-- already applied and silently drop the write. Uniqueness is per user, and the handler's
-- lookup is scoped the same way -- the two must agree or idempotency is either a lost write
-- or a constraint violation.
CREATE UNIQUE INDEX idx_changes_user_op ON changes(user_id, client_op_id);

-- The winning edit time per field, which is what an arriving op is compared against. Separate
-- from `changes` because that table holds losers too, and a scan of it per field would grow
-- with history rather than with the data.
CREATE TABLE field_clock (
    entity      TEXT NOT NULL,
    entity_uuid TEXT NOT NULL,
    field       TEXT NOT NULL,
    edited_at   TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    PRIMARY KEY (entity, entity_uuid, field)
);
