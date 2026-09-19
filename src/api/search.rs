use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<App> {
    Router::new().route("/search", get(search))
}

const DEFAULT_LIMIT: i64 = 25;
const MAX_LIMIT: i64 = 100;

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// An activity hit carries its object's name so the result list is readable without a
/// second request per row.
#[derive(Serialize, sqlx::FromRow)]
pub struct ActivityHit {
    pub id: i64,
    pub object_id: i64,
    pub object_name: String,
    pub date: String,
    pub category: String,
    pub title: String,
    pub notes: String,
    pub counter_value: Option<i64>,
    pub cost_cents: Option<i64>,
    pub weight_grams: Option<i64>,
    #[serde(serialize_with = "crate::domain::tags::serialize_json_text")]
    pub tags: String,
    /// A trip's places, so a hit on one of them is readable in the result list without a second
    /// request -- see the `{like}` match on both, below.
    pub from_place: Option<String>,
    pub to_place: Option<String>,
}

/// An object hit carries its parent's name for the same reason an activity hit carries its
/// object's: a list of four things called "Filter" is unreadable until each one says which
/// object it lives in. `None` is a root object, which has no parent to name.
#[derive(Serialize, sqlx::FromRow)]
pub struct ObjectHit {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub type_: String,
    pub counter_unit: Option<String>,
    pub fuel_unit: Option<String>,
    pub description: String,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    pub archived_at: Option<String>,
    pub cover_attachment_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub parent_name: Option<String>,
    #[serde(serialize_with = "crate::domain::tags::serialize_json_text")]
    pub tags: String,
}

#[derive(Serialize)]
pub struct SearchResults {
    pub objects: Vec<ObjectHit>,
    pub activities: Vec<ActivityHit>,
}

/// Wraps `q` in `%...%` after neutralising the LIKE wildcards, so a query containing `%` or
/// `_` matches those characters literally instead of turning into a match-everything pattern.
/// Pairs with `ESCAPE '\'` in the statements below.
fn like_pattern(q: &str) -> String {
    let mut escaped = String::with_capacity(q.len() + 2);
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    format!("%{escaped}%")
}

/// Substring search over the caller's own objects and activities.
///
/// Deliberately `LIKE` rather than FTS5: at the scale LogB is built for -- one household's
/// belongings -- a scan is instant, and it needs no shadow table or trigger to keep in sync.
///
/// The operator is the one thing the two backends spell differently: PostgreSQL's `LIKE` does
/// not fold case at all, so it needs `ILIKE`. That is not a pure translation, and it is the
/// one place a user can tell the two apart. SQLite's `LIKE` folds only the 26 ASCII letters,
/// so "ÖLWECHSEL" finds "Ölwechsel" but "ölwechsel" does not; PostgreSQL's `ILIKE` folds by the
/// server's collation, so on a UTF-8 database it finds it. Closing that gap would mean an ICU
/// build of SQLite or a shadow column of folded text -- a cost out of all proportion to a
/// search box over one household's belongings. `tests/dialect.rs` pins both halves so the
/// difference stays a known one rather than a surprise.
///
/// `type` is deliberately not matched. It used to be, back when the column held whatever the
/// user had typed -- so a German user searching "Auto" found their car. It now holds `car`, an
/// identifier no user ever wrote, and matching it searches the schema rather than the data:
/// "Auto" finds nothing, "other" returns every unclassified object, and "bike" drags in every
/// e-bike. Name and description are the words the user chose, and the migration preserved
/// unmapped legacy text into the description, so those objects stay findable by the words
/// their owner actually used. Finding an object by its type is a filter, not a search term.
async fn search(user: AuthUser, State(state): State<App>, Query(q): Query<SearchQuery>) -> Result<Json<SearchResults>, AppError> {
    let term = q.q.trim();
    if term.is_empty() {
        return Err(AppError::BadRequest("q is required".into()));
    }
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let pattern = like_pattern(term);

    let like = state.backend.case_insensitive_like();
    // Qualified: the self-join puts two `name` columns in scope, and an unqualified one is
    // ambiguous to PostgreSQL.
    let order = state.backend.name_order("o.name");
    // `tags` holds JSON text, so a term containing JSON punctuation would match the encoding
    // rather than a tag: `[` finds every row, `"` every tagged one. Such a term cannot be part
    // of a tag the user means, so the tags match is left out of the statement for it. Only
    // these fixed fragments are spliced in; the term itself stays a bind parameter.
    let match_tags = !term.contains(['[', ']', '"', ',', '\\']);
    let object_tags = if match_tags { format!(" OR o.tags {like} $2 ESCAPE '\\'") } else { String::new() };
    let activity_tags = if match_tags { format!(" OR a.tags {like} $2 ESCAPE '\\'") } else { String::new() };

    let objects = sqlx::query_as::<_, ObjectHit>(sqlx::AssertSqlSafe(format!(
        "SELECT o.id, o.user_id, o.name, o.type, o.counter_unit, o.fuel_unit, o.description, \
         o.purchase_date, o.purchase_price_cents, o.archived_at, o.cover_attachment_id, \
         o.parent_id, o.created_at, o.updated_at, p.name AS parent_name, o.tags \
         FROM objects o LEFT JOIN objects p ON p.id = o.parent_id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL AND ( \
           o.name {like} $2 ESCAPE '\\' OR o.description {like} $2 ESCAPE '\\'{object_tags}) \
         ORDER BY o.archived_at IS NOT NULL, {order} LIMIT $3")))
    .bind(user.id).bind(&pattern).bind(limit)
    .fetch_all(&state.db).await?;

    let activities = sqlx::query_as::<_, ActivityHit>(sqlx::AssertSqlSafe(format!(
        "SELECT a.id, a.object_id, o.name AS object_name, a.date, a.category, a.title, a.notes, \
         a.counter_value, a.cost_cents, a.weight_grams, a.tags, a.from_place, a.to_place \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
           AND (a.title {like} $2 ESCAPE '\\' OR a.notes {like} $2 ESCAPE '\\' \
             OR a.from_place {like} $2 ESCAPE '\\' OR a.to_place {like} $2 ESCAPE '\\'{activity_tags}) \
         ORDER BY a.date DESC, a.id DESC LIMIT $3")))
    .bind(user.id).bind(&pattern).bind(limit)
    .fetch_all(&state.db).await?;

    Ok(Json(SearchResults { objects, activities }))
}

#[cfg(test)]
mod tests {
    use super::like_pattern;

    #[test]
    fn plain_terms_become_a_contains_pattern() {
        assert_eq!(like_pattern("oil"), "%oil%");
    }

    #[test]
    fn like_wildcards_in_the_query_are_matched_literally() {
        assert_eq!(like_pattern("100%"), "%100\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("c:\\temp"), "%c:\\\\temp%");
    }
}
