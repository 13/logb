//! `/types`: each user's own object types. Built-in types are not rows and never appear here.
//!
//! A type is soft-deleted like every other synced row, and only while no live object uses it:
//! an object whose type vanished would have no icon and no categories to offer.

use crate::auth::AuthUser;
use crate::db;
use crate::domain::custom_type::{self, TypeInput, CUSTOM_PREFIX};
use crate::domain::tags::fold;
use crate::error::AppError;
use crate::state::App;
use crate::sync::{record, Entity};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn router() -> Router<App> {
    Router::new()
        .route("/types", get(list).post(create))
        .route("/types/{id}", patch(update).delete(delete))
}

/// The one sentence a name collision answers with, on create and on rename alike.
pub(crate) const NAME_TAKEN: &str = "you already have a type with this name";

/// The 400 for a name collision, with the `name_taken` code a client translates.
fn name_taken_error() -> AppError {
    AppError::Invalid {
        code: "name_taken",
        message: NAME_TAKEN.into(),
    }
}

#[derive(sqlx::FromRow)]
struct TypeRow {
    id: i64,
    client_uuid: String,
    name: String,
    icon: String,
    /// JSON array text, written only after `custom_type::normalize`.
    categories: String,
    counter_unit: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
pub struct TypeOut {
    pub id: i64,
    pub client_uuid: String,
    /// What an object's `type` holds when it uses this type, so a client never builds it.
    pub key: String,
    pub name: String,
    pub icon: String,
    pub categories: Vec<String>,
    pub counter_unit: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<TypeRow> for TypeOut {
    fn from(r: TypeRow) -> Self {
        TypeOut {
            id: r.id,
            key: format!("{CUSTOM_PREFIX}{}", r.client_uuid),
            client_uuid: r.client_uuid,
            name: r.name,
            icon: r.icon,
            categories: serde_json::from_str(&r.categories).unwrap_or_default(),
            counter_unit: r.counter_unit,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// The same body for POST and PATCH: PATCH replaces every field, so there is no "absent keeps
/// the current value" to get wrong. `client_uuid` is read on create only.
#[derive(Deserialize)]
pub struct TypeBody {
    pub name: String,
    pub icon: String,
    pub categories: Vec<String>,
    #[serde(default)]
    pub counter_unit: Option<String>,
    #[serde(default)]
    pub client_uuid: Option<String>,
}

impl TypeBody {
    fn normalized(self) -> Result<(TypeInput, Option<String>), AppError> {
        let input = TypeInput {
            name: self.name,
            icon: self.icon,
            categories: self.categories,
            counter_unit: self.counter_unit,
        };
        let input = custom_type::normalize(input).map_err(|e| AppError::Invalid {
            code: e.code,
            message: e.message,
        })?;
        Ok((input, self.client_uuid))
    }
}

/// Whether another of the user's live types already has this name, ignoring case and accents.
/// Folded in Rust with `tags::fold` rather than compared in SQL: neither backend's `lower()`
/// strips accents, and a user has a handful of types, not thousands.
///
/// On the write transaction's own connection, so two creates racing with one name cannot both
/// pass it. Sync apply and import ask it too, so every door holds the same rule.
pub(crate) async fn name_taken(
    conn: &mut sqlx::AnyConnection,
    user_id: i64,
    name: &str,
    except: Option<i64>,
) -> Result<bool, AppError> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, name FROM object_types WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let wanted = fold(name);
    Ok(rows
        .iter()
        .any(|(id, other)| Some(*id) != except && fold(other) == wanted))
}

async fn load_owned(db: &sqlx::AnyPool, user_id: i64, id: i64) -> Result<TypeRow, AppError> {
    sqlx::query_as(
        "SELECT id, client_uuid, name, icon, categories, counter_unit, created_at, updated_at \
         FROM object_types WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn list(user: AuthUser, State(state): State<App>) -> Result<Json<Vec<TypeOut>>, AppError> {
    // Case-insensitive on both backends, as the objects list is -- see `Backend::name_order`.
    let order = state.backend.name_order("name");
    let rows = sqlx::query_as::<_, TypeRow>(sqlx::AssertSqlSafe(format!(
        "SELECT id, client_uuid, name, icon, categories, counter_unit, created_at, updated_at \
         FROM object_types WHERE user_id = $1 AND deleted_at IS NULL ORDER BY {order}, id"
    )))
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(TypeOut::from).collect()))
}

async fn create(
    user: AuthUser,
    State(state): State<App>,
    Json(body): Json<TypeBody>,
) -> Result<(StatusCode, Json<TypeOut>), AppError> {
    let (input, client_uuid) = body.normalized()?;
    // Lower case because the uuid becomes part of an object's `type`, which
    // `ObjectInput::validate` lower-cases: a type stored as `3F25...` could never be used.
    let client_uuid = super::normalize_client_uuid(client_uuid)?.map(|u| u.to_lowercase());
    if let Some(uuid) = client_uuid.as_deref() {
        // Idempotent on the caller's own live row, a conflict on anyone else's or a tombstone --
        // the same rule, and the same reasoning about checking outside the lock, as
        // `objects::create`.
        let existing: Option<(i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT id, user_id, deleted_at FROM object_types WHERE client_uuid = $1",
        )
        .bind(uuid)
        .fetch_optional(&state.db)
        .await?;
        match existing {
            Some((id, owner, None)) if owner == user.id => {
                return Ok((
                    StatusCode::OK,
                    Json(load_owned(&state.db, user.id, id).await?.into()),
                ));
            }
            Some(_) => return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }
    let uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let now = db::now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    if name_taken(&mut tx, user.id, &input.name, None).await? {
        return Err(name_taken_error());
    }
    let row = sqlx::query_as::<_, TypeRow>(
        "INSERT INTO object_types (user_id, client_uuid, name, icon, categories, counter_unit, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id, client_uuid, name, icon, categories, counter_unit, created_at, updated_at")
        .bind(user.id).bind(&uuid).bind(&input.name).bind(&input.icon)
        .bind(serde_json::to_string(&input.categories).unwrap_or_else(|_| "[]".into()))
        .bind(&input.counter_unit).bind(&now).bind(&now)
        .fetch_one(&mut *tx).await;
    let row = match row {
        Ok(row) => row,
        // Two replays of one client_uuid racing past the pre-check: the loser trips the unique
        // constraint, and the id is spoken for.
        Err(e)
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
        {
            tx.rollback().await?;
            return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into()));
        }
        Err(e) => return Err(e.into()),
    };
    // Logged like an object create, so a device pulls the new type and a stale offline edit
    // stamped before this moment loses to it.
    record::record_create(
        &mut tx,
        user.id,
        Entity::ObjectType,
        &uuid,
        &record::edited_at_now(),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row.into())))
}

