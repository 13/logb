-- The sync purge (`sync::feed::purge`) runs `DELETE FROM changes WHERE applied_at < $1` every
-- hour, and without an index that is a full scan of the whole change feed -- the one table that
-- grows with every edit anyone makes.
CREATE INDEX idx_changes_applied_at ON changes(applied_at);
-- "Does any row still reference this blob" (`files WHERE sha256 = $1`, asked before a blob is
-- unlinked) has no user_id to lead with, so the (user_id, sha256) unique index cannot serve it.
CREATE INDEX idx_files_sha256 ON files(sha256);
