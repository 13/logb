CREATE TABLE telegram_credentials (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    token_cipher TEXT NOT NULL,
    token_nonce TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    bot_username TEXT NOT NULL,
    update_offset TEXT NOT NULL DEFAULT '0',
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
