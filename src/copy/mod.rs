//! `logb --copy-to <url>`: move a database to another backend, once.
//!
//! Identifiers are preserved rather than reassigned. Every foreign key in this schema is an
//! `id`, and every device that has ever synced holds `client_uuid`s that resolve to them, so a
//! copy that renumbered anything would silently detach half the data from the other half.

use crate::db::{self, BoxError};
use crate::dialect::Backend;
use sqlx::pool::PoolConnection;
use sqlx::{Any, AnyPool, AssertSqlSafe};

mod table;
mod verify;

use table::{copy_table, resync_identity_sequences};
use verify::{compare, fingerprints, shapes};
pub use verify::verify;

/// The tables, in an order where every foreign key's target is written before it is.
///
/// Public because it is the copy's one hand-written list, and a list nothing checks goes stale
/// silently: a table missing from it is copied nowhere *and* invisible to the verification
/// below, which reads the same list. `tests/it/schema_parity.rs` asserts it names exactly the
/// tables the schema has.
pub const TABLES: [&str; 18] = [
    "users",
    "notification_deliveries",
    "object_types",
    "telegram_credentials",
    "telegram_connections",
    "telegram_link_codes",
    "push_subscriptions",
    "settings",
    "api_tokens",
    "pairing_codes",
    "sessions",
    "objects",
    "activities",
    "files",
    "attachments",
    "reminders",
    "changes",
    "field_clock",
];

/// Shown when something else still holds the source. Copying out from under a running server
/// captures a moving target: the later tables would be read after the earlier ones changed, and
/// the copy would be internally inconsistent while reporting success.
const IN_USE: &str = "the source database is still in use by something else -- stop the server \
                      (and anything else connected to it) before copying, so the copy is taken \
                      from a database nobody is writing to";

/// Shown when the destination is already somebody's database.
///
/// The question actually asked is whether it has users, and the message says so rather than
/// claiming a general emptiness check: `users` is the table every LogB instance fills first and
/// the only one that means "this database belongs to someone". Rows anywhere else, in a
/// database with no users, are not refused here -- the verification below finds the destination
/// holding rows the source does not have, names the table, and rolls the whole copy back. That
/// is a worse message for a case an operator has to work at to produce, and it is deliberately
/// not worth a second query per table on every copy to improve it.
///
/// There is deliberately no flag to override this. Both databases number their rows from 1, so
/// copying into a populated destination collides on the first primary key it writes; and even
/// where it did not, the verification below would find the destination holding rows the source
/// does not have and roll the whole copy back. A merge is a different operation from a copy,
/// and this command does not do it.
const NOT_EMPTY: &str = "the destination database already holds data (its `users` table is not \
                         empty). Copying into it would put two histories in one database.";

#[derive(Debug)]
pub struct Report {
    /// Each table and the number of rows copied, in the order they were copied.
    pub tables: Vec<(String, i64)>,
    /// The identity the destination now advertises. Every device re-bootstraps against it.
    pub epoch: String,
}

/// Copies `source_url`'s database into `dest_url`'s, row for row and id for id.
///
/// One of two entry points into `copy_from`, which is the whole of the copy. They differ in
/// nothing but how the source is made safe to read from: this one refuses a source anything
/// else still holds (`claim_source`), because a command run from a shell cannot stop the
/// writers itself. `run_live` is the other, for the server copying its own database.
///
/// Both pools are closed on every path, success or not: on SQLite the source is held in
/// exclusive locking mode for the duration, and on PostgreSQL the destination's write
/// transaction holds this application's advisory lock -- neither should outlive the call.
pub async fn run(source_url: &str, dest_url: &str) -> Result<Report, BoxError> {
    if source_url == dest_url {
        return Err("the source and the destination are the same database".into());
    }
    let source = db::connect_existing(source_url).await?;
    let dest = match db::connect(dest_url).await {
        Ok(dest) => dest,
        Err(e) => {
            source.close().await;
            return Err(e);
        }
    };
    let result = copy(
        &source,
        Backend::of(source_url),
        &dest,
        Backend::of(dest_url),
    )
    .await;
    source.close().await;
    dest.close().await;
    result
}

