use super::attachments::{self, AttachmentOut};
use super::objects::{load_owned_object, validate_date, ObjectRow};
use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub const CATEGORIES: [&str; 7] =
    ["maintenance", "repair", "purchase", "inspection", "modification", "fuel", "other"];

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/activities", get(list).post(create))
        .route("/objects/{id}/recent-titles", get(recent_titles))
        .route("/activities/{id}", get(read).patch(update).delete(delete))
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ActivityRow {
    pub id: i64,
    pub object_id: i64,
    pub date: String,
    pub category: String,
    pub title: String,
    pub notes: String,
    pub counter_value: Option<i64>,
    pub cost_cents: Option<i64>,
    pub quantity_milli: Option<i64>,
    pub client_op_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct ActivityInput {
    pub date: String,
    pub category: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub counter_value: Option<i64>,
    #[serde(default)]
    pub cost_cents: Option<i64>,
    #[serde(default)]
    pub quantity_milli: Option<i64>,
    /// Client-generated id for this creation attempt. Present only from the offline outbox.
    #[serde(default)]
    pub client_op_id: Option<String>,
}

impl ActivityInput {
    pub(crate) fn validate(&mut self, object: &ObjectRow) -> Result<(), AppError> {
        validate_date(&self.date)?;
        if !CATEGORIES.contains(&self.category.as_str()) {
            return Err(AppError::BadRequest(format!("category must be one of {}", CATEGORIES.join(", "))));
        }
        self.title = self.title.trim().to_string();
        if self.title.is_empty() { return Err(AppError::BadRequest("title is required".into())); }
        if let Some(c) = self.counter_value {
            if object.counter_unit.is_none() {
                return Err(AppError::BadRequest("this object has no counter".into()));
            }
            if c < 0 { return Err(AppError::BadRequest("counter_value must be >= 0".into())); }
        }
        if matches!(self.cost_cents, Some(c) if c < 0) {
            return Err(AppError::BadRequest("cost_cents must be >= 0".into()));
        }
        if let Some(q) = self.quantity_milli {
            if q < 0 { return Err(AppError::BadRequest("quantity_milli must be >= 0".into())); }
            if object.counter_unit.is_none() {
                return Err(AppError::BadRequest("quantity_milli needs an object with a counter".into()));
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct ActivityOut {
    #[serde(flatten)]
    pub activity: ActivityRow,
    pub attachments: Vec<AttachmentOut>,
}

pub async fn with_attachments(state: &App, rows: Vec<ActivityRow>) -> Result<Vec<ActivityOut>, AppError> {
    let object_id = match rows.first() { Some(r) => r.object_id, None => return Ok(vec![]) };
    let all = attachments::for_object(state, object_id).await?;
    Ok(rows.into_iter().map(|activity| {
        let attachments = all.iter().filter(|a| a.activity_id == Some(activity.id)).cloned().collect();
        ActivityOut { activity, attachments }
    }).collect())
}

async fn one_out(state: &App, row: ActivityRow) -> Result<ActivityOut, AppError> {
    Ok(with_attachments(state, vec![row]).await?.pop().unwrap())
}

pub async fn load_owned_activity(state: &App, user_id: i64, id: i64) -> Result<ActivityRow, AppError> {
    sqlx::query_as::<_, ActivityRow>(
        "SELECT a.id, a.object_id, a.date, a.category, a.title, a.notes, a.counter_value, a.cost_cents, \
         a.quantity_milli, a.client_op_id, a.created_at, a.updated_at FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE a.id = ? AND o.user_id = ?",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

/// A page of a long timeline. Kept generous because the common object has tens of
/// activities, not thousands; the cap only exists so a decade-old car cannot make the
/// dashboard ship megabytes to a phone in one response.
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

#[derive(Deserialize)]
pub struct ListQuery {
    pub category: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// How many activities match the filters, ignoring the page window -- the client needs it to
/// know whether a "load more" button belongs on screen.
pub async fn count_for_object(state: &App, object_id: i64, q: &ListQuery) -> Result<i64, AppError> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM activities WHERE object_id = ?1 \
         AND (?2 IS NULL OR category = ?2) AND (?3 IS NULL OR date >= ?3) AND (?4 IS NULL OR date <= ?4)",
    )
    .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to)
    .fetch_one(&state.db).await?;
    Ok(n)
}

pub async fn list_for_object(state: &App, object_id: i64, q: &ListQuery) -> Result<Vec<ActivityRow>, AppError> {
    if let Some(d) = &q.from { validate_date(d)?; }
    if let Some(d) = &q.to { validate_date(d)?; }
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = q.offset.unwrap_or(0).max(0);
    Ok(sqlx::query_as::<_, ActivityRow>(
        "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at \
         FROM activities WHERE object_id = ?1 \
         AND (?2 IS NULL OR category = ?2) AND (?3 IS NULL OR date >= ?3) AND (?4 IS NULL OR date <= ?4) \
         ORDER BY date DESC, id DESC LIMIT ?5 OFFSET ?6",
    )
    .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to).bind(limit).bind(offset)
    .fetch_all(&state.db).await?)
}

async fn list(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Query(q): Query<ListQuery>) -> Result<Response, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let rows = list_for_object(&state, object_id, &q).await?;
    let total = count_for_object(&state, object_id, &q).await?;
    let out = with_attachments(&state, rows).await?;
    Ok((
        [(axum::http::header::HeaderName::from_static("x-total-count"), total.to_string())],
        Json(out),
    ).into_response())
}

/// One row per distinct (title, category) an object has seen, newest first, with the
/// values of the most recent occurrence.
///
/// A dedicated endpoint rather than reusing `list`: that one joins every attachment of
/// the object, which is payload a phone does not need in order to fill a datalist.
#[derive(Serialize, sqlx::FromRow)]
pub struct TitleSuggestion {
    pub title: String,
    pub category: String,
    pub last_date: String,
    pub last_cost_cents: Option<i64>,
    pub last_counter: Option<i64>,
}

const SUGGESTION_LIMIT: i64 = 20;

async fn recent_titles(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
) -> Result<Json<Vec<TitleSuggestion>>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    // The correlated subqueries pick the newest occurrence explicitly. SQLite would also
    // hand back a bare column from the MAX() row, but that behaviour is a quirk to rely on,
    // not a contract.
    let rows = sqlx::query_as::<_, TitleSuggestion>(
        "SELECT a.title, a.category, MAX(a.date) AS last_date, \
           (SELECT x.cost_cents FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_cost_cents, \
           (SELECT x.counter_value FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_counter \
         FROM activities a WHERE a.object_id = ?1 \
         GROUP BY a.title, a.category ORDER BY last_date DESC LIMIT ?2",
    )
    .bind(object_id)
    .bind(SUGGESTION_LIMIT)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn create(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Json(mut body): Json<ActivityInput>) -> Result<Response, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    body.validate(&object)?;
    // A blank (or all-whitespace) client_op_id means no idempotency was requested, not a
    // real, indexable id -- see `super::normalize_op_id` for why that distinction matters.
    body.client_op_id = super::normalize_op_id(body.client_op_id.take());
    // A retry after a lost response must resolve to the row the first attempt made.
    if let Some(op) = body.client_op_id.as_deref() {
        if let Some(existing) = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
             quantity_milli, client_op_id, created_at, updated_at \
             FROM activities WHERE client_op_id = ?",
        )
        .bind(op)
        .fetch_optional(&state.db)
        .await?
        {
            return op_id_row_response(&state, existing, object_id).await;
        }
    }
    let now = db::now();
    let inserted = sqlx::query_as::<_, ActivityRow>(
        "INSERT INTO activities (object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         RETURNING id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at",
    )
    .bind(object_id).bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(&body.client_op_id).bind(&now).bind(&now)
    .fetch_one(&state.db).await;
    let row = match inserted {
        Ok(row) => row,
        // Two concurrent requests carrying the same client_op_id -- e.g. several browser tabs
        // sharing one offline outbox, all flushing on reconnect -- can both pass the pre-check
        // above before either has inserted. The loser's INSERT then trips the partial unique
        // index instead of the pre-check catching it; treat that exactly like the pre-check
        // would have, by adopting the winner's row rather than failing the request.
        Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
            let op = body.client_op_id.as_deref().expect("only a client_op_id insert can trip this index");
            let winner = sqlx::query_as::<_, ActivityRow>(
                "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
                 quantity_milli, client_op_id, created_at, updated_at \
                 FROM activities WHERE client_op_id = ?",
            )
            .bind(op)
            .fetch_one(&state.db).await?;
            return op_id_row_response(&state, winner, object_id).await;
        }
        Err(e) => return Err(e.into()),
    };
    Ok((StatusCode::CREATED, Json(one_out(&state, row).await?)).into_response())
}

