-- `sessions.token` now holds sha256(token) as lowercase hex, never the token a browser holds.
-- A row written before this cannot be hashed in SQL on both backends alike, and would never
-- match a hashed lookup anyway, so every existing session ends here: everyone signs in once
-- after upgrading. docs/upgrading.md says so under 0.17.0.
DELETE FROM sessions;