async fn update(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    Json(body): Json<TypeBody>,
) -> Result<Json<TypeOut>, AppError> {
    let (input, _) = body.normalized()?;
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let existing: Option<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT client_uuid, name, icon, categories, counter_unit FROM object_types \
         WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((uuid, old_name, old_icon, old_categories, old_unit)) = existing else {
        return Err(AppError::NotFound);
    };
    // Its own row excluded: renaming "E-scooter" to "E-Scooter" is not a collision.
    if name_taken(&mut tx, user.id, &input.name, Some(id)).await? {
        return Err(name_taken_error());
    }
    let categories = serde_json::to_string(&input.categories).unwrap_or_else(|_| "[]".into());
    // PATCH replaces every field, but only the ones that differ are logged -- see
    // `record::record_update`. `categories` is logged as the JSON text the column holds.
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if input.name != old_name {
        changed.push(("name", json!(input.name)));
    }
    if input.icon != old_icon {
        changed.push(("icon", json!(input.icon)));
    }
    if categories != old_categories {
        changed.push(("categories", json!(categories)));
    }
    if input.counter_unit != old_unit {
        changed.push(("counter_unit", json!(input.counter_unit)));
    }
    let row = sqlx::query_as::<_, TypeRow>(
        "UPDATE object_types SET name = $1, icon = $2, categories = $3, counter_unit = $4, updated_at = $5 \
         WHERE id = $6 \
         RETURNING id, client_uuid, name, icon, categories, counter_unit, created_at, updated_at")
        .bind(&input.name).bind(&input.icon).bind(&categories)
        .bind(&input.counter_unit).bind(db::now()).bind(id)
        .fetch_one(&mut *tx).await?;
    if !changed.is_empty() {
        record::record_update(
            &mut tx,
            user.id,
            Entity::ObjectType,
            &uuid,
            &changed,
            &record::edited_at_now(),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(row.into()))
}

async fn delete(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let uuid: Option<(String,)> =
        sqlx::query_as("SELECT client_uuid FROM object_types WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL")
            .bind(id).bind(user.id)
            .fetch_optional(&mut *tx).await?;
    let Some((uuid,)) = uuid else {
        return Err(AppError::NotFound);
    };
    // Counted under the write lock, the same lock an object create or update takes to check its
    // type, so no object can start using this type between the count and the tombstone.
    // Tombstoned objects do not count: they are gone for the user, and nothing reads their type.
    let in_use: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM objects WHERE user_id = $1 AND type = $2 AND deleted_at IS NULL",
    )
    .bind(user.id)
    .bind(format!("{CUSTOM_PREFIX}{uuid}"))
    .fetch_one(&mut *tx)
    .await?;
    if in_use > 0 {
        return Err(AppError::InUse(in_use));
    }
    let now = db::now();
    sqlx::query("UPDATE object_types SET deleted_at = $1, updated_at = $2 WHERE id = $3")
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    record::record_delete(
        &mut tx,
        user.id,
        Entity::ObjectType,
        &uuid,
        &record::edited_at_now(),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
