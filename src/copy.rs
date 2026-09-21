//! `logb --copy-to <url>`: move a database to another backend, once.
//!
//! Identifiers are preserved rather than reassigned. Every foreign key in this schema is an
//! `id`, and every device that has ever synced holds `client_uuid`s that resolve to them, so a
//! copy that renumbered anything would silently detach half the data from the other half.

use crate::db::{self, BoxError};
use crate::dialect::Backend;
use sqlx::any::AnyTypeInfoKind;
use sqlx::pool::PoolConnection;
use sqlx::{Any, AnyPool, AssertSqlSafe, Column, Row, ValueRef};

/// The tables, in an order where every foreign key's target is written before it is.
///
/// Public because it is the copy's one hand-written list, and a list nothing checks goes stale
/// silently: a table missing from it is copied nowhere *and* invisible to the verification
/// below, which reads the same list. `tests/schema_parity.rs` asserts it names exactly the
/// tables the schema has.
pub const TABLES: [&str; 16] = [
    "users",
    "object_types",
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
/// Takes the pool the server already holds rather than a URL, and hands the transaction down:
/// on PostgreSQL that transaction holds this application's advisory lock, so anything below
/// that acquired a second connection from this pool would block against it and hang rather
/// than fail. The destination is a different database, so its own pool is free to open.
pub async fn run_live(db: &AnyPool, backend: Backend, dest_url: &str) -> Result<Report, BoxError> {
    // Opened before the write lock is taken: `db::connect` migrates the destination, and every
    // write to this server is stalled for as long as the transaction below is open.
    let dest = db::connect(dest_url).await?;
    let mut src = match db::begin_write(db, backend).await {
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
    let mut tx = db::begin_write(dest, dest_backend).await?;
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

/// Checks that the destination holds what the source does, table by table.
///
/// `run` calls this on the transaction it is about to commit; this entry point opens its own
/// connections, for asking the same question of a copy that has already been made.
///
/// What is compared is the row count, and -- where the table has the columns -- the sum of its
/// primary keys and the newest `updated_at`. It is a cheap check rather than a byte-for-byte
/// one: it catches a table that lost or gained rows, and one whose ids are not the ids the
/// source had -- a destination that renumbered from 1, say, which is the way a copy plausibly
/// loses identifiers.
///
/// What a sum cannot see is a *permutation*: the same ids dealt out to different rows sum to
/// the same number. Nothing in `copy_table` can produce that -- every row is written with the
/// id it was read with, in one statement -- and the value-by-value comparison in
/// `tests/copy.rs` is what actually proves it, table by table, across a change of engine.
/// Column values this does not read at all are the copy's own `INSERT ... SELECT *` round
/// trip, which either works for every row or none.
pub async fn verify(source_url: &str, dest_url: &str) -> Result<(), BoxError> {
    let source = db::connect_existing(source_url).await?;
    let dest = match db::connect_existing(dest_url).await {
        Ok(dest) => dest,
        Err(e) => {
            source.close().await;
            return Err(e);
        }
    };
    let result = compare_databases(&source, &dest).await;
    source.close().await;
    dest.close().await;
    result
}

async fn compare_databases(source: &AnyPool, dest: &AnyPool) -> Result<(), BoxError> {
    let mut src = source.acquire().await?;
    let mut dst = dest.acquire().await?;
    let shapes = shapes(&mut src).await?;
    let expected = fingerprints(&mut src, &shapes).await?;
    let found = fingerprints(&mut dst, &shapes).await?;
    compare(&shapes, &expected, &found)
}

/// What one table can be compared on.
struct Shape {
    table: &'static str,
    /// The primary key worth summing, where the table has one that is a number.
    key: Option<&'static str>,
    /// Whether the table carries an `updated_at`.
    updated: bool,
}

/// What one table's contents are worth, reduced to three values.
#[derive(Debug, PartialEq)]
struct Fingerprint {
    rows: i64,
    keys: i64,
    updated: String,
}

impl Fingerprint {
    /// The parts that were actually compared, for an error an operator can act on.
    fn describe(&self, shape: &Shape) -> String {
        let plural = if self.rows == 1 { "row" } else { "rows" };
        let mut parts = vec![format!("{} {plural}", self.rows)];
        if let Some(key) = shape.key {
            parts.push(format!("{} summing to {}", key, self.keys));
        }
        if shape.updated {
            parts.push(format!("newest updated_at {:?}", self.updated));
        }
        parts.join(", ")
    }
}

/// Learns what each table can be compared on from a row of it, rather than from a list written
/// out here -- the same reason `copy_table` takes its column names from the rows it read. A
/// table with no rows to learn from is compared on its count alone, which is all a count of
/// zero needs: a destination that has rows where the source has none differs on that count.
async fn shapes(conn: &mut sqlx::AnyConnection) -> Result<Vec<Shape>, BoxError> {
    let mut shapes = Vec::with_capacity(TABLES.len());
    for table in TABLES {
        let select = format!("SELECT * FROM {table} LIMIT 1");
        let row = sqlx::query(AssertSqlSafe(select))
            .fetch_optional(&mut *conn)
            .await?;
        let names: Vec<&str> = row
            .iter()
            .flat_map(|row| row.columns())
            .map(|column| column.name())
            .collect();
        let has = |name: &str| names.contains(&name);
        // `changes` numbers its rows `seq`; everything else that numbers them at all uses `id`.
        let key = ["id", "seq"].into_iter().find(|name| has(name));
        shapes.push(Shape {
            table,
            key,
            updated: has("updated_at"),
        });
    }
    Ok(shapes)
}

/// Reduces every table to its fingerprint, through one connection.
///
/// The shapes are worked out once, from the source, and used for both sides: the two
/// fingerprints have to be answers to the same question to be comparable at all.
async fn fingerprints(
    conn: &mut sqlx::AnyConnection,
    shapes: &[Shape],
) -> Result<Vec<Fingerprint>, BoxError> {
    let mut out = Vec::with_capacity(shapes.len());
    for shape in shapes {
        // The casts are not decoration. PostgreSQL sums a `bigint` into a `numeric`, which the
        // `Any` driver cannot decode, and an untyped literal standing in for an absent column
        // would come back as whatever each backend guessed.
        let keys = match shape.key {
            Some(key) => format!("CAST(COALESCE(SUM({}), 0) AS BIGINT)", identifier(key)?),
            None => "CAST(0 AS BIGINT)".to_string(),
        };
        let updated = match shape.updated {
            true => "COALESCE(MAX(updated_at), '')",
            false => "CAST('' AS TEXT)",
        };
        let sql = format!("SELECT COUNT(*), {keys}, {updated} FROM {}", shape.table);
        let (rows, keys, updated) = sqlx::query_as::<_, (i64, i64, String)>(AssertSqlSafe(sql))
            .fetch_one(&mut *conn)
            .await?;
        out.push(Fingerprint {
            rows,
            keys,
            updated,
        });
    }
    Ok(out)
}

/// The first table that differs, named, with both sides' values.
fn compare(
    shapes: &[Shape],
    expected: &[Fingerprint],
    found: &[Fingerprint],
) -> Result<(), BoxError> {
    for ((shape, want), got) in shapes.iter().zip(expected).zip(found) {
        if want != got {
            return Err(format!(
                "the copy did not arrive intact: {} differs -- the source has {}, \
                 the destination has {}",
                shape.table,
                want.describe(shape),
                got.describe(shape)
            )
            .into());
        }
    }
    Ok(())
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

/// Reads one table out of the source and writes it into the destination, returning the row count.
///
/// The column names come from the rows themselves rather than from eleven hard-coded lists
/// here: `tests/schema_parity.rs` already guarantees both backends name their columns the same,
/// and a list written out here would drift from the schema the first time one changed.
async fn copy_table(
    src: &mut sqlx::AnyConnection,
    tx: &mut sqlx::Transaction<'static, Any>,
    table: &str,
) -> Result<i64, BoxError> {
    let select = format!("SELECT * FROM {table}");
    let rows = sqlx::query(AssertSqlSafe(select))
        .fetch_all(&mut *src)
        .await?;
    // `TABLES` puts every foreign key's target table before the table referencing it, which is
    // the whole of the ordering problem for ten of the eleven. `objects` points at itself, so
    // its rows also have to be ordered against each other -- see `parents_before_children`.
    let rows = if table == "objects" {
        parents_before_children(rows)?
    } else {
        rows
    };
    let mut copied = 0i64;
    for row in &rows {
        let mut names = Vec::with_capacity(row.columns().len());
        let mut placeholders = Vec::with_capacity(row.columns().len());
        let mut values = Vec::with_capacity(row.columns().len());
        for column in row.columns() {
            let i = column.ordinal();
            names.push(identifier(column.name())?);
            // The column's *value* type, not its declared one: SQLite stores types per value,
            // and this is the shape the row actually arrived in.
            let kind = row.try_get_raw(i)?.type_info().kind();
            if kind == AnyTypeInfoKind::Null {
                // A null is written as the literal `NULL` rather than bound as a parameter. A
                // bound null carries a type, and the only type available for one here is the
                // source backend's idea of it -- SQLite reports no type at all -- which
                // PostgreSQL would then reject against the column it is being put in.
                placeholders.push("NULL".to_string());
                continue;
            }
            values.push((i, kind));
            placeholders.push(format!("${}", values.len()));
        }
        let insert = format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            names.join(", "),
            placeholders.join(", ")
        );
        let mut query = sqlx::query(AssertSqlSafe(insert));
        for (i, kind) in values {
            query = match kind {
                AnyTypeInfoKind::Bool => query.bind(row.try_get::<bool, _>(i)?),
                AnyTypeInfoKind::SmallInt => query.bind(row.try_get::<i16, _>(i)?),
                AnyTypeInfoKind::Integer => query.bind(row.try_get::<i32, _>(i)?),
                AnyTypeInfoKind::BigInt => query.bind(row.try_get::<i64, _>(i)?),
                AnyTypeInfoKind::Real => query.bind(row.try_get::<f32, _>(i)?),
                AnyTypeInfoKind::Double => query.bind(row.try_get::<f64, _>(i)?),
                AnyTypeInfoKind::Text => query.bind(row.try_get::<String, _>(i)?),
                AnyTypeInfoKind::Blob => query.bind(row.try_get::<Vec<u8>, _>(i)?),
                AnyTypeInfoKind::Null => unreachable!("a null column was written as a literal"),
            };
        }
        query.execute(&mut **tx).await?;
        copied += 1;
    }
    Ok(copied)
}

/// Orders `objects` so that every row is written after the row its `parent_id` points at.
///
/// `objects.parent_id` is the schema's one self-referencing foreign key -- nothing else in
/// `migrations/postgres/0001_schema.sql` points at its own table -- and PostgreSQL checks a
/// foreign key per statement, not at commit: the reference is not declared `DEFERRABLE`. The
/// rows arrive in whatever order the source hands them over, which on SQLite is rowid order,
/// which is creation order -- and creation order has nothing to do with tree order. Create a
/// bike, create a garage afterwards, move the bike into the garage, and the child holds the
/// lower id. Written in that order the child's `INSERT` names a parent PostgreSQL has not seen
/// yet, and the whole copy aborts partway through.
///
/// A breadth-first walk outwards from the roots, rather than a sort: the tree has no bounded
/// depth. Rows that no such walk reaches -- a `parent_id` pointing into a cycle, which no write
/// path in this application can produce -- are appended in the order they were read rather than
/// dropped, so a source in that state is still copied as faithfully as it can be and the
/// database, not this function, decides what it thinks of it.
fn parents_before_children(
    rows: Vec<sqlx::any::AnyRow>,
) -> Result<Vec<sqlx::any::AnyRow>, BoxError> {
    use std::collections::{HashMap, VecDeque};

    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        ids.push((
            whole_number(row, "id")?,
            optional_whole_number(row, "parent_id")?,
        ));
    }
    // Which row holds each id, so a `parent_id` can be turned into the position that has to be
    // written first.
    let at: HashMap<i64, usize> = ids
        .iter()
        .enumerate()
        .filter_map(|(i, (id, _))| id.map(|id| (id, i)))
        .collect();

    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, (_, parent)) in ids.iter().enumerate() {
        match parent.and_then(|parent| at.get(&parent)) {
            // A parent this table holds: this row waits until that one has been written.
            Some(&parent) if parent != i => children.entry(parent).or_default().push(i),
            // No parent at all, or one that is not in this table -- nothing here can be written
            // before it, so it goes in the first wave.
            _ => queue.push_back(i),
        }
    }

    let mut order = Vec::with_capacity(rows.len());
    let mut written = vec![false; rows.len()];
    while let Some(i) = queue.pop_front() {
        if std::mem::replace(&mut written[i], true) {
            continue;
        }
        order.push(i);
        for child in children.get(&i).into_iter().flatten() {
            queue.push_back(*child);
        }
    }
    order.extend((0..rows.len()).filter(|i| !written[*i]));

    let mut rows: Vec<Option<sqlx::any::AnyRow>> = rows.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|i| {
            rows[i]
                .take()
                .expect("every position is ordered exactly once")
        })
        .collect())
}

