use crate::auth::AuthUser;
use crate::domain::tags::fold;
use crate::error::AppError;
use crate::search_text;
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
    #[serde(default)]
    pub offset: Option<i64>,
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
    /// request -- matched the same way as `title` and `notes`, below.
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
    pub has_more: bool,
}

/// Substring search over the caller's own objects and activities.
///
/// Each row carries `search_text` (see `crate::search_text`): its searched fields, each folded
/// by `domain::tags::fold` (NFD, combining marks stripped, lower case) and written by Rust on
/// every write path. The term is folded the same way here, so the SQL only has to ask whether
/// one folded string contains another -- a question both backends answer identically, where
/// folding in SQL would not be (SQLite's `LIKE` folds only ASCII, PostgreSQL's `ILIKE` folds by
/// the cluster's collation). The fold matches what the objects list already does on the client
/// (`lib/object-list.ts`).
///
/// Matching, ordering and paging all happen in SQL, so a request reads one page of rows
/// rather than every row the user has. The answer is the one the old in-Rust scan gave: a hit
/// is a row one of whose fields contains the whole term; objects come unarchived first, then
/// by name; activities newest first; `offset` and `limit` apply to each list separately, and
/// `has_more` says whether either list has a row beyond this page. Ties -- two objects of the
/// same name -- are broken by id, which the scan left to the database.
///
/// `type` is deliberately not matched. It used to be, back when the column held whatever the
/// user had typed -- so a German user searching "Auto" found their car. It now holds `car`, an
/// identifier no user ever wrote, and matching it searches the schema rather than the data:
/// "Auto" finds nothing, "other" returns every unclassified object, and "bike" drags in every
/// e-bike. Name and description are the words the user chose, and the migration preserved
/// unmapped legacy text into the description, so those objects stay findable by the words
/// their owner actually used. Finding an object by its type is a filter, not a search term.
async fn search(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<SearchResults>, AppError> {
    let term = q.q.trim();
    if term.is_empty() {
        return Err(AppError::BadRequest("q is required".into()));
    }
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = q.offset.unwrap_or(0).max(0);
    let folded = fold(term);
    // A term holding the separator `search_text` puts between fields could only match across
    // two of them, which is not a hit; and PostgreSQL refuses a NUL in any text it is handed,
    // where no stored field can hold one. Neither can match, so neither reaches the database.
    if folded.contains([search_text::SEPARATOR, '\0']) {
        return Ok(Json(SearchResults { objects: Vec::new(), activities: Vec::new(), has_more: false }));
    }
    let backend = state.backend;
    // Qualified: the self-join puts two `name` columns in scope, and an unqualified one is
    // ambiguous to PostgreSQL.
    let order = backend.name_order("o.name");
    let objects_match = backend.contains("o.search_text", "$2");
    let activities_match = backend.contains("a.search_text", "$2");
    let needle = backend.contains_param(&folded);
    // One row past the page, so `has_more` needs no count.
    let fetch = limit + 1;

    let mut objects = sqlx::query_as::<_, ObjectHit>(sqlx::AssertSqlSafe(format!(
        "SELECT o.id, o.user_id, o.name, o.type, o.counter_unit, o.fuel_unit, o.description, \
         o.purchase_date, o.purchase_price_cents, o.archived_at, o.cover_attachment_id, \
         o.parent_id, o.created_at, o.updated_at, p.name AS parent_name, o.tags \
         FROM objects o LEFT JOIN objects p ON p.id = o.parent_id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL AND {objects_match} \
         ORDER BY o.archived_at IS NOT NULL, {order}, o.id \
         LIMIT $3 OFFSET $4"
    )))
    .bind(user.id)
    .bind(&needle)
    .bind(fetch)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let mut activities = sqlx::query_as::<_, ActivityHit>(sqlx::AssertSqlSafe(format!(
        "SELECT a.id, a.object_id, o.name AS object_name, a.date, a.category, a.title, a.notes, \
         a.counter_value, a.cost_cents, a.weight_grams, a.tags, a.from_place, a.to_place \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
         AND {activities_match} \
         ORDER BY a.date DESC, a.id DESC \
         LIMIT $3 OFFSET $4"
    )))
    .bind(user.id)
    .bind(&needle)
    .bind(fetch)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let has_more = objects.len() > limit as usize || activities.len() > limit as usize;
    objects.truncate(limit as usize);
    activities.truncate(limit as usize);

    Ok(Json(SearchResults { objects, activities, has_more }))
}
