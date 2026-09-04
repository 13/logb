use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects", get(list).post(create))
        .route("/objects/{id}", get(read).patch(update).delete(delete))
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ObjectRow {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub category: String,
    pub counter_unit: Option<String>,
    pub description: String,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    pub archived_at: Option<String>,
    pub cover_attachment_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ObjectStats {
    pub total_cost_cents: i64,
    pub activity_count: i64,
    pub current_counter: Option<i64>,
    pub due_reminder_count: i64,
}

#[derive(Serialize)]
pub struct ObjectOut {
    #[serde(flatten)]
    pub object: ObjectRow,
    pub stats: ObjectStats,
}

#[derive(Deserialize)]
pub struct ObjectInput {
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub counter_unit: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub purchase_date: Option<String>,
    #[serde(default)]
    pub purchase_price_cents: Option<i64>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub cover_attachment_id: Option<i64>,
}

pub fn validate_date(s: &str) -> Result<(), AppError> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| AppError::BadRequest(format!("invalid date '{s}', expected YYYY-MM-DD")))
}

impl ObjectInput {
    fn validate(&mut self) -> Result<(), AppError> {
        self.name = self.name.trim().to_string();
        self.category = self.category.trim().to_string();
        if self.name.is_empty() { return Err(AppError::BadRequest("name is required".into())); }
        if self.category.is_empty() { return Err(AppError::BadRequest("category is required".into())); }
        if let Some(u) = &self.counter_unit {
            if !matches!(u.as_str(), "km" | "mi" | "h") {
                return Err(AppError::BadRequest("counter_unit must be km, mi, h or null".into()));
            }
        }
        if let Some(d) = &self.purchase_date { validate_date(d)?; }
        if matches!(self.purchase_price_cents, Some(p) if p < 0) {
            return Err(AppError::BadRequest("purchase_price_cents must be >= 0".into()));
        }
        Ok(())
    }
}

/// The object with `id` if it belongs to `user_id`; otherwise 404.
pub async fn load_owned_object(state: &App, user_id: i64, id: i64) -> Result<ObjectRow, AppError> {
    sqlx::query_as::<_, ObjectRow>(
        "SELECT id, user_id, name, category, counter_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
         FROM objects WHERE id = ? AND user_id = ?",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

pub async fn stats(state: &App, object_id: i64) -> Result<ObjectStats, AppError> {
    sqlx::query_as::<_, ObjectStats>(
        "SELECT \
           COALESCE((SELECT SUM(cost_cents) FROM activities WHERE object_id = ?1), 0) AS total_cost_cents, \
           (SELECT COUNT(*) FROM activities WHERE object_id = ?1) AS activity_count, \
           (SELECT MAX(counter_value) FROM activities WHERE object_id = ?1) AS current_counter, \
           (SELECT COUNT(*) FROM reminders r WHERE r.object_id = ?1 AND r.done_at IS NULL AND ( \
              (r.due_date IS NOT NULL AND r.due_date <= ?2) OR \
              (r.due_counter IS NOT NULL AND r.due_counter <= (SELECT MAX(counter_value) FROM activities WHERE object_id = ?1)) \
           )) AS due_reminder_count",
    )
    .bind(object_id).bind(db::today())
    .fetch_one(&state.db).await
    .map_err(Into::into)
}

async fn with_stats(state: &App, object: ObjectRow) -> Result<ObjectOut, AppError> {
    let stats = stats(state, object.id).await?;
    Ok(ObjectOut { object, stats })
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub archived: bool,
}

async fn list(user: AuthUser, State(state): State<App>, Query(q): Query<ListQuery>) -> Result<Json<Vec<ObjectOut>>, AppError> {
    let rows = if q.archived {
        sqlx::query_as::<_, ObjectRow>(
            "SELECT id, user_id, name, category, counter_unit, description, purchase_date, \
             purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
             FROM objects WHERE user_id = ? AND archived_at IS NOT NULL ORDER BY name COLLATE NOCASE")
            .bind(user.id).fetch_all(&state.db).await?
    } else {
        sqlx::query_as::<_, ObjectRow>(
            "SELECT id, user_id, name, category, counter_unit, description, purchase_date, \
             purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
             FROM objects WHERE user_id = ? AND archived_at IS NULL ORDER BY name COLLATE NOCASE")
            .bind(user.id).fetch_all(&state.db).await?
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(with_stats(&state, row).await?);
    }
    Ok(Json(out))
}

async fn create(user: AuthUser, State(state): State<App>, Json(mut body): Json<ObjectInput>) -> Result<(StatusCode, Json<ObjectOut>), AppError> {
    body.validate()?;
    let now = db::now();
    let archived_at = if body.archived == Some(true) { Some(now.clone()) } else { None };
    let row = sqlx::query_as::<_, ObjectRow>(
        "INSERT INTO objects (user_id, name, category, counter_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?) \
         RETURNING id, user_id, name, category, counter_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at",
    )
    .bind(user.id).bind(&body.name).bind(&body.category).bind(&body.counter_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(archived_at).bind(&now).bind(&now)
    .fetch_one(&state.db).await?;
    Ok((StatusCode::CREATED, Json(with_stats(&state, row).await?)))
}

async fn read(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ObjectOut>, AppError> {
    let row = load_owned_object(&state, user.id, id).await?;
    Ok(Json(with_stats(&state, row).await?))
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(mut body): Json<ObjectInput>) -> Result<Json<ObjectOut>, AppError> {
    let existing = load_owned_object(&state, user.id, id).await?;
    body.validate()?;
    let archived_at = match body.archived {
        Some(true) => existing.archived_at.clone().or_else(|| Some(db::now())),
        Some(false) => None,
        None => existing.archived_at.clone(),
    };
    if let Some(cover) = body.cover_attachment_id {
        let ok: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE id = ? AND object_id = ? AND kind = 'photo'")
            .bind(cover).bind(id).fetch_optional(&state.db).await?;
        if ok.is_none() {
            return Err(AppError::BadRequest("cover_attachment_id must be a photo of this object".into()));
        }
    }
    sqlx::query(
        "UPDATE objects SET name = ?, category = ?, counter_unit = ?, description = ?, purchase_date = ?, \
         purchase_price_cents = ?, archived_at = ?, cover_attachment_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&body.name).bind(&body.category).bind(&body.counter_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(archived_at)
    .bind(body.cover_attachment_id.or(existing.cover_attachment_id)).bind(db::now()).bind(id)
    .execute(&state.db).await?;
    let row = load_owned_object(&state, user.id, id).await?;
    Ok(Json(with_stats(&state, row).await?))
}

async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    load_owned_object(&state, user.id, id).await?;
    sqlx::query("DELETE FROM objects WHERE id = ?").bind(id).execute(&state.db).await?;
    super::attachments::purge_orphan_files(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}
