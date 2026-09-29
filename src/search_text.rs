//! The stored, folded text `GET /search` matches against.
//!
//! Search used to load every live object and activity a user has and fold each field in Rust
//! on every request. It now matches one column, `search_text`, which holds exactly what that
//! loop computed: every searched field, folded by `domain::tags::fold` (NFD, combining marks
//! dropped, lower case), joined by [`SEPARATOR`]. The folding is still done here in Rust and
//! only here -- neither backend can fold accents in SQL, and the two would disagree if they
//! tried (see `dialect.rs`, item 1).
//!
//! Which fields are searched is decided in this file alone:
//!   - an object: `name`, `description` and each of its tags -- not `type`, which holds an
//!     identifier rather than words anyone typed (see `api::search`);
//!   - an activity: `title`, `notes`, `from_place`, `to_place` and each of its tags.
//!
//! The separator is what keeps "the term occurs inside ONE field" true: a term cannot contain
//! it (`api::search` answers nothing for one that does), so no match can straddle two fields.
//!
//! Every write path refreshes the column after it writes: REST create and update
//! (`api::objects`, `api::activities::write`), a sync `set` of a searched field
//! (`sync::apply`), and an import (`api::export::import`). A delete needs nothing -- search
//! filters on `deleted_at` -- and `--copy-to` copies the column like any other, which is right
//! because the folding is the same Rust on both sides. A path that forgets to refresh leaves a
//! stale or NULL column, which is a silent search miss; `tests/it/search.rs` has a test per path.
//!
//! [`backfill`] fills every NULL at startup, and clears and refills every row whenever
//! [`FOLD_VERSION`] changes -- so changing `fold`, or which fields are searched, is a matter of
//! bumping that constant.

use crate::domain::tags::fold;
use sqlx::{AnyConnection, AssertSqlSafe};

/// Bump this whenever `fold` or the list of searched fields changes: the next start then
/// recomputes every row's `search_text`.
pub const FOLD_VERSION: &str = "1";

/// The `settings` key holding the `FOLD_VERSION` the stored column was computed with.
const FOLD_VERSION_KEY: &str = "search_fold_version";

/// Between two fields in `search_text`. U+001F (unit separator): a control character nobody
/// types into a name or a note, and which `api::search` refuses to look for.
pub const SEPARATOR: char = '\u{1f}';

/// Rows folded per write transaction during [`backfill`], so the write lock is never held for
/// long even on a database with a hundred thousand activities.
const BATCH: i64 = 500;

fn join<'a>(fields: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for (i, field) in fields.into_iter().enumerate() {
        if i > 0 {
            out.push(SEPARATOR);
        }
        out.push_str(&fold(field));
    }
    out
}

/// The tags column decoded. Text that is not a JSON array contributes no tags rather than
/// failing the write: the column is written only by `domain::tags`, and search always treated
/// an undecodable value as "no tags" too.
fn tags(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

pub fn object_text(name: &str, description: &str, tags_json: &str) -> String {
    let tags = tags(tags_json);
    join([name, description].into_iter().chain(tags.iter().map(String::as_str)))
}

pub fn activity_text(
    title: &str,
    notes: &str,
    from_place: Option<&str>,
    to_place: Option<&str>,
    tags_json: &str,
) -> String {
    let tags = tags(tags_json);
    join(
        [Some(title), Some(notes), from_place, to_place]
            .into_iter()
            .flatten()
            .chain(tags.iter().map(String::as_str)),
    )
}

/// Which rows a refresh recomputes.
#[derive(Clone, Copy)]
pub enum Rows<'a> {
    /// One row, by id.
    Id(i64),
    /// One row, by `client_uuid` -- how a sync op names it.
    Uuid(&'a str),
    /// Every row of this user's not yet folded -- what an import leaves behind.
    UnfoldedOf(i64),
    /// Up to `BATCH` unfolded rows of anyone's -- one step of [`backfill`].
    Unfolded,
}

impl Rows<'_> {
    /// The `WHERE` clause, with the owner test spelled for the table: activities carry no
    /// `user_id` of their own.
    fn filter(&self, activities: bool) -> String {
        match self {
            Rows::Id(_) => "id = $1".into(),
            Rows::Uuid(_) => "client_uuid = $1".into(),
            Rows::UnfoldedOf(_) if activities => {
                "search_text IS NULL AND object_id IN (SELECT id FROM objects WHERE user_id = $1)"
                    .into()
            }
            Rows::UnfoldedOf(_) => "search_text IS NULL AND user_id = $1".into(),
            Rows::Unfolded => format!("search_text IS NULL ORDER BY id LIMIT {BATCH}"),
        }
    }
}

