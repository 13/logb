//! Putting a snapshot back.
//!
//! The order of operations is the design: nothing is touched until the source has proved itself,
//! and the database being replaced is moved aside rather than deleted, because a restore is
//! destructive and operators do sometimes restore the wrong file.

use crate::db::{self, BoxError};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Report {
    /// Where the replaced database was moved, if there was one.
    pub replaced_to: Option<PathBuf>,
    /// The identity the restored database now advertises. Every device holding the old one is
    /// sent back to a full bootstrap.
    pub epoch: String,
}

pub async fn run(data_dir: &Path, snapshot: &Path) -> Result<Report, BoxError> {
    // 1. Prove the source before risking anything.
    crate::backup::verify(snapshot)
        .await
        .map_err(|e| -> BoxError { format!("{} is not a usable snapshot: {e}", snapshot.display()).into() })?;
    {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(snapshot)
            .create_if_missing(false)
            .read_only(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
        let looks_right: Result<(i64,), _> =
            sqlx::query_as("SELECT count(*) FROM _sqlx_migrations").fetch_one(&pool).await;
        pool.close().await;
        looks_right.map_err(|e| -> BoxError {
            format!("{} is not a usable snapshot: no migration history ({e})", snapshot.display()).into()
        })?;
    }

    // 2. Move the live database aside. Its -wal and -shm go with it: leaving them beside a
    //    different database would have SQLite reading another file's journal.
    let live = data_dir.join("logby.db");
    let replaced_to = if live.exists() {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let dest = data_dir.join(format!("logby.db.replaced-{stamp}"));
        std::fs::rename(&live, &dest)?;
        for suffix in ["-wal", "-shm"] {
            let from = data_dir.join(format!("logby.db{suffix}"));
            if from.exists() {
                std::fs::rename(&from, data_dir.join(format!("logby.db.replaced-{stamp}{suffix}")))?;
            }
        }
        Some(dest)
    } else {
        None
    };

    // 3. Put the snapshot in place, then let `connect` migrate it -- a snapshot may predate the
    //    binary restoring it.
    std::fs::copy(snapshot, &live)?;
    let pool = db::connect(data_dir).await?;

    // 4. Give it a new identity, so no device resumes on a cursor whose meaning has changed.
    let epoch = crate::sync::epoch::rotate(&pool).await?;
    pool.close().await;

    Ok(Report { replaced_to, epoch })
}