/// The row a client_op_id lookup found -- whether from the pre-check or after losing an
/// insert race -- may belong to a different object than the one being posted to; that's a
/// 409, not this object's row.
async fn op_id_row_response(state: &App, existing: ActivityRow, object_id: i64) -> Result<Response, AppError> {
    if existing.object_id != object_id {
        return Err(AppError::Conflict("client_op_id already used for another object".into()));
    }
    Ok((StatusCode::OK, Json(one_out(state, existing).await?)).into_response())
}

async fn read(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ActivityOut>, AppError> {
    let row = load_owned_activity(&state, user.id, id).await?;
    Ok(Json(one_out(&state, row).await?))
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(mut body): Json<ActivityInput>) -> Result<Json<ActivityOut>, AppError> {
    let existing = load_owned_activity(&state, user.id, id).await?;
    let object = load_owned_object(&state, user.id, existing.object_id).await?;
    body.validate(&object)?;
    sqlx::query(
        "UPDATE activities SET date = ?, category = ?, title = ?, notes = ?, counter_value = ?, cost_cents = ?, quantity_milli = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(db::now()).bind(id)
    .execute(&state.db).await?;
    let row = load_owned_activity(&state, user.id, id).await?;
    Ok(Json(one_out(&state, row).await?))
}

async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    load_owned_activity(&state, user.id, id).await?;
    sqlx::query(
        "UPDATE objects SET cover_attachment_id = NULL \
         WHERE cover_attachment_id IN (SELECT id FROM attachments WHERE activity_id = ?)",
    )
    .bind(id).execute(&state.db).await?;
    let files = attachments::files_of_activity(&state, id).await?;
    sqlx::query("DELETE FROM activities WHERE id = ?").bind(id).execute(&state.db).await?;
    attachments::purge_orphan_files(&state, &files).await?;
    Ok(StatusCode::NO_CONTENT)
}
