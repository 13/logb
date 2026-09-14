-- Tags on objects and entries: a JSON array of strings, normalised by `domain::tags` before it is
-- ever written, so every reader can trust its shape. A column, not a join table: tags travel
-- through sync, export and offline edits exactly like `notes` without a new entity.
ALTER TABLE objects ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
ALTER TABLE activities ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
