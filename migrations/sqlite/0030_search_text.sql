-- The folded text `GET /search` matches against: every searched field of the row, each folded
-- by `domain::tags::fold` (NFD, combining marks dropped, lower case) and joined by U+001F. It is
-- written by Rust on every write path (`search_text::refresh_*`), never by SQL, because neither
-- backend can fold accents itself -- see `src/search_text.rs`.
--
-- Nullable on purpose: NULL means "not folded yet". Rows that exist when this runs are filled at
-- the next start by `search_text::backfill`, in batches, and so is every row again whenever the
-- folding changes (the `search_fold_version` setting).
ALTER TABLE objects ADD COLUMN search_text TEXT;
ALTER TABLE activities ADD COLUMN search_text TEXT;
