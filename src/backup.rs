//! The nightly snapshot.
//!
//! `VACUUM INTO` (via `db::backup_to`) reads through a consistent snapshot, so this is safe
//! against a live instance -- unlike copying the file, which can catch it mid-write and miss the
//! WAL entirely.

use crate::db::{self, BoxError};
use crate::error::AppError;
use crate::state::App;
use std::path::{Path, PathBuf};

/// How many snapshots survive. Two weeks is long enough to notice a bad delete that nobody
/// spotted the same day, and bounded so the directory can never fill the volume.
pub const KEEP: usize = 14;

/// Opens a finished snapshot and asks SQLite whether it is sound.
///
/// A backup nobody has opened is a guess. This is the cheapest possible proof, and it runs in
/// milliseconds on a database this size.
///
/// `integrity_check` alone is not enough: SQLite treats a zero-length file as a valid,
/// schema-less database, so a `touch`, an interrupted copy, or a `backup_to` that died partway
/// would sail through it. This checks the file is non-empty and that the schema this
/// application always migrates in (`_sqlx_migrations`) is actually present, in addition to the
/// integrity check -- and gives each rejection its own wording, since these are read from a log
/// at 3am rather than matched in code.
pub async fn verify(path: &Path) -> Result<(), BoxError> {
    let len = std::fs::metadata(path)?.len();
    if len == 0 {
        return Err("the file is empty, not a database".into());
    }

    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;

    // Capture result to avoid early return with `?` that skips pool.close(). Close pool
    // unconditionally, then match and return or continue.
    let has_schema: Result<Option<(String,)>, _> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'")
            .fetch_optional(&pool)
            .await;
    pool.close().await;

    match has_schema {
        Ok(Some(_)) => {},  // schema found, continue to integrity check
        Ok(None) => return Err("the file is a valid SQLite database but has no LogB schema".into()),
        Err(e) => return Err(Box::new(e)),
    }

    // Reopen pool for integrity check.
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;

    let result: Result<(String,), _> = sqlx::query_as("PRAGMA integrity_check").fetch_one(&pool).await;
    pool.close().await;
    match result {
        Ok((r,)) if r == "ok" => Ok(()),
        Ok((r,)) => Err(format!("integrity_check said {r}").into()),
        Err(e) => Err(Box::new(e)),
    }
}

/// Writes today's snapshot if it is due and not already there. Returns the path when one was
/// made, `None` when there was nothing to do.
pub async fn tick(state: &App, hour_now: u32) -> Result<Option<PathBuf>, AppError> {
    let Some(dir) = state.config.backup_dir.clone() else { return Ok(None) };
    if hour_now < state.config.backup_hour {
        return Ok(None);
    }
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("logb-{}.db", db::today()));

    // An existing file only counts as done if it verifies. One that does not is worse than
    // nothing -- it occupies today's slot while being unrestorable -- so it is replaced.
    if dest.exists() {
        if verify(&dest).await.is_ok() {
            return Ok(None);
        }
        tracing::warn!(path = %dest.display(), "replacing an unverifiable snapshot");
        std::fs::remove_file(&dest)?;
    }

    if let Err(e) = db::backup_to(&state.db, &dest).await {
        // A disk-full or similar failure can still leave a partial (even zero-length) file at
        // `dest`. Left behind, it would be mistaken for a finished backup on the next tick
        // today, the same way an unverifiable one would be -- so clear it the same way.
        let _ = std::fs::remove_file(&dest);
        return Err(AppError::Internal(e.to_string()));
    }
    if let Err(e) = verify(&dest).await {
        // Leave no unrestorable file behind, and leave every earlier snapshot alone: a failure
        // today must not cost yesterday's good copy.
        let _ = std::fs::remove_file(&dest);
        return Err(AppError::Internal(format!("snapshot failed verification: {e}")));
    }

    // A prune problem must never be reported as a backup failure: the snapshot above is already
    // written and verified, so `prune` handles its own errors internally rather than via `?`.
    prune(&dir);
    Ok(Some(dest))
}

/// Deletes all but the newest `KEEP` snapshots.
///
/// Entries are selected by name pattern only (`logb-*.db`) -- there is no provenance tracking,
/// so a directory or symlink that happens to match is treated the same as a real snapshot. A
/// dated name sorts chronologically as a string, so ordering needs no parsing, and anything
/// that does not match the pattern is not ours to delete.
///
/// Failures are logged and skipped rather than propagated: this runs after a snapshot has
/// already been written and verified, so one undeletable entry (a stray directory, a
/// permission problem) must not stop the others from being pruned, and must never be mistaken
/// by the caller for the backup itself having failed.
fn prune(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e, "could not list backup directory for pruning");
            return;
        }
    };
    let mut ours: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("logb-") && n.ends_with(".db"))
        })
        .collect();
    ours.sort();
    let excess = ours.len().saturating_sub(KEEP);
    for path in ours.into_iter().take(excess) {
        tracing::debug!(path = %path.display(), "pruning an expired snapshot");
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), error = %e, "could not prune an expired snapshot");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SQLite treats a zero-length file as a valid, schema-less database -- `integrity_check`
    /// on one returns "ok". `verify` must catch this itself, before ever asking SQLite.
    #[tokio::test]
    async fn verify_rejects_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.db");
        std::fs::write(&path, b"").unwrap();
        let err = verify(&path).await.unwrap_err();
        assert!(err.to_string().to_lowercase().contains("empty"), "reason should name the file as empty: {err}");
    }

    /// A file can be a perfectly sound SQLite database and still not be a LogB backup --
    /// `integrity_check` alone cannot tell the difference, so `verify` must also look for the
    /// schema this application always creates.
    #[tokio::test]
    async fn verify_rejects_a_valid_sqlite_file_that_is_not_a_logb_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unrelated.db");
        {
            let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&path).create_if_missing(true);
            let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
            sqlx::query("CREATE TABLE not_logb (id INTEGER)").execute(&pool).await.unwrap();
            pool.close().await;
        }
        let err = verify(&path).await.unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("migration") || err.to_string().to_lowercase().contains("schema"),
            "reason should name the missing LogB schema, distinct from an integrity failure: {err}"
        );
    }
}