#[derive(sqlx::FromRow)]
struct ObjectFields {
    id: i64,
    name: String,
    description: String,
    tags: String,
    search_text: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ActivityFields {
    id: i64,
    title: String,
    notes: String,
    from_place: Option<String>,
    to_place: Option<String>,
    tags: String,
    search_text: Option<String>,
}

/// Recomputes `search_text` for the chosen objects from what is stored, and answers how many
/// rows it looked at. Called with the write transaction that just changed them.
pub async fn refresh_objects(conn: &mut AnyConnection, rows: Rows<'_>) -> Result<usize, sqlx::Error> {
    let sql = format!(
        "SELECT id, name, description, tags, search_text FROM objects WHERE {}",
        rows.filter(false)
    );
    let query = sqlx::query_as::<_, ObjectFields>(AssertSqlSafe(sql));
    let found = match rows {
        Rows::Id(id) | Rows::UnfoldedOf(id) => query.bind(id),
        Rows::Uuid(uuid) => query.bind(uuid),
        Rows::Unfolded => query,
    }
    .fetch_all(&mut *conn)
    .await?;
    let changed = found.iter().filter_map(|row| {
        let text = object_text(&row.name, &row.description, &row.tags);
        (row.search_text.as_deref() != Some(text.as_str())).then_some((row.id, text))
    });
    write(conn, "objects", changed.collect()).await?;
    Ok(found.len())
}

/// As [`refresh_objects`], for activities.
pub async fn refresh_activities(
    conn: &mut AnyConnection,
    rows: Rows<'_>,
) -> Result<usize, sqlx::Error> {
    let sql = format!(
        "SELECT id, title, notes, from_place, to_place, tags, search_text FROM activities WHERE {}",
        rows.filter(true)
    );
    let query = sqlx::query_as::<_, ActivityFields>(AssertSqlSafe(sql));
    let found = match rows {
        Rows::Id(id) | Rows::UnfoldedOf(id) => query.bind(id),
        Rows::Uuid(uuid) => query.bind(uuid),
        Rows::Unfolded => query,
    }
    .fetch_all(&mut *conn)
    .await?;
    let changed = found.iter().filter_map(|row| {
        let text = activity_text(
            &row.title,
            &row.notes,
            row.from_place.as_deref(),
            row.to_place.as_deref(),
            &row.tags,
        );
        (row.search_text.as_deref() != Some(text.as_str())).then_some((row.id, text))
    });
    write(conn, "activities", changed.collect()).await?;
    Ok(found.len())
}

/// Writes each `(id, text)`, a whole chunk per statement: an import or a backfill folds
/// hundreds of rows at once, and one `UPDATE` per row is one round trip per row on PostgreSQL.
/// Rows whose text did not change were already left out -- most updates change a field search
/// does not look at.
///
/// `UPDATE ... FROM (VALUES ...)` is spelled the same on both backends (SQLite since 3.33), and
/// both name a bare `VALUES` list's columns `column1`, `column2`.
async fn write(
    conn: &mut AnyConnection,
    table: &'static str,
    rows: Vec<(i64, String)>,
) -> Result<(), sqlx::Error> {
    for chunk in rows.chunks(BATCH as usize) {
        let values = (0..chunk.len())
            .map(|i| format!("(${}, ${})", 2 * i + 1, 2 * i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "UPDATE {table} SET search_text = v.column2 FROM (VALUES {values}) AS v \
             WHERE {table}.id = v.column1"
        );
        let mut query = sqlx::query(AssertSqlSafe(sql));
        for (id, text) in chunk {
            query = query.bind(*id).bind(text.as_str());
        }
        query.execute(&mut *conn).await?;
    }
    Ok(())
}

/// Whether a sync `set` of `field` changes what search matches, and so has to refresh.
pub fn is_searched(entity: crate::sync::Entity, field: &str) -> bool {
    use crate::sync::Entity;
    match entity {
        Entity::Object => matches!(field, "name" | "description" | "tags"),
        Entity::Activity => matches!(field, "title" | "notes" | "from_place" | "to_place" | "tags"),
        _ => false,
    }
}

/// Fills every NULL `search_text`, first clearing the whole column if it was computed with a
/// different [`FOLD_VERSION`]. Run once at startup, after the migrations.
///
/// Each batch is its own short write transaction, so a large backfill never holds the write
/// lock for more than one batch at a time. A backfill cut short by a restart simply continues:
/// the version is recorded before the refill, and the rows still NULL are the ones left to do.
pub async fn backfill(state: &crate::state::App) -> Result<(), crate::error::AppError> {
    let mut tx = crate::db::begin_write(state).await?;
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = $1")
        .bind(FOLD_VERSION_KEY)
        .fetch_optional(&mut *tx)
        .await?;
    if stored.as_deref() != Some(FOLD_VERSION) {
        for table in ["objects", "activities"] {
            sqlx::query(AssertSqlSafe(format!(
                "UPDATE {table} SET search_text = NULL WHERE search_text IS NOT NULL"
            )))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ($1, $2) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(FOLD_VERSION_KEY)
        .bind(FOLD_VERSION)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let mut filled = 0;
    for activities in [false, true] {
        loop {
            let mut tx = crate::db::begin_write(state).await?;
            let n = if activities {
                refresh_activities(&mut tx, Rows::Unfolded).await?
            } else {
                refresh_objects(&mut tx, Rows::Unfolded).await?
            };
            tx.commit().await?;
            filled += n;
            if n == 0 {
                break;
            }
        }
    }
    if filled > 0 {
        tracing::info!(rows = filled, version = FOLD_VERSION, "folded search text");
    }
    Ok(())
}

/// PostgreSQL only: trigram indexes on `search_text`, so a search for a term of three or more
/// characters reads the index instead of every row. With 20 000 activities that is about 10 ms
/// of scan against well under 1 ms for a rare term; shorter terms have no trigram to look up,
/// and the planner scans for them as before.
///
/// Created here rather than in a migration because `pg_trgm` is an extension, and creating one
/// can be refused -- a role without CREATE on the database, a server built without contrib. A
/// migration that failed would stop the server; here a refusal is logged once per start and
/// search keeps working as a plain scan, which it was already fast enough to be. Deliberately
/// absent from the migrations for the same reason `tests/it/schema_parity.rs` does not see it:
/// it is an optional accelerator, not part of the schema.
///
/// SQLite has no counterpart: its scan of one user's rows of one narrow column is already about
/// 10 ms at that size, and an FTS5 table would be a second copy of the text to keep in step.
pub async fn ensure_trigram_indexes(state: &crate::state::App) {
    if state.backend != crate::dialect::Backend::Postgres {
        return;
    }
    if let Err(e) = create_trigram_indexes(state).await {
        tracing::info!(
            error = %e,
            "no pg_trgm trigram index for search (the extension could not be created); \
             search scans instead, which is fine for a household's data"
        );
    }
}

async fn create_trigram_indexes(state: &crate::state::App) -> Result<(), crate::error::AppError> {
    let mut tx = crate::db::begin_write(state).await?;
    let installed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_extension WHERE extname = 'pg_trgm'")
            .fetch_one(&mut *tx)
            .await?;
    if installed == 0 {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_trgm").execute(&mut *tx).await?;
    }
    for table in ["objects", "activities"] {
        sqlx::query(AssertSqlSafe(format!(
            "CREATE INDEX IF NOT EXISTS idx_{table}_search_trgm ON {table} \
             USING gin (search_text gin_trgm_ops)"
        )))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_folded_and_kept_apart() {
        let s = SEPARATOR;
        assert_eq!(
            object_text("Golf", "Élan", r#"["Fahrräder","Winter"]"#),
            format!("golf{s}elan{s}fahrrader{s}winter")
        );
        assert_eq!(
            activity_text("Oil", "", None, Some("Bäckerei"), "[]"),
            format!("oil{s}{s}backerei")
        );
    }

    #[test]
    fn tags_that_do_not_decode_contribute_nothing() {
        assert_eq!(object_text("a", "b", "not json"), format!("a{}b", SEPARATOR));
    }
}
