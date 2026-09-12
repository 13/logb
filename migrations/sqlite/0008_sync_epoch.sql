-- Which database a cursor belongs to.
--
-- `changes.seq` is an autoincrement, so a restored snapshot reissues the same numbers for
-- entirely different ops. A device resuming on its old cursor would pull ops that are not the
-- edits it missed and apply them as though they were -- wrong history, silently, with nothing
-- reporting an error. Devices carry this value alongside their cursor; `--restore` changes it,
-- and a mismatch sends them back to a full bootstrap.
INSERT INTO settings (key, value) VALUES ('sync_epoch', lower(hex(randomblob(16))));
