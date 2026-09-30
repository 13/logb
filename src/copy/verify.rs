//! Checking a copy: whether the destination holds what the source does, table by table.

use super::table::identifier;
use super::TABLES;
use crate::db::{self, BoxError};
use sqlx::{AnyPool, AssertSqlSafe, Column, Row};

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
/// `tests/it/copy.rs` is what actually proves it, table by table, across a change of engine.
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
pub(super) struct Shape {
    table: &'static str,
    /// The primary key worth summing, where the table has one that is a number.
    key: Option<&'static str>,
    /// Whether the table carries an `updated_at`.
    updated: bool,
}

/// What one table's contents are worth, reduced to three values.
#[derive(Debug, PartialEq)]
pub(super) struct Fingerprint {
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
pub(super) async fn shapes(conn: &mut sqlx::AnyConnection) -> Result<Vec<Shape>, BoxError> {
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
pub(super) async fn fingerprints(
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
pub(super) fn compare(
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
