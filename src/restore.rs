//! Putting a snapshot back.
//!
//! The order of operations is the design: nothing is touched until the source has proved itself,
//! and the database being replaced is moved aside rather than deleted, because a restore is
//! destructive and operators do sometimes restore the wrong file.

use crate::db::{self, BoxError};
use std::path::{Path, PathBuf};

/// Picks out the one `sqlx::migrate!` failure that is not a real migration failure: a snapshot
/// carrying a migration version this binary's embedded set does not contain. That is the ahead-
/// schema case (an older binary restoring a newer release's backup), and it is detected before
/// any migration is applied -- see `validate_applied_migrations` in sqlx -- so it must not be
/// treated the same as a genuine migration error. Every other `MigrateError` variant (a dirty
/// migration, a checksum mismatch, an execution failure) still means the abort-with-explanation
/// path below, so this stays narrow on purpose.
fn ahead_schema_version(err: &BoxError) -> Option<i64> {
    match err.downcast_ref::<sqlx::migrate::MigrateError>() {
        Some(sqlx::migrate::MigrateError::VersionMissing(version)) => Some(*version),
        _ => None,
    }
}

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
        // A row count, not just a successful query: the table existing but empty would answer
        // `fetch_one` fine, yet is not "migration history" -- every real logby database has run
        // at least one migration, so an empty table is exactly as suspect as a missing one.
        let count: Result<(i64,), _> =
            sqlx::query_as("SELECT count(*) FROM _sqlx_migrations").fetch_one(&pool).await;
        pool.close().await;
        match count {
            Ok((n,)) if n > 0 => {},
            Ok(_) => {
                return Err(
                    format!("{} is not a usable snapshot: no migration history (table is empty)", snapshot.display())
                        .into(),
                )
            },
            Err(e) => {
                return Err(format!(
                    "{} is not a usable snapshot: no migration history ({e})",
                    snapshot.display()
                )
                .into())
            },
        }
    }

    // 2. Move the live database aside, and its -wal/-shm with it. The sidecars are handled
    //    whether or not `logby.db` itself exists: a restore that died between this step and the
    //    copy below leaves exactly that -- an orphaned -wal/-shm with no main file -- and if left
    //    in place it would sit beside the database the copy is about to create, with SQLite
    //    reading it as that database's journal. Both share one stamp so the set, main file or
    //    not, stays recoverable together.
    let live = data_dir.join("logby.db");
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let replaced_to = if live.exists() {
        let dest = data_dir.join(format!("logby.db.replaced-{stamp}"));
        std::fs::rename(&live, &dest)?;
        Some(dest)
    } else {
        None
    };
    for suffix in ["-wal", "-shm"] {
        let from = data_dir.join(format!("logby.db{suffix}"));
        if from.exists() {
            std::fs::rename(&from, data_dir.join(format!("logby.db.replaced-{stamp}{suffix}")))?;
        }
    }

    // From here on, `logby.db` has already been touched -- there is no "nothing happened"
    // reading of a failure any more. A bare propagated error would let an operator assume a
    // failed `--restore` is a no-op, start the server, and run it on a restored database that
    // still advertises the old sync epoch (or isn't fully migrated) -- exactly what epoch
    // rotation exists to prevent. Every error past this point must say plainly that the swap
    // already happened, where the previous copy is kept, which step failed, and what to do.
    let recovery = match &replaced_to {
        Some(p) => format!("the database it replaced is kept at {}", p.display()),
        None => "there was no previous database to keep -- this data directory had none".to_string(),
    };

    // 3. Put the snapshot in place, then let `connect` migrate it -- a snapshot may predate the
    //    binary restoring it.
    std::fs::copy(snapshot, &live).map_err(|e| -> BoxError {
        format!(
            "restore failed while copying the snapshot into place ({e}). the live database has \
             already been replaced and {} is currently MISSING -- there is no database there at \
             all. {recovery}. restore the previous copy (or rerun --restore) before starting \
             the server.",
            live.display()
        )
        .into()
    })?;
    let pool = match db::connect(data_dir).await {
        Ok(pool) => pool,
        Err(e) => match ahead_schema_version(&e) {
            // A snapshot may instead be ahead of this binary -- an older binary restoring a
            // newer release's backup -- which `sqlx::migrate!` reports as `VersionMissing`
            // rather than a real migration failure: it happens before any migration is
            // applied, so the copy just placed is untouched and fully usable, not "partially
            // migrated". `api::mod::health` already treats an ahead schema as healthy on the
            // grounds that migrations are additive; a restore has to reach the same
            // conclusion, or an operator gets a green health probe on a database that never
            // had its epoch rotated -- exactly what epoch rotation exists to prevent, and
            // re-running --restore would hit this same "failure" forever.
            Some(version) => {
                tracing::warn!(
                    version,
                    "restored snapshot carries migration {version}, which this binary does \
                     not recognise; treating the schema as ahead (additive-only, per the \
                     health check's own rule) instead of aborting the restore"
                );
                db::connect_existing(data_dir).await.map_err(|e| -> BoxError {
                    format!(
                        "restore placed an ahead-schema snapshot but reopening it read-write \
                         failed ({e}). {recovery}. do not start the server against it until \
                         this is understood.",
                    )
                    .into()
                })?
            },
            // Every other migration error (a dirty migration, a checksum mismatch, an
            // execution failure) keeps aborting -- this catch stays narrow on purpose.
            None => {
                return Err(format!(
                    "restore failed while migrating the restored snapshot ({e}). the live \
                     database has already been replaced with a PARTIALLY MIGRATED copy of the \
                     snapshot at {}. {recovery}. do not start the server against it -- restore \
                     the previous copy (or a known-good snapshot) before starting the server.",
                    live.display()
                )
                .into())
            },
        },
    };

    // 4. Give it a new identity, so no device resumes on a cursor whose meaning has changed.
    let epoch = crate::sync::epoch::rotate(&pool).await.map_err(|e| -> BoxError {
        format!(
            "restore failed while rotating the sync epoch ({e}). the live database has already \
             been replaced with the migrated snapshot, but it is STILL ADVERTISING THE OLD sync \
             epoch. {recovery}. do not start the server until the epoch is rotated -- rerun \
             --restore, or rotate the epoch by hand, before starting the server."
        )
        .into()
    })?;
    pool.close().await;

    Ok(Report { replaced_to, epoch })
}
