-- Notifications per person. Until now there was one webhook for the whole instance, so on a
-- shared instance everyone's reminders went to the same place, in English.
--
-- `notify_url` is a user's own webhook (an ntfy topic, say); null keeps them in the instance
-- digest. `push_subscriptions` holds one row per browser a user turned push notifications on
-- in. The endpoint is unique: it names a browser profile, so subscribing it again -- or someone
-- else signing in on that browser -- moves the row rather than duplicating it.
ALTER TABLE users ADD COLUMN notify_url TEXT;
ALTER TABLE users ADD COLUMN notify_format TEXT NOT NULL DEFAULT 'text' CHECK (notify_format IN ('json', 'text'));

CREATE TABLE push_subscriptions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    endpoint   TEXT NOT NULL,
    p256dh     TEXT NOT NULL,
    auth       TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_push_subscriptions_endpoint ON push_subscriptions(endpoint);
CREATE INDEX idx_push_subscriptions_user ON push_subscriptions(user_id);
