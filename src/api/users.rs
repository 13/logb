use crate::auth::{self, AdminUser, AuthUser};
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<App> {
    Router::new()
        .route("/users", get(list).post(create))
        .route("/users/{id}", axum::routing::patch(update).delete(delete))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct UserOut {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub lang: String,
    pub created_at: String,
}

async fn list(AdminUser(_): AdminUser, State(state): State<App>) -> Result<Json<Vec<UserOut>>, AppError> {
    Ok(Json(sqlx::query_as::<_, UserOut>("SELECT id, username, is_admin, lang, created_at FROM users ORDER BY id")
        .fetch_all(&state.db).await?))
}

#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

async fn create(
    AdminUser(_): AdminUser,
    State(state): State<App>,
    Json(body): Json<CreateUser>,
) -> Result<(StatusCode, Json<UserOut>), AppError> {
    auth::validate_username(&body.username)?;
    auth::validate_password(&body.password)?;
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(&body.username).fetch_optional(&state.db).await?;
    if exists.is_some() {
        return Err(AppError::Conflict("username already taken".into()));
    }
    let user = sqlx::query_as::<_, UserOut>(
        "INSERT INTO users (username, password_hash, is_admin, lang, created_at) VALUES (?, ?, ?, 'en', ?) \
         RETURNING id, username, is_admin, lang, created_at",
    )
    .bind(&body.username).bind(auth::hash_password(&body.password)?).bind(body.is_admin).bind(db::now())
    .fetch_one(&state.db).await
    .map_err(|e| match e.as_database_error().filter(|d| d.is_unique_violation()) {
        Some(_) => AppError::Conflict("username already taken".into()),
        None => AppError::from(e),
    })?;
    Ok((StatusCode::CREATED, Json(user)))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    pub password: Option<String>,
    pub is_admin: Option<bool>,
    pub lang: Option<String>,
}

async fn update(
    me: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUser>,
) -> Result<Json<UserOut>, AppError> {
    if !me.is_admin && me.id != id {
        return Err(AppError::Forbidden);
    }
    if body.is_admin.is_some() && !me.is_admin {
        return Err(AppError::Forbidden);
    }
    let _ = sqlx::query_as::<_, UserOut>("SELECT id, username, is_admin, lang, created_at FROM users WHERE id = ?")
        .bind(id).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;

    // Validate every field before writing anything, so a later-rejected field
    // can't leave an earlier field's write committed.
    if let Some(p) = &body.password {
        auth::validate_password(p)?;
    }
    if let Some(a) = body.is_admin {
        if me.id == id && !a {
            return Err(AppError::BadRequest("cannot remove your own admin role".into()));
        }
    }
    if let Some(l) = &body.lang {
        if !matches!(l.as_str(), "en" | "de") {
            return Err(AppError::BadRequest("lang must be en or de".into()));
        }
    }

    if let Some(p) = &body.password {
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(auth::hash_password(p)?).bind(id).execute(&state.db).await?;
    }
    if let Some(a) = body.is_admin {
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?").bind(a).bind(id).execute(&state.db).await?;
    }
    if let Some(l) = &body.lang {
        sqlx::query("UPDATE users SET lang = ? WHERE id = ?").bind(l).bind(id).execute(&state.db).await?;
    }
    let user = sqlx::query_as::<_, UserOut>("SELECT id, username, is_admin, lang, created_at FROM users WHERE id = ?")
        .bind(id).fetch_one(&state.db).await?;
    Ok(Json(user))
}

async fn delete(AdminUser(me): AdminUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    if me.id == id {
        return Err(AppError::BadRequest("cannot delete yourself".into()));
    }
    let n = sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(&state.db).await?.rows_affected();
    if n == 0 { Err(AppError::NotFound) } else { Ok(StatusCode::NO_CONTENT) }
}
