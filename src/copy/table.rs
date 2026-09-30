//! One table from the source into the destination, and the PostgreSQL sequences after.

use super::TABLES;
use crate::db::BoxError;
use sqlx::any::AnyTypeInfoKind;
use sqlx::{Any, AssertSqlSafe, Column, Row, ValueRef};

/// Reads one table out of the source and writes it into the destination, returning the row count.
///
/// The column names come from the rows themselves rather than from eleven hard-coded lists
/// here: `tests/it/schema_parity.rs` already guarantees both backends name their columns the same,
/// and a list written out here would drift from the schema the first time one changed.
pub(super) async fn copy_table(
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
pub(super) async fn resync_identity_sequences(
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
pub(super) fn identifier(name: &str) -> Result<&str, BoxError> {
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

