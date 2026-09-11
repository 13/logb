use super::objects::ObjectRow;
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
}

#[derive(Serialize)]
pub struct SearchResults {
    pub objects: Vec<ObjectRow>,
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
/// SQLite's `LIKE` folds case for ASCII only, so a query for "olwechsel" will not match
/// "Ölwechsel"; that is the trade for not carrying an index.
async fn search(user: AuthUser, State(state): State<App>, Query(q): Query<SearchQuery>) -> Result<Json<SearchResults>, AppError> {
    let term = q.q.trim();
    if term.is_empty() {
        return Err(AppError::BadRequest("q is required".into()));
    }
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let pattern = like_pattern(term);

    let objects = sqlx::query_as::<_, ObjectRow>(
        "SELECT id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
         FROM objects WHERE user_id = ?1 AND deleted_at IS NULL AND ( \
           name LIKE ?2 ESCAPE '\\' OR type LIKE ?2 ESCAPE '\\' OR description LIKE ?2 ESCAPE '\\') \
         ORDER BY archived_at IS NOT NULL, name COLLATE NOCASE LIMIT ?3",
    )
    .bind(user.id).bind(&pattern).bind(limit)
    .fetch_all(&state.db).await?;

    let activities = sqlx::query_as::<_, ActivityHit>(
        "SELECT a.id, a.object_id, o.name AS object_name, a.date, a.category, a.title, a.notes, \
         a.counter_value, a.cost_cents \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE o.user_id = ?1 AND a.deleted_at IS NULL AND o.deleted_at IS NULL \
           AND (a.title LIKE ?2 ESCAPE '\\' OR a.notes LIKE ?2 ESCAPE '\\') \
         ORDER BY a.date DESC, a.id DESC LIMIT ?3",
    )
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