/// `whole_number`, but a column the source table does not have at all also reads as `None`.
///
/// Only `parent_id` is read this way, and only because the source is opened by
/// `db::connect_existing`, which deliberately does not migrate it: an old backup, or the
/// database of a previous release being copied by a new binary, predates
/// `0010_object_hierarchy.sql` and has no `parent_id` column. `copy_table` already copes --
/// it takes its column list from the rows it actually read, so a narrower source is written
/// narrower -- and this is the one place that named a column instead of reading what was there,
/// turning a copy that used to work into a raw `ColumnNotFound` naming nothing an operator
/// could act on. A table with no hierarchy has every row a root, which is what `None` says, and
/// the walk below then leaves the rows in the order they were read.
fn optional_whole_number(row: &sqlx::any::AnyRow, name: &str) -> Result<Option<i64>, BoxError> {
    if row.try_column(name).is_err() {
        return Ok(None);
    }
    whole_number(row, name)
}

/// One integer column of a row, as an `i64`, or `None` where it is NULL.
///
/// The value's own type rather than the column's declared one, for the same reason
/// `copy_table` reads it that way: SQLite carries a type per value, and the two backends do not
/// report the same width for the same column.
fn whole_number(row: &sqlx::any::AnyRow, name: &str) -> Result<Option<i64>, BoxError> {
    let value = row.try_get_raw(name)?;
    if value.is_null() {
        return Ok(None);
    }
    let number = match value.type_info().kind() {
        AnyTypeInfoKind::SmallInt => i64::from(row.try_get::<i16, _>(name)?),
        AnyTypeInfoKind::Integer => i64::from(row.try_get::<i32, _>(name)?),
        AnyTypeInfoKind::BigInt => row.try_get::<i64, _>(name)?,
        kind => {
            return Err(format!(
                "the source database's objects.{name} holds {kind:?}, not a number"
            )
            .into())
        }
    };
    Ok(Some(number))
}

