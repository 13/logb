-- An object may belong inside another one: a garage inside a house, a light inside the garage.
-- Nullable, so every existing object -- which has no parent today -- needs no backfill.
--
-- No `ON DELETE` clause: deletion in this app is a soft tombstone (`UPDATE ... SET deleted_at`),
-- not a real `DELETE`, so `ON DELETE CASCADE` would only ever fire during the retention purge's
-- hard delete, days after a user-facing delete already happened via `record::cascade_object`.
-- Leaving it unspecified means SQLite (and PostgreSQL) refuse a hard delete of a parent while
-- any row still references it -- a second, defence-in-depth layer under the purge guard in
-- `sync/feed.rs`, which must already have tombstoned every descendant before this could matter.
ALTER TABLE objects ADD COLUMN parent_id INTEGER REFERENCES objects(id);
CREATE INDEX idx_objects_parent ON objects(parent_id);
