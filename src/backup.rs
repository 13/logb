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
pub async fn verify(path: &Path) -> Result<(), BoxError> {
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
    let dest = dir.join(format!("logby-{}.db", db::today()));

    // An existing file only counts as done if it verifies. One that does not is worse than
    // nothing -- it occupies today's slot while being unrestorable -- so it is replaced.
    if dest.exists() {
        if verify(&dest).await.is_ok() {
            return Ok(None);
        }
        tracing::warn!(path = %dest.display(), "replacing an unverifiable snapshot");
        std::fs::remove_file(&dest)?;
    }

    db::backup_to(&state.db, &dest).await.map_err(|e| AppError::Internal(e.to_string()))?;
    if let Err(e) = verify(&dest).await {
        // Leave no unrestorable file behind, and leave every earlier snapshot alone: a failure
        // today must not cost yesterday's good copy.
        let _ = std::fs::remove_file(&dest);
        return Err(AppError::Internal(format!("snapshot failed verification: {e}")));
    }

    prune(&dir)?;
    Ok(Some(dest))
}

/// Deletes all but the newest `KEEP` snapshots.
///
/// Only files this module named are considered. A dated name sorts chronologically as a string,
/// so ordering needs no parsing -- and anything else in the directory is not ours to delete.
fn prune(dir: &Path) -> std::io::Result<()> {
    let mut ours: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("logby-") && n.ends_with(".db"))
        })
        .collect();
    ours.sort();
    let excess = ours.len().saturating_sub(KEEP);
    for path in ours.into_iter().take(excess) {
        tracing::debug!(path = %path.display(), "pruning an expired snapshot");
        std::fs::remove_file(path)?;
    }
    Ok(())
}
