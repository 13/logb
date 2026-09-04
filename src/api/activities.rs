use super::attachments::{self, AttachmentOut};
use super::objects::{load_owned_object, validate_date, ObjectRow};
use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub const CATEGORIES: [&str; 7] =
    ["maintenance", "repair", "purchase", "inspection", "modification", "fuel", "other"];

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/activities", get(list).post(create))
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
}

impl ActivityInput {
    fn validate(&mut self, object: &ObjectRow) -> Result<(), AppError> {
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
         a.created_at, a.updated_at FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE a.id = ? AND o.user_id = ?",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub category: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

pub async fn list_for_object(state: &App, object_id: i64, q: &ListQuery) -> Result<Vec<ActivityRow>, AppError> {
    if let Some(d) = &q.from { validate_date(d)?; }
    if let Some(d) = &q.to { validate_date(d)?; }
    Ok(sqlx::query_as::<_, ActivityRow>(
        "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, created_at, updated_at \
         FROM activities WHERE object_id = ?1 \
         AND (?2 IS NULL OR category = ?2) AND (?3 IS NULL OR date >= ?3) AND (?4 IS NULL OR date <= ?4) \
         ORDER BY date DESC, id DESC",
    )
    .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to)
    .fetch_all(&state.db).await?)
}

async fn list(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Query(q): Query<ListQuery>) -> Result<Json<Vec<ActivityOut>>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let rows = list_for_object(&state, object_id, &q).await?;
    Ok(Json(with_attachments(&state, rows).await?))
}

async fn create(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Json(mut body): Json<ActivityInput>) -> Result<(StatusCode, Json<ActivityOut>), AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    body.validate(&object)?;
    let now = db::now();
    let row = sqlx::query_as::<_, ActivityRow>(
        "INSERT INTO activities (object_id, date, category, title, notes, counter_value, cost_cents, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         RETURNING id, object_id, date, category, title, notes, counter_value, cost_cents, created_at, updated_at",
    )
    .bind(object_id).bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(&now).bind(&now)
    .fetch_one(&state.db).await?;
    Ok((StatusCode::CREATED, Json(one_out(&state, row).await?)))
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
        "UPDATE activities SET date = ?, category = ?, title = ?, notes = ?, counter_value = ?, cost_cents = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(db::now()).bind(id)
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
    sqlx::query("DELETE FROM activities WHERE id = ?").bind(id).execute(&state.db).await?;
    attachments::purge_orphan_files(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}
