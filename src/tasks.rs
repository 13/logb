//! The one background loop: everything LogB does on a timer lives here.

use crate::db;
use crate::notify;
use crate::state::App;
use std::time::Duration;

/// How often the loop wakes. Both jobs below are cheap and idempotent, so a coarse tick is
/// enough -- the digest only has to land inside its hour, not on the minute.
const TICK: Duration = Duration::from_secs(60);
/// Expired sessions are swept at most this often; the table is tiny and the delete is indexed.
const PRUNE_EVERY: Duration = Duration::from_secs(3600);
/// How long a device may stay offline before an incremental pull is no longer possible and it
/// must re-bootstrap. Also how long a tombstone survives, since the two are the same guarantee
/// seen from either end.
const RETENTION_DAYS: i64 = 90;

/// Removes sessions whose expiry has passed. Returns how many went.
///
/// Logging in used to be the only thing that ever pruned, so an instance nobody signed into
/// -- or one used from a single long-lived session -- kept every expired row forever.
pub async fn prune_sessions(state: &App) -> Result<u64, crate::error::AppError> {
    let n = sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
        .bind(db::now())
        .execute(&state.db)
        .await?
        .rows_affected();
    Ok(n)
}

pub fn spawn(state: App) {
    tracing::info!(hour = state.config.notify_hour, timezone = %crate::db::timezone(), "reminder digest scheduler enabled");
    // `backup::tick` is `VACUUM INTO`, a SQLite mechanism -- calling it every tick against
    // PostgreSQL would mean running and failing every night instead of never running. Decided
    // once here, from the same `state.backend` the rest of the app already trusts, rather than
    // inside the loop, so the nightly job truly does not run rather than running and failing.
    let backup_enabled = state.backend == crate::dialect::Backend::Sqlite;
    if !backup_enabled {
        tracing::info!(
            "automatic backup is off: the database is PostgreSQL, and backing it up is the \
             operator's own responsibility"
        );
    }
    crate::telegram::spawn(state.clone());
    tokio::spawn(async move {
        let mut since_prune = PRUNE_EVERY;
        loop {
            tokio::time::sleep(TICK).await;
            since_prune += TICK;
            if since_prune >= PRUNE_EVERY {
                since_prune = Duration::ZERO;
                match prune_sessions(&state).await {
                    Ok(n) if n > 0 => tracing::debug!(sessions = n, "pruned expired sessions"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "session prune failed"),
                }
                match crate::sync::feed::purge(&state, RETENTION_DAYS).await {
                    Ok(n) if n > 0 => tracing::debug!(changes = n, "purged expired sync history"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "sync purge failed"),
                }
            }
            match notify::tick(&state, db::local_hour()).await {
                Ok(Some(d)) => {
                    tracing::info!(reminders = d.reminders.len(), "sent reminder digest")
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "reminder digest failed"),
            }
            if backup_enabled {
                match crate::backup::tick(&state, db::local_hour()).await {
                    Ok(Some(path)) => {
                        tracing::info!(path = %path.display(), "wrote database snapshot")
                    }
                    Ok(None) => {}
                    Err(e) => tracing::error!(error = %e, "database snapshot failed"),
                }
            }
        }
    });
}