/// Moves each identity sequence past the ids that were just written into it.
///
/// Which columns those are is read from the destination rather than listed here, for the same
/// reason the column names are: a list would drift. PostgreSQL only -- SQLite's `AUTOINCREMENT`
/// counter follows the largest rowid inserted, so it needs nothing.
async fn resync_identity_sequences(
    tx: &mut sqlx::Transaction<'static, Any>,
) -> Result<(), BoxError> {
    // `table_name` and `column_name` are PostgreSQL's `information_schema.sql_identifier`
    // type, which the `Any` driver cannot decode -- without the casts this query fails.
    let identity: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text FROM information_schema.columns \
         WHERE table_schema = current_schema() AND is_identity = 'YES'",
    )
    .fetch_all(&mut **tx)
    .await?;
    for (table, column) in identity {
        if !TABLES.contains(&table.as_str()) {
            continue;
        }
        let column = identifier(&column)?;
        // `setval(seq, 1, false)` for an empty table hands out 1 next; `setval(seq, max, true)`
        // for a populated one hands out max + 1. Ids are positive, so the `> 0` picks between
        // them.
        let sql = format!(
            "SELECT setval(pg_get_serial_sequence('{table}', '{column}'), \
             GREATEST(COALESCE((SELECT max({column}) FROM {table}), 0), 1), \
             COALESCE((SELECT max({column}) FROM {table}), 0) > 0)"
        );
        sqlx::query(AssertSqlSafe(sql)).execute(&mut **tx).await?;
    }
    Ok(())
}

/// A name that is safe to write into a statement, because it is only letters, digits and
/// underscores.
///
/// Column names arrive from whatever database the source happens to be, and they are the one
/// part of these statements that is not a literal in this file. Refusing anything else is
/// cheaper than reasoning about how each backend quotes.
fn identifier(name: &str) -> Result<&str, BoxError> {
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    if plain {
        Ok(name)
    } else {
        Err(format!(
            "the source database has a column named {name:?}, which is not a plain identifier"
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The order, and that nothing is listed twice: a foreign key's target has to be written
    /// before the row pointing at it, and a table copied twice would collide on its own keys.
    ///
    /// That the list is *complete* is not checkable from here -- it takes a real database to
    /// say what tables the schema has -- and is asserted in `tests/schema_parity.rs`, against
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
