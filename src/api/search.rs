use crate::auth::AuthUser;
use crate::domain::tags::fold;
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
/// Deliberately `LIKE` rather than FTS5: at the scale LogB is built for -- one household's
/// belongings -- a scan is instant, and it needs no shadow table or trigger to keep in sync.
///
/// Matching is done in Rust, not SQL: both statements select the caller's live rows and
/// `matches` folds each field with `domain::tags::fold` (NFD, combining marks stripped, lower
/// case) before testing `contains`. SQLite's `LIKE` folds only ASCII and PostgreSQL's `ILIKE`
/// folds by the cluster's collation, so a SQL predicate would answer "ölwechsel" differently
/// per backend; folding here makes the two agree, and matches what the objects list already
/// does on the client (`lib/object-list.ts`). A household's rows fit in one scan, the same
/// cost `activities::list_for_object` already pays for its tag filter.
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
    // Qualified: the self-join puts two `name` columns in scope, and an unqualified one is
    // ambiguous to PostgreSQL.
    let order = state.backend.name_order("o.name");

    let objects = sqlx::query_as::<_, ObjectHit>(sqlx::AssertSqlSafe(format!(
        "SELECT o.id, o.user_id, o.name, o.type, o.counter_unit, o.fuel_unit, o.description, \
         o.purchase_date, o.purchase_price_cents, o.archived_at, o.cover_attachment_id, \
         o.parent_id, o.created_at, o.updated_at, p.name AS parent_name, o.tags \
         FROM objects o LEFT JOIN objects p ON p.id = o.parent_id \
         WHERE o.user_id = $1 AND o.deleted_at IS NULL \
         ORDER BY o.archived_at IS NOT NULL, {order}"
    )))
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let activities = sqlx::query_as::<_, ActivityHit>(
        "SELECT a.id, a.object_id, o.name AS object_name, a.date, a.category, a.title, a.notes, \
         a.counter_value, a.cost_cents, a.weight_grams, a.tags, a.from_place, a.to_place \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = $1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
         ORDER BY a.date DESC, a.id DESC",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let skip = offset as usize;
    let mut objects: Vec<ObjectHit> = objects
        .into_iter()
        .filter(|o| matches(&folded, &[Some(&o.name), Some(&o.description)], &o.tags))
        .skip(skip)
        .take(limit as usize + 1)
        .collect();
    let mut activities: Vec<ActivityHit> = activities
        .into_iter()
        .filter(|a| {
            matches(
                &folded,
                &[Some(&a.title), Some(&a.notes), a.from_place.as_deref(), a.to_place.as_deref()],
                &a.tags,
            )
        })
        .skip(skip)
        .take(limit as usize + 1)
        .collect();

    let has_more = objects.len() > limit as usize || activities.len() > limit as usize;
    objects.truncate(limit as usize);
    activities.truncate(limit as usize);

    Ok(Json(SearchResults { objects, activities, has_more }))
}

/// Whether any of `fields`, or any tag in `tags_json`, contains the already-folded `term`.
/// Tags are matched one by one after decoding, so a term made of JSON punctuation cannot match
/// the encoding of every tagged row.
fn matches(term: &str, fields: &[Option<&str>], tags_json: &str) -> bool {
    if fields.iter().flatten().any(|f| fold(f).contains(term)) {
        return true;
    }
    serde_json::from_str::<Vec<String>>(tags_json)
        .map(|tags| tags.iter().any(|t| fold(t).contains(term)))
        .unwrap_or(false)
}
