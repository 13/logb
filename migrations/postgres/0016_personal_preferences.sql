ALTER TABLE users ADD COLUMN appearance TEXT;
ALTER TABLE users ADD COLUMN notify_hour BIGINT CHECK (notify_hour BETWEEN 0 AND 23);
CREATE TABLE notification_deliveries (
    target TEXT PRIMARY KEY,
    user_id BIGINT REFERENCES users(id) ON DELETE CASCADE,
    day TEXT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    attempted_at TEXT,
    last_success TEXT,
    last_error TEXT
);
CREATE INDEX idx_notification_deliveries_user ON notification_deliveries(user_id);
