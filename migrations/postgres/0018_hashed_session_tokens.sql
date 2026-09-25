-- `sessions.token` now holds sha256(token) as lowercase hex, never the token a browser holds.
-- A row written before this would never match a hashed lookup, so every existing session ends
-- here: everyone signs in once after upgrading. docs/upgrading.md says so under 0.17.0. The
-- SQLite twin is migrations/sqlite/0027_hashed_session_tokens.sql.
DELETE FROM sessions;
