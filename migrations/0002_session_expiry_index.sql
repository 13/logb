-- Expired sessions are swept on a timer now, not only at login, and the sweep filters on
-- expires_at.
CREATE INDEX idx_sessions_expires ON sessions(expires_at);
