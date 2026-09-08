//! The one background loop: everything logby does on a timer lives here.

use crate::db;
use crate::notify;
use crate::state::App;
use std::time::Duration;

/// How often the loop wakes. Both jobs below are cheap and idempotent, so a coarse tick is
/// enough -- the digest only has to land inside its hour, not on the minute.
const TICK: Duration = Duration::from_secs(60);
/// Expired sessions are swept at most this often; the table is tiny and the delete is indexed.
const PRUNE_EVERY: Duration = Duration::from_secs(3600);

/// Removes sessions whose expiry has passed. Returns how many went.
///
/// Logging in used to be the only thing that ever pruned, so an instance nobody signed into
/// -- or one used from a single long-lived session -- kept every expired row forever.
pub async fn prune_sessions(state: &App) -> Result<u64, crate::error::AppError> {
    let n = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(db::now())
        .execute(&state.db)
        .await?
        .rows_affected();
    Ok(n)
}

pub fn spawn(state: App) {
    if state.config.notify_url.is_some() {
        tracing::info!(hour = state.config.notify_hour, timezone = %state.config.timezone, "reminder digest enabled");
    }
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
            }
            match notify::tick(&state, db::local_hour()).await {
                Ok(Some(d)) => tracing::info!(reminders = d.reminders.len(), "sent reminder digest"),
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "reminder digest failed"),
            }
        }
    });
}
