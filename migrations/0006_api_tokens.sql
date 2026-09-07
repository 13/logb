-- Long-lived bearer tokens, so a client that is not a browser can authenticate without
-- pretending to be one. A cookie is the wrong shape for a native app: SameSite and HttpOnly
-- exist to constrain a browser, and nothing outside one benefits from either.
--
-- Unlike `sessions`, which stores its token as the primary key, only a HASH of the token is
-- kept. A session lasts 30 days and its value is in a cookie jar; an API token has no expiry
-- and lives in a device's keystore, so a leaked backup would otherwise hand over every account
-- it names, indefinitely and undetectably. The plaintext is shown to the user exactly once, at
-- creation, and is not recoverable afterwards.
--
-- `prefix` is the first few characters of the plaintext, kept so the list can identify a token
-- the user is looking at without being able to reconstruct it.
CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    prefix       TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
);
CREATE INDEX idx_api_tokens_user ON api_tokens(user_id);
