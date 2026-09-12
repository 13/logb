//! The identity of the database a cursor refers to. See `migrations/sqlite/0008_sync_epoch.sql`.

use crate::error::AppError;
use rand::RngExt;

/// A new epoch value: 16 random bytes as lowercase hex.
///
/// Generated in Rust rather than by the database. SQLite's migrations minted it with
/// `lower(hex(randomblob(16)))`, which PostgreSQL has no equivalent of, and the seed had to
/// move into the app anyway (see `db::seed_settings`) because the PostgreSQL schema seeds
/// nothing. The format is deliberately the one those migrations produced, so an epoch is the
/// same shape whether the database was created before or after this change.
///
/// The value is opaque -- a client only ever compares it for equality -- so all that is asked
/// of it is that a fresh one never collide with a previous one.
pub fn fresh() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill(&mut bytes);
    hex::encode(bytes)
}

pub async fn current(db: &sqlx::AnyPool) -> Result<String, AppError> {
    let (v,): (String,) = sqlx::query_as("SELECT value FROM settings WHERE key = 'sync_epoch'")
        .fetch_one(db)
        .await?;
    Ok(v)
}

/// Gives the database a new identity, invalidating every cursor held anywhere.
///
/// Called by `--restore`. It is deliberately not reachable over HTTP: rotating without
/// replacing the data would send every device to a bootstrap for no reason.
///
/// An upsert, not a plain `UPDATE`: a bare `UPDATE ... WHERE key = 'sync_epoch'` matches zero
/// rows if that row is ever absent (a restored snapshot old enough to predate it, or the row
/// having been lost some other way), and would still report the freshly minted value as the new
/// epoch while the database went on advertising its old identity -- or none at all, which then
/// makes every pull 500 through `current`'s `fetch_one`. The `rows_affected` assertion is the
/// invariant this relies on: for a single primary-keyed row, insert-or-update always affects
/// exactly one row, so anything else means the write did not do what it claims.
/// Takes any executor rather than a pool: `copy::run` rotates from inside the destination's
/// write transaction, and on PostgreSQL that transaction holds this application's advisory
/// lock -- a pool handed in there would open a connection of its own, block against that lock,
/// and hang rather than fail. `&AnyPool` still satisfies this, so `--restore` is unchanged.
pub async fn rotate<'e, E>(db: E) -> Result<String, AppError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    let fresh = fresh();
    let result = sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('sync_epoch', $1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(&fresh)
        .execute(db)
        .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Internal(format!(
            "sync epoch rotation affected {} rows, not 1 -- the epoch was not reliably set",
            result.rows_affected()
        )));
    }
    Ok(fresh)
}