/// Copies the database this server is running on into `dest_url`'s, without stopping it.
///
/// The other entry point into `copy_from`, and the same copy in every respect but one: where
/// `run` makes the source safe to read by refusing a database anything else holds, this one
/// makes it safe by *being* the writer -- it reads inside `db::begin_write`, the transaction
/// every write in this application goes through, so no write can land on the source while the
/// copy is in flight and the eleven tables are still read from one point in time. `run`'s
/// refusal is unusable here because the process asking for the copy is precisely the server
/// that would be refused.
///
/// Takes the state rather than a URL, and hands the transaction down: on PostgreSQL that
/// transaction holds this application's advisory lock, so anything below that acquired a
/// second connection from this pool would block against it and hang rather than fail. The
/// destination is a different database, so its own pool is free to open.
pub async fn run_live(state: &crate::state::App, dest_url: &str) -> Result<Report, BoxError> {
    // Opened before the write lock is taken: `db::connect` migrates the destination, and every
    // write to this server is stalled for as long as the transaction below is open.
    let dest = db::connect(dest_url).await?;
    let mut src = match db::begin_write(state).await {
        Ok(src) => src,
        Err(e) => {
            dest.close().await;
            return Err(e.into());
        }
    };
    let result = copy_from(&mut src, &dest, Backend::of(dest_url)).await;
    // The source was only ever read. Let go of it explicitly rather than leaving the rollback
    // to a drop, so the server is taking writes again before this returns.
    let _ = src.rollback().await;
    dest.close().await;
    result
}

/// `run`'s half: claim the source, copy, let it go.
async fn copy(
    source: &AnyPool,
    source_backend: Backend,
    dest: &AnyPool,
    dest_backend: Backend,
) -> Result<Report, BoxError> {
    // Claimed before the destination is touched at all, so a refusal leaves a destination that
    // was never written to.
    let mut src = claim_source(source, source_backend).await?;
    let report = copy_from(&mut src, dest, dest_backend).await?;

    // The source was only ever read. Let go of it explicitly rather than leaving the rollback
    // to a drop, so the lock is gone before this returns.
    let _ = sqlx::raw_sql(AssertSqlSafe("ROLLBACK"))
        .execute(&mut *src)
        .await;

    Ok(report)
}

/// The copy itself, from a source that is already open and already safe to read.
///
/// `src` is a connection with a transaction open on it -- how that transaction came to be is
/// the only thing `run` and `run_live` disagree about. It is a `&mut AnyConnection` rather than
/// a pool precisely so that neither entry point can hand this a pool to acquire from: under
/// `run_live` the caller's transaction holds this application's advisory lock, and a second
/// connection taken here would block against it and hang.
async fn copy_from(
    src: &mut sqlx::AnyConnection,
    dest: &AnyPool,
    dest_backend: Backend,
) -> Result<Report, BoxError> {
    let mut tx = db::begin_write_on(dest, dest_backend).await?;
    // Everything below runs on `tx`, never on `dest` itself. On PostgreSQL that transaction
    // holds the application's advisory lock, so a helper that opened a connection of its own
    // here would block against it and hang rather than fail.
    // Whether the destination is somebody's database, which is what `users` answers -- not
    // whether it is empty in general. See `NOT_EMPTY` for what happens to the rest.
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut *tx)
        .await?;
    if users > 0 {
        return Err(NOT_EMPTY.into());
    }

    // `db::connect` has just migrated the destination, and `db::seed_settings` writes the
    // `sync_epoch` and `currency` rows on first open. They are the only rows an "empty"
    // destination can hold, and the source's own settings are what should be there instead --
    // so they go, rather than colliding on the primary key a moment from now.
    sqlx::query("DELETE FROM settings")
        .execute(&mut *tx)
        .await?;

    let mut tables = Vec::with_capacity(TABLES.len());
    for table in TABLES {
        let rows = copy_table(&mut *src, &mut tx, table).await?;
        tables.push((table.to_string(), rows));
    }

    // Before the commit, deliberately. A copy that lost rows has to roll back entirely rather
    // than leave a half-copied database that looks finished: the next thing an operator does
    // after a successful copy is delete the source. It runs here, before the two writes below
    // deliberately change the destination, so that what is compared is purely what was copied.
    //
    // Both sides are read through handles that are already open -- the source's read
    // transaction and the destination's write transaction. On PostgreSQL the latter holds this
    // application's advisory lock, so a helper that opened a connection of its own here would
    // block against it and hang rather than fail.
    let shapes = shapes(&mut *src).await?;
    let expected = fingerprints(&mut *src, &shapes).await?;
    let found = fingerprints(&mut tx, &shapes).await?;
    compare(&shapes, &expected, &found)?;

    // PostgreSQL's identity columns hand out numbers from a sequence that an explicit `id` does
    // not advance. Left alone, the first row the copied database wrote itself would collide
    // with the first row it was given.
    if dest_backend == Backend::Postgres {
        resync_identity_sequences(&mut tx).await?;
    }

    // Last, and inside the transaction: `changes` and `field_clock` are copied verbatim, so
    // the cursor numbers survive -- but a device resuming mid-stream against a freshly copied
    // database is not a risk worth taking for the one re-bootstrap it saves.
    let epoch = crate::sync::epoch::rotate(&mut *tx).await?;
    tx.commit().await?;

    // The source is left exactly as it was handed over, transaction still open: ending it is
    // the caller's half of the job, and the two callers end it differently.
    Ok(Report { tables, epoch })
}

