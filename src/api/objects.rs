use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::state::App;
use crate::sync::{record, Entity};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use std::collections::HashMap;

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
    pub fuel_unit: Option<String>,
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
    /// file_id of the cover attachment, so the client can build a thumbnail URL directly
    pub cover_file_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct ObjectInput {
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub counter_unit: Option<String>,
    #[serde(default)]
    pub fuel_unit: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub purchase_date: Option<String>,
    #[serde(default)]
    pub purchase_price_cents: Option<i64>,
    #[serde(default)]
    pub archived: Option<bool>,
    /// Three-state on PATCH: absent keeps the current cover, `null` clears it, an id sets it.
    /// A plain `Option` cannot tell "absent" from "null", which is why the cover could
    /// previously only ever be set, never removed.
    #[serde(default, deserialize_with = "double_option")]
    pub cover_attachment_id: Option<Option<i64>>,
}

/// Deserializes a present field -- including an explicit `null` -- as `Some(..)`, leaving
/// `None` to mean "the client did not send this field at all".
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(d).map(Some)
}

pub fn validate_date(s: &str) -> Result<(), AppError> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| AppError::BadRequest(format!("invalid date '{s}', expected YYYY-MM-DD")))
}

impl ObjectInput {
    pub(crate) fn validate(&mut self) -> Result<(), AppError> {
        self.name = self.name.trim().to_string();
        self.category = self.category.trim().to_string();
        if self.name.is_empty() { return Err(AppError::BadRequest("name is required".into())); }
        if self.category.is_empty() { return Err(AppError::BadRequest("category is required".into())); }
        if let Some(u) = &self.counter_unit {
            if !matches!(u.as_str(), "km" | "mi" | "h") {
                return Err(AppError::BadRequest("counter_unit must be km, mi, h or null".into()));
            }
        }
        if let Some(u) = &self.fuel_unit {
            if !matches!(u.as_str(), "l" | "gal" | "kwh") {
                return Err(AppError::BadRequest("fuel_unit must be l, gal, kwh or null".into()));
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
        "SELECT id, user_id, name, category, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
         FROM objects WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

/// One row of derived data per object: the stats block plus the cover's `file_id`.
#[derive(sqlx::FromRow)]
struct DerivedRow {
    object_id: i64,
    total_cost_cents: i64,
    activity_count: i64,
    current_counter: Option<i64>,
    due_reminder_count: i64,
    cover_file_id: Option<i64>,
}

/// Everything `ObjectOut` needs beyond the `objects` row itself, for every object the user
/// owns, in one statement. `only` narrows it to a single object for the read/create/update
/// handlers, which keeps one copy of this SQL rather than a per-object and a per-list variant.
///
/// The list endpoint used to call `stats` and then a cover lookup once per object, so showing
/// N objects cost 2N + 1 queries.
async fn derived(state: &App, user_id: Option<i64>, only: Option<i64>) -> Result<HashMap<i64, DerivedRow>, AppError> {
    // INVARIANT: this due_reminder_count subquery is a second, hand-written encoding of
    // `domain::reminder::is_due` -- it exists only so N objects' counts can be computed in one
    // statement instead of loading every reminder and folding `is_due` over them in memory. The
    // two must keep agreeing row for row; `due_reminder_count_agrees_with_each_reminders_due_flag`
    // in tests/objects.rs is what catches them drifting apart.
    let rows = sqlx::query_as::<_, DerivedRow>(
        "SELECT o.id AS object_id, \
           COALESCE((SELECT SUM(cost_cents) FROM activities WHERE object_id = o.id AND deleted_at IS NULL), 0) AS total_cost_cents, \
           (SELECT COUNT(*) FROM activities WHERE object_id = o.id AND deleted_at IS NULL) AS activity_count, \
           (SELECT MAX(counter_value) FROM activities WHERE object_id = o.id AND deleted_at IS NULL) AS current_counter, \
           (SELECT COUNT(*) FROM reminders r WHERE r.object_id = o.id AND r.done_at IS NULL AND r.deleted_at IS NULL \
              AND (r.snoozed_until IS NULL OR r.snoozed_until <= ?2) AND ( \
              (r.due_date IS NOT NULL AND r.due_date <= ?2) OR \
              (r.due_counter IS NOT NULL AND r.due_counter <= (SELECT MAX(counter_value) FROM activities WHERE object_id = o.id AND deleted_at IS NULL)) \
           )) AS due_reminder_count, \
           (SELECT file_id FROM attachments WHERE id = o.cover_attachment_id AND deleted_at IS NULL) AS cover_file_id \
         FROM objects o WHERE o.deleted_at IS NULL AND (?1 IS NULL OR o.user_id = ?1) AND (?3 IS NULL OR o.id = ?3)",
    )
    .bind(user_id).bind(db::today()).bind(only)
    .fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(|r| (r.object_id, r)).collect())
}

impl DerivedRow {
    fn stats(&self) -> ObjectStats {
        ObjectStats {
            total_cost_cents: self.total_cost_cents,
            activity_count: self.activity_count,
            current_counter: self.current_counter,
            due_reminder_count: self.due_reminder_count,
        }
    }

    fn into_out(self, object: ObjectRow) -> ObjectOut {
        let stats = self.stats();
        ObjectOut { object, stats, cover_file_id: self.cover_file_id }
    }
}

/// The stats block for one object, for callers outside this module. Ownership is not checked
/// here: every caller has already loaded the object through `load_owned_object`.
pub async fn stats(state: &App, object_id: i64) -> Result<ObjectStats, AppError> {
    derived(state, None, Some(object_id)).await?
        .get(&object_id)
        .map(DerivedRow::stats)
        .ok_or(AppError::NotFound)
}

async fn with_stats(state: &App, object: ObjectRow) -> Result<ObjectOut, AppError> {
    let id = object.id;
    derived(state, Some(object.user_id), Some(id)).await?
        .remove(&id)
        .map(|d| d.into_out(object))
        .ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub archived: bool,
}

async fn list(user: AuthUser, State(state): State<App>, Query(q): Query<ListQuery>) -> Result<Json<Vec<ObjectOut>>, AppError> {
    let rows = if q.archived {
        sqlx::query_as::<_, ObjectRow>(
            "SELECT id, user_id, name, category, counter_unit, fuel_unit, description, purchase_date, \
             purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
             FROM objects WHERE user_id = ? AND deleted_at IS NULL AND archived_at IS NOT NULL ORDER BY name COLLATE NOCASE")
            .bind(user.id).fetch_all(&state.db).await?
    } else {
        sqlx::query_as::<_, ObjectRow>(
            "SELECT id, user_id, name, category, counter_unit, fuel_unit, description, purchase_date, \
             purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
             FROM objects WHERE user_id = ? AND deleted_at IS NULL AND archived_at IS NULL ORDER BY name COLLATE NOCASE")
            .bind(user.id).fetch_all(&state.db).await?
    };
    let mut derived = derived(&state, Some(user.id), None).await?;
    let out = rows
        .into_iter()
        .filter_map(|row| derived.remove(&row.id).map(|d| d.into_out(row)))
        .collect();
    Ok(Json(out))
}

async fn create(user: AuthUser, State(state): State<App>, Json(mut body): Json<ObjectInput>) -> Result<(StatusCode, Json<ObjectOut>), AppError> {
    body.validate()?;
    let now = db::now();
    let archived_at = if body.archived == Some(true) { Some(now.clone()) } else { None };
    let object_uuid = uuid::Uuid::new_v4().to_string();
    let edited_at = record::edited_at_now();
    let mut tx = state.db.begin().await?;
    let row = sqlx::query_as::<_, ObjectRow>(
        "INSERT INTO objects (user_id, name, category, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at, client_uuid) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?) \
         RETURNING id, user_id, name, category, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at",
    )
    .bind(user.id).bind(&body.name).bind(&body.category).bind(&body.counter_unit).bind(&body.fuel_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(archived_at).bind(&now).bind(&now)
    .bind(&object_uuid)
    .fetch_one(&mut *tx).await?;
    record::record_create(&mut tx, user.id, Entity::Object, &object_uuid, &edited_at).await?;
    tx.commit().await?;
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
    let cover_attachment_id = match body.cover_attachment_id {
        None => existing.cover_attachment_id,
        Some(None) => None,
        Some(Some(cover)) => {
            let ok: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE id = ? AND object_id = ? AND kind = 'photo' AND deleted_at IS NULL")
                .bind(cover).bind(id).fetch_optional(&state.db).await?;
            if ok.is_none() {
                return Err(AppError::BadRequest("cover_attachment_id must be a photo of this object".into()));
            }
            Some(cover)
        }
    };

    // Only fields whose value actually differs are logged -- see `record::record_update` --
    // so a PATCH that rewrites a field with its existing value produces no `changes` row.
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if body.name != existing.name { changed.push(("name", json!(body.name))); }
    if body.category != existing.category { changed.push(("category", json!(body.category))); }
    if body.counter_unit != existing.counter_unit { changed.push(("counter_unit", json!(body.counter_unit))); }
    if body.fuel_unit != existing.fuel_unit { changed.push(("fuel_unit", json!(body.fuel_unit))); }
    if body.description != existing.description { changed.push(("description", json!(body.description))); }
    if body.purchase_date != existing.purchase_date { changed.push(("purchase_date", json!(body.purchase_date))); }
    if body.purchase_price_cents != existing.purchase_price_cents {
        changed.push(("purchase_price_cents", json!(body.purchase_price_cents)));
    }
    if archived_at != existing.archived_at { changed.push(("archived_at", json!(archived_at))); }
    if cover_attachment_id != existing.cover_attachment_id {
        changed.push(("cover_attachment_id", json!(cover_attachment_id)));
    }

    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE objects SET name = ?, category = ?, counter_unit = ?, fuel_unit = ?, description = ?, purchase_date = ?, \
         purchase_price_cents = ?, archived_at = ?, cover_attachment_id = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&body.name).bind(&body.category).bind(&body.counter_unit).bind(&body.fuel_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(&archived_at)
    .bind(cover_attachment_id).bind(db::now()).bind(id)
    .execute(&mut *tx).await?;
    if !changed.is_empty() {
        let uuid = record::uuid_of(&mut tx, Entity::Object, id).await?;
        record::record_update(&mut tx, user.id, Entity::Object, &uuid, &changed, &record::edited_at_now()).await?;
    }
    tx.commit().await?;

    let row = load_owned_object(&state, user.id, id).await?;
    Ok(Json(with_stats(&state, row).await?))
}

/// Deleting an object writes a tombstone rather than removing the row, so a client that was
/// offline when the delete happened can still learn about it on its next sync.
///
/// `ON DELETE CASCADE` only fires for a real `DELETE`, so the cascade the schema used to
/// provide has to be spelled out here -- without it the object's activities, reminders and
/// attachments would stay alive and keep syncing after their parent was gone. One transaction,
/// so a half-applied cascade cannot survive a failure mid-way.
///
/// The object's `files` rows and their blobs are deliberately left alone: a file is
/// content-addressed and shared, `attachments.file_id` is `ON DELETE RESTRICT`, and the
/// attachment rows pointing at it still exist. They are freed when the retention purge
/// finally removes those tombstoned attachments.
async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    let now = db::now();
    let edited_at = record::edited_at_now();
    let mut tx = state.db.begin().await?;
    let affected = sqlx::query(
        "UPDATE objects SET deleted_at = ?, updated_at = ? \
         WHERE id = ? AND user_id = ? AND deleted_at IS NULL")
        .bind(&now).bind(&now).bind(id).bind(user.id)
        .execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    // `record::cascade_object` is the single copy of this cascade, shared with
    // `sync::apply::apply_op`'s `delete` handling -- see the module docs on `sync::record` for
    // why hand-rolling it a second time here is exactly what drifted twice before.
    let object_uuid = record::uuid_of(&mut tx, Entity::Object, id).await?;
    let cascaded = record::cascade_object(&mut tx, &object_uuid, &now).await?;
    record::record_delete(&mut tx, user.id, Entity::Object, &object_uuid, &edited_at).await?;
    record::log_cascade(&mut tx, user.id, &edited_at, record::DEVICE_ID, &cascaded).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
