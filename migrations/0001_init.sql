CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    lang          TEXT NOT NULL DEFAULT 'en',
    created_at    TEXT NOT NULL
);

CREATE TABLE sessions (
    token      TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_user ON sessions(user_id);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO settings (key, value) VALUES ('currency', 'EUR');

CREATE TABLE objects (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id              INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    category             TEXT NOT NULL,
    counter_unit         TEXT CHECK (counter_unit IN ('km', 'mi', 'h')),
    description          TEXT NOT NULL DEFAULT '',
    purchase_date        TEXT,
    purchase_price_cents INTEGER,
    archived_at          TEXT,
    cover_attachment_id  INTEGER,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
CREATE INDEX idx_objects_user ON objects(user_id);

CREATE TABLE activities (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id     INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    date          TEXT NOT NULL,
    category      TEXT NOT NULL CHECK (category IN
                    ('maintenance','repair','purchase','inspection','modification','fuel','other')),
    title         TEXT NOT NULL,
    notes         TEXT NOT NULL DEFAULT '',
    counter_value INTEGER,
    cost_cents    INTEGER,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX idx_activities_object_date ON activities(object_id, date);

CREATE TABLE files (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sha256        TEXT NOT NULL,
    original_name TEXT NOT NULL,
    mime          TEXT NOT NULL,
    size          INTEGER NOT NULL,
    width         INTEGER,
    height        INTEGER,
    taken_at      TEXT,
    created_at    TEXT NOT NULL,
    UNIQUE (user_id, sha256)
);

CREATE TABLE attachments (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id   INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    activity_id INTEGER REFERENCES activities(id) ON DELETE CASCADE,
    file_id     INTEGER NOT NULL REFERENCES files(id) ON DELETE RESTRICT,
    kind        TEXT NOT NULL CHECK (kind IN ('photo', 'document')),
    caption     TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_attachments_object ON attachments(object_id);
CREATE INDEX idx_attachments_activity ON attachments(activity_id);
CREATE INDEX idx_attachments_file ON attachments(file_id);

CREATE TABLE reminders (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id        INTEGER NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    title            TEXT NOT NULL,
    notes            TEXT NOT NULL DEFAULT '',
    due_date         TEXT,
    due_counter      INTEGER,
    repeat_months    INTEGER,
    repeat_counter   INTEGER,
    done_at          TEXT,
    done_activity_id INTEGER REFERENCES activities(id) ON DELETE SET NULL,
    created_at       TEXT NOT NULL,
    CHECK (due_date IS NOT NULL OR due_counter IS NOT NULL)
);
CREATE INDEX idx_reminders_object ON reminders(object_id);
