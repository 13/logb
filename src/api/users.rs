use crate::auth::{self, AdminUser, AuthUser};
use crate::db;
use crate::error::AppError;
use crate::state::App;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

pub fn router() -> Router<App> {
    Router::new()
        .route("/users", get(list).post(create))
        .route("/users/{id}", axum::routing::patch(update).delete(delete))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct UserOut {
    pub id: i64,
    pub username: String,
    pub is_admin: crate::db::Bool,
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
    // `lower(username)`, not `username`: SQLite gets case-insensitive uniqueness from the
    // column's `COLLATE NOCASE`, which PostgreSQL has no equivalent of -- its schema declares a
    // unique index on `lower(username)` instead. Comparing the same way here makes the two
    // backends agree that "BEN" is taken when "Ben" exists, and lets PostgreSQL use that index.
    // Usernames are ASCII by `validate_username`, so `lower` folds all of one.
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE lower(username) = lower($1)")
        .bind(&body.username).fetch_optional(&state.db).await?;
    if exists.is_some() {
        return Err(AppError::Conflict("username already taken".into()));
    }
    // `is_admin` is bound as an integer, not a `bool`. `db::Bool` already handles reading the
    // column back from either backend; this is the writing half of the same decision. The
    // column is 0/1 on both -- SQLite has no boolean type and the PostgreSQL schema keeps the
    // same shape -- and PostgreSQL refuses a boolean parameter for a SMALLINT column outright,
    // which made every `POST /users` a 500 there. SQLite is indifferent.
    let user = sqlx::query_as::<_, UserOut>(
        "INSERT INTO users (username, password_hash, is_admin, lang, created_at) VALUES ($1, $2, $3, 'en', $4) \
         RETURNING id, username, is_admin, lang, created_at",
    )
    .bind(&body.username).bind(auth::hash_password(&body.password).await?).bind(i64::from(body.is_admin)).bind(db::now())
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
    /// The caller's present password. Required with `password` when the caller is changing
    /// their own (see `update`); ignored otherwise.
    pub current_password: Option<String>,
    pub is_admin: Option<bool>,
    pub lang: Option<String>,
}

/// Changes a user's password, admin role or language.
///
/// Changing your OWN password needs `current_password`. A session is a weaker credential than
/// the password: it sits in a browser that may be left open on a shared computer, and an API
/// token reaches this route too. Without the check, whoever holds either could set a password
/// of their choosing -- which also ends every other session and token, locking the owner out of
/// their own account. Missing and wrong are refused alike with `AppError::WrongPassword`.
///
/// Every such attempt spends the same allowance a login does, per address and per username:
/// checking a guess here is checking a password, and without the limit a stolen session would
/// be an unthrottled oracle for it. The attempt is counted before anything is verified, so the
/// right answer is refused too once the allowance is spent -- exactly as at the login form.
///
/// An admin resetting SOMEONE ELSE's password is not asked for anything: the admin does not
/// know it, and resetting it is the whole point.
async fn update(
    me: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<UpdateUser>,
) -> Result<(CookieJar, Json<UserOut>), AppError> {
    if !me.is_admin.0 && me.id != id {
        return Err(AppError::Forbidden);
    }
    if body.is_admin.is_some() && !me.is_admin.0 {
        return Err(AppError::Forbidden);
    }
    let _ = sqlx::query_as::<_, UserOut>("SELECT id, username, is_admin, lang, created_at FROM users WHERE id = $1")
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
    if body.password.is_some() && me.id == id {
        auth::check_login_rate(&state, auth::client_ip(&state, &headers, peer))?;
        auth::check_username_rate(&state, &me.username)?;
        let current = body.current_password.as_deref().unwrap_or("");
        if current.is_empty() {
            return Err(AppError::WrongPassword);
        }
        let (stored,): (String,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = $1")
            .bind(id).fetch_one(&state.db).await?;
        if !auth::verify_password(current, &stored).await {
            return Err(AppError::WrongPassword);
        }
    }

    // Hashed before the write transaction opens, as `setup` does: Argon2 takes long enough that
    // holding the one writer connection (SQLite) or the advisory lock (PostgreSQL) through it
    // would stall every other write in the app.
    let new_hash = match &body.password {
        Some(p) => Some(auth::hash_password(p).await?),
        None => None,
    };

    // Every statement below runs in one write transaction, so a failure part-way can no longer
    // leave, say, the new password committed while the sessions opened with the old one live
    // on. Everything inside uses `tx` and nothing else: acquiring a second pooled connection
    // between `begin_write` and `commit` would, on SQLite, wait on the very connection this
    // transaction holds.
    let mut tx = db::begin_write(&state).await?;
    let mut new_token = None;
    if let Some(hash) = &new_hash {
        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(hash).bind(id).execute(&mut *tx).await?;
        // The new password only means anything if the sessions opened with the old one stop
        // working. Someone changing their own password keeps this browser signed in, on a
        // freshly issued session; every other session for that account is gone either way.
        auth::delete_sessions_for_user_in(&mut tx, id).await?;
        if me.id == id {
            new_token = Some(auth::create_session_in(&mut tx, id).await?);
        }
    }
    if let Some(a) = body.is_admin {
        // An integer, for the same reason as the INSERT above.
        sqlx::query("UPDATE users SET is_admin = $1 WHERE id = $2").bind(i64::from(a)).bind(id).execute(&mut *tx).await?;
    }
    if let Some(l) = &body.lang {
        sqlx::query("UPDATE users SET lang = $1 WHERE id = $2").bind(l).bind(id).execute(&mut *tx).await?;
    }
    let user = sqlx::query_as::<_, UserOut>("SELECT id, username, is_admin, lang, created_at FROM users WHERE id = $1")
        .bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    // The cookie only once the session it names has committed.
    let jar = match new_token {
        Some(token) => jar.add(auth::session_cookie(token, auth::wants_secure(&state, &headers))),
        None => jar,
    };
    Ok((jar, Json(user)))
}

/// Deleting a user has to unwind their data in dependency order by hand.
///
/// `DELETE FROM users` alone fails outright for anyone who has ever uploaded a file: the
/// cascade from `users` reaches `files` while `attachments.file_id` is declared
/// `ON DELETE RESTRICT`, and SQLite aborts the whole statement with a foreign key error. So
/// attachments go first, then objects (which cascades activities and reminders), then the
/// user's `files` rows, then the user. Sessions cascade from `users` on their own.
async fn delete(AdminUser(me): AdminUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    if me.id == id {
        return Err(AppError::BadRequest("cannot delete yourself".into()));
    }
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE id = $1")
        .bind(id).fetch_optional(&state.db).await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    // Read the blobs to clean up before the rows that name them are gone.
    let blobs: Vec<(i64, String)> = sqlx::query_as("SELECT id, sha256 FROM files WHERE user_id = $1")
        .bind(id).fetch_all(&state.db).await?;

    let mut tx = db::begin_write(&state).await?;
    sqlx::query("DELETE FROM attachments WHERE object_id IN (SELECT id FROM objects WHERE user_id = $1)")
        .bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM objects WHERE user_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM files WHERE user_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;

    for (file_id, sha) in blobs {
        super::attachments::discard_blob(&state, file_id, &sha).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