/// Takes the source for the duration of the copy, and refuses if anything else holds it.
///
/// The connection comes back with a read transaction open on it, so every table is read from
/// one point in time rather than eleven.
async fn claim_source(pool: &AnyPool, backend: Backend) -> Result<PoolConnection<Any>, BoxError> {
    let mut conn = pool.acquire().await?;
    match backend {
        Backend::Sqlite => {
            // `BEGIN IMMEDIATE` on its own is not the test the obvious reading suggests: an
            // idle server holds no write lock under WAL, so it would succeed with the server
            // running. `locking_mode = EXCLUSIVE` is what actually asks for the database to
            // itself -- in WAL mode it takes the shared-memory locks, which fails with
            // SQLITE_BUSY exactly when another connection has the database open.
            //
            // The busy timeout is lowered first because `connect_existing` sets thirty
            // seconds: an operator who forgot to stop the server should be told now, not after
            // half a minute of silence. Nothing contends with us once the lock is ours.
            sqlx::raw_sql(AssertSqlSafe("PRAGMA busy_timeout = 250"))
                .execute(&mut *conn)
                .await?;
            sqlx::raw_sql(AssertSqlSafe("PRAGMA locking_mode = EXCLUSIVE"))
                .execute(&mut *conn)
                .await?;
            if let Err(e) = sqlx::raw_sql(AssertSqlSafe("BEGIN IMMEDIATE"))
                .execute(&mut *conn)
                .await
            {
                return Err(format!("{IN_USE} ({e})").into());
            }
        }
        Backend::Postgres => {
            // The snapshot is taken first, so the count below is asked from inside the
            // transaction the rows will be read in.
            sqlx::raw_sql(AssertSqlSafe(
                "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            ))
            .execute(&mut *conn)
            .await?;
            let others: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity \
                 WHERE datname = current_database() AND pid <> pg_backend_pid()",
            )
            .fetch_one(&mut *conn)
            .await?;
            if others > 0 {
                let plural = if others == 1 {
                    "connection"
                } else {
                    "connections"
                };
                return Err(format!("{IN_USE} ({others} other {plural} to it)").into());
            }
        }
    }
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use table::identifier;

    /// The order, and that nothing is listed twice: a foreign key's target has to be written
    /// before the row pointing at it, and a table copied twice would collide on its own keys.
    ///
    /// That the list is *complete* is not checkable from here -- it takes a real database to
    /// say what tables the schema has -- and is asserted in `tests/it/schema_parity.rs`, against
    /// both backends' catalogues.
    #[test]
    fn the_table_list_has_no_duplicates_and_puts_targets_before_their_references() {
        let mut seen: Vec<&str> = Vec::new();
        for table in TABLES {
            assert!(!seen.contains(&table), "{table} is listed twice");
            seen.push(table);
        }
        for (table, target) in [
            ("sessions", "users"),
            ("object_types", "users"),
            ("push_subscriptions", "users"),
            ("api_tokens", "users"),
            ("objects", "users"),
            ("activities", "objects"),
            ("files", "users"),
            ("attachments", "objects"),
            ("attachments", "activities"),
            ("attachments", "files"),
            ("reminders", "objects"),
            ("reminders", "activities"),
            ("changes", "users"),
        ] {
            let at = |name: &str| TABLES.iter().position(|t| *t == name).unwrap();
            assert!(
                at(target) < at(table),
                "{target} must be copied before {table}"
            );
        }
    }

    /// Column names go into statement text, so anything that is not a bare identifier has to be
    /// refused rather than quoted-and-hoped.
    #[test]
    fn only_bare_identifiers_are_allowed_into_a_statement() {
        assert_eq!(identifier("client_uuid").unwrap(), "client_uuid");
        assert_eq!(identifier("sha256").unwrap(), "sha256");
        for bad in ["", "drop table users", "a\"b", "a;b", "a-b", "1st"] {
            assert!(
                identifier(bad).is_err(),
                "{bad:?} should not be allowed into a statement"
            );
        }
    }
}
