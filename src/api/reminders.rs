use super::activities::load_owned_activity;
use super::objects::{load_owned_object, stats, validate_date};
use crate::auth::AuthUser;
use crate::db;
use crate::domain::reminder::{counter_until, days_until, is_due, is_upcoming, next_due, snoozed_date, Repeat};
use crate::error::AppError;
use crate::state::App;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<App> {
    Router::new()
        .route("/objects/{id}/reminders", get(list).post(create))
        .route("/reminders/due", get(due_list))
        .route("/reminders/{id}", get(read).patch(update).delete(delete))
        .route("/reminders/{id}/done", post(done))
        .route("/reminders/{id}/snooze", post(snooze).delete(unsnooze))
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct ReminderRow {
    pub id: i64,
    pub object_id: i64,
    pub title: String,
    pub notes: String,
    pub due_date: Option<String>,
    pub due_counter: Option<i64>,
    pub repeat_months: Option<i64>,
    pub repeat_counter: Option<i64>,
    pub done_at: Option<String>,
    pub done_activity_id: Option<i64>,
    pub created_at: String,
    /// Hides the reminder from `due` until this date, without touching `due_date` or
    /// `due_counter` -- see `domain::reminder::is_due`.
    pub snoozed_until: Option<String>,
    // joined
    pub object_name: String,
    pub counter_unit: Option<String>,
    pub current_counter: Option<i64>,
}

#[derive(Serialize)]
pub struct ReminderOut {
    #[serde(flatten)]
    pub row: ReminderRow,
    pub due: bool,
    /// Days from today until the due date; negative when it has passed.
    pub days_until: Option<i64>,
    /// Counter units still to go; negative when passed.
    pub counter_until: Option<i64>,
}

/// Parses a stored `YYYY-MM-DD` date. Returns `None` on malformed input instead of
/// panicking -- a row's date column is not guaranteed valid (e.g. an archive import
/// bypassing normal validation), and a panic in a request handler is never acceptable.
fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn today() -> NaiveDate {
    parse_date(&db::today()).expect("server-generated date is always valid")
}

impl From<ReminderRow> for ReminderOut {
    fn from(row: ReminderRow) -> Self {
        let today = today();
        let date = row.due_date.as_deref().and_then(parse_date);
        let snoozed_until = row.snoozed_until.as_deref().and_then(parse_date);
        let due = row.done_at.is_none() && is_due(today, row.current_counter, date, row.due_counter, snoozed_until);
        let days_until = days_until(today, date);
        let counter_until = counter_until(row.current_counter, row.due_counter);
        ReminderOut { row, due, days_until, counter_until }
    }
}

async fn load_owned(state: &App, user_id: i64, id: i64) -> Result<ReminderRow, AppError> {
    sqlx::query_as::<_, ReminderRow>(
        "SELECT r.id, r.object_id, r.title, r.notes, r.due_date, r.due_counter, r.repeat_months, \
         r.repeat_counter, r.done_at, r.done_activity_id, r.created_at, r.snoozed_until, o.name AS object_name, o.counter_unit, \
         (SELECT MAX(counter_value) FROM activities a WHERE a.object_id = o.id) AS current_counter \
         FROM reminders r JOIN objects o ON o.id = r.object_id WHERE r.id = ? AND o.user_id = ?",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
pub struct ReminderInput {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub due_counter: Option<i64>,
    #[serde(default)]
    pub repeat_months: Option<i64>,
    #[serde(default)]
    pub repeat_counter: Option<i64>,
}

impl ReminderInput {
    pub(crate) fn validate(&mut self, counter_unit: Option<&str>) -> Result<(), AppError> {
        self.title = self.title.trim().to_string();
        if self.title.is_empty() { return Err(AppError::BadRequest("title is required".into())); }
        if let Some(d) = &self.due_date { validate_date(d)?; }
        if self.due_date.is_none() && self.due_counter.is_none() {
            return Err(AppError::BadRequest("due_date or due_counter is required".into()));
        }
        if (self.due_counter.is_some() || self.repeat_counter.is_some()) && counter_unit.is_none() {
            return Err(AppError::BadRequest("this object has no counter".into()));
        }
        if matches!(self.due_counter, Some(c) if c < 0) { return Err(AppError::BadRequest("due_counter must be >= 0".into())); }
        if matches!(self.repeat_months, Some(m) if m <= 0) { return Err(AppError::BadRequest("repeat_months must be > 0".into())); }
        if matches!(self.repeat_counter, Some(c) if c <= 0) { return Err(AppError::BadRequest("repeat_counter must be > 0".into())); }
        Ok(())
    }
}

async fn insert(state: &App, object_id: i64, b: &ReminderInput) -> Result<i64, AppError> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO reminders (object_id, title, notes, due_date, due_counter, repeat_months, repeat_counter, created_at, client_uuid) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(object_id).bind(&b.title).bind(&b.notes).bind(&b.due_date).bind(b.due_counter)
    .bind(b.repeat_months).bind(b.repeat_counter).bind(db::now()).bind(uuid::Uuid::new_v4().to_string())
    .fetch_one(&state.db).await?;
    Ok(id)
}

async fn list(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>) -> Result<Json<Vec<ReminderOut>>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let rows = sqlx::query_as::<_, ReminderRow>(
        "SELECT r.id, r.object_id, r.title, r.notes, r.due_date, r.due_counter, r.repeat_months, \
         r.repeat_counter, r.done_at, r.done_activity_id, r.created_at, r.snoozed_until, o.name AS object_name, o.counter_unit, \
         (SELECT MAX(counter_value) FROM activities a WHERE a.object_id = o.id) AS current_counter \
         FROM reminders r JOIN objects o ON o.id = r.object_id WHERE r.object_id = ? \
         ORDER BY r.done_at IS NOT NULL, r.due_date IS NULL, r.due_date, r.due_counter, r.id",
    )
    .bind(object_id).fetch_all(&state.db).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// Reminders that are due, plus those coming due within `within_days`. `0` -- the default --
/// reproduces the old behaviour exactly, which is what the daily digest wants.
pub async fn due_for_user(state: &App, user_id: i64, within_days: i64) -> Result<Vec<ReminderOut>, AppError> {
    let rows = sqlx::query_as::<_, ReminderRow>(
        "SELECT r.id, r.object_id, r.title, r.notes, r.due_date, r.due_counter, r.repeat_months, \
         r.repeat_counter, r.done_at, r.done_activity_id, r.created_at, r.snoozed_until, o.name AS object_name, o.counter_unit, \
         (SELECT MAX(counter_value) FROM activities a WHERE a.object_id = o.id) AS current_counter \
         FROM reminders r JOIN objects o ON o.id = r.object_id \
         WHERE o.user_id = ? AND r.done_at IS NULL AND o.archived_at IS NULL \
         ORDER BY r.due_date IS NULL, r.due_date, r.id",
    )
    .bind(user_id).fetch_all(&state.db).await?;
    let today = today();
    Ok(rows
        .into_iter()
        .map(ReminderOut::from)
        .filter(|r| r.row.done_at.is_none())
        .filter(|r| {
            let snoozed = r.row.snoozed_until.as_deref().and_then(parse_date);
            is_upcoming(today, r.due, r.days_until, within_days, snoozed)
        })
        .collect())
}

#[derive(Deserialize)]
pub struct DueQuery {
    #[serde(default)]
    pub within_days: i64,
}

async fn due_list(user: AuthUser, State(state): State<App>, Query(q): Query<DueQuery>) -> Result<Json<Vec<ReminderOut>>, AppError> {
    Ok(Json(due_for_user(&state, user.id, q.within_days.clamp(0, 365)).await?))
}

async fn create(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Json(mut body): Json<ReminderInput>) -> Result<(StatusCode, Json<ReminderOut>), AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    body.validate(object.counter_unit.as_deref())?;
    let id = insert(&state, object_id, &body).await?;
    Ok((StatusCode::CREATED, Json(load_owned(&state, user.id, id).await?.into())))
}

async fn read(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ReminderOut>, AppError> {
    Ok(Json(load_owned(&state, user.id, id).await?.into()))
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(mut body): Json<ReminderInput>) -> Result<Json<ReminderOut>, AppError> {
    let existing = load_owned(&state, user.id, id).await?;
    body.validate(existing.counter_unit.as_deref())?;
    sqlx::query(
        "UPDATE reminders SET title = ?, notes = ?, due_date = ?, due_counter = ?, repeat_months = ?, repeat_counter = ? WHERE id = ?",
    )
    .bind(&body.title).bind(&body.notes).bind(&body.due_date).bind(body.due_counter)
    .bind(body.repeat_months).bind(body.repeat_counter).bind(id)
    .execute(&state.db).await?;
    Ok(Json(load_owned(&state, user.id, id).await?.into()))
}

async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    load_owned(&state, user.id, id).await?;
    sqlx::query("DELETE FROM reminders WHERE id = ?").bind(id).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Default)]
pub struct DoneInput {
    #[serde(default)]
    pub activity_id: Option<i64>,
}

#[derive(Serialize)]
pub struct DoneOut {
    pub done: ReminderOut,
    pub next: Option<ReminderOut>,
}

async fn done(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, body: Option<Json<DoneInput>>) -> Result<Json<DoneOut>, AppError> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let r = load_owned(&state, user.id, id).await?;
    if r.done_at.is_some() {
        return Err(AppError::Conflict("reminder already done".into()));
    }
    let activity = match body.activity_id {
        Some(aid) => {
            let a = load_owned_activity(&state, user.id, aid).await?;
            if a.object_id != r.object_id {
                return Err(AppError::BadRequest("activity belongs to another object".into()));
            }
            Some(a)
        }
        None => None,
    };
    sqlx::query("UPDATE reminders SET done_at = ?, done_activity_id = ? WHERE id = ?")
        .bind(db::now()).bind(body.activity_id).bind(id)
        .execute(&state.db).await?;

    let base_date = activity.as_ref().and_then(|a| parse_date(&a.date)).unwrap_or_else(today);
    // Only fall back to the object's highest reading when the linked activity has none:
    // `.or(..)` on an awaited value would run the stats query even in the common case.
    let base_counter = match activity.as_ref().and_then(|a| a.counter_value) {
        Some(c) => Some(c),
        None => stats(&state, r.object_id).await?.current_counter,
    };
    let repeat = Repeat { months: r.repeat_months.map(|m| m as u32), counter: r.repeat_counter };
    let next = match next_due(base_date, base_counter, r.due_counter, repeat) {
        Some((date, counter)) => {
            let input = ReminderInput {
                title: r.title.clone(), notes: r.notes.clone(),
                due_date: date.map(|d| d.to_string()), due_counter: counter,
                repeat_months: r.repeat_months, repeat_counter: r.repeat_counter,
            };
            let nid = insert(&state, r.object_id, &input).await?;
            Some(load_owned(&state, user.id, nid).await?.into())
        }
        None => None,
    };
    Ok(Json(DoneOut { done: load_owned(&state, user.id, id).await?.into(), next }))
}

#[derive(Deserialize)]
pub struct SnoozeInput {
    pub days: i64,
}

/// Hide a reminder from `due` for `days`. Snoozing means "not now, in a week", so an overdue
/// reminder is measured from today rather than from the date it blew past.
///
/// This writes `snoozed_until`, never `due_date` or `due_counter`: the reminder keeps its real
/// due date and counter target, so `days_until`/`counter_until` stay truthful and a
/// counter-based reminder can actually be suppressed (rewriting `due_date` never touched it).
async fn snooze(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    Json(body): Json<SnoozeInput>,
) -> Result<Json<ReminderOut>, AppError> {
    if !(1..=365).contains(&body.days) {
        return Err(AppError::BadRequest("days must be between 1 and 365".into()));
    }
    let r = load_owned(&state, user.id, id).await?;
    if r.done_at.is_some() {
        return Err(AppError::Conflict("reminder already done".into()));
    }
    let until = snoozed_date(today(), r.due_date.as_deref().and_then(parse_date), body.days);
    sqlx::query("UPDATE reminders SET snoozed_until = ? WHERE id = ?")
        .bind(until.to_string())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(load_owned(&state, user.id, id).await?.into()))
}

/// Undo a snooze: clears `snoozed_until` so the reminder's normal due-ness (by date, by
/// counter, or both) applies again immediately.
///
/// Clearing a reminder that is not currently snoozed succeeds and changes nothing -- the
/// caller asked for "not snoozed", and that is already the state, so there is nothing to
/// reject. Treating it as an error would force every client to first check `snoozed_until`
/// before it could safely call this, for no benefit: the end state is identical either way.
///
/// A done reminder is *not* rejected here, unlike `snooze`: `snooze` rejects because there is
/// no due-ness left to suppress and creating a fresh snooze on a closed reminder would be
/// meaningless, but clearing `snoozed_until` on a done reminder is just tidying up stale state
/// on the way to that same no-op-success end state -- it cannot make a done reminder due again
/// (`done_at.is_some()` always wins in `ReminderOut::from`), so there is nothing to guard.
async fn unsnooze(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ReminderOut>, AppError> {
    load_owned(&state, user.id, id).await?;
    sqlx::query("UPDATE reminders SET snoozed_until = NULL WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(load_owned(&state, user.id, id).await?.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `ReminderRow` with sensible defaults, so each test only sets the fields it cares about.
    fn row() -> ReminderRow {
        ReminderRow {
            id: 1, object_id: 1, title: "Oil change".into(), notes: "".into(),
            due_date: None, due_counter: None, repeat_months: None, repeat_counter: None,
            done_at: None, done_activity_id: None, created_at: "2024-01-01T00:00:00Z".into(),
            snoozed_until: None,
            object_name: "Golf".into(), counter_unit: None, current_counter: None,
        }
    }

    #[test]
    fn unparseable_due_date_does_not_panic_and_is_not_due() {
        let bad = ReminderRow { due_date: Some("not-a-date".into()), done_at: None, ..row() };
        let out = ReminderOut::from(bad);
        assert!(!out.due, "an unparseable stored date must not make a reminder due");
    }

    #[test]
    fn valid_past_due_date_is_due() {
        let good = ReminderRow { due_date: Some("2020-01-01".into()), done_at: None, ..row() };
        let out = ReminderOut::from(good);
        assert!(out.due, "a valid past due_date must still mark the reminder due");
    }

    #[test]
    fn unparseable_due_date_does_not_block_the_counter_path() {
        let row = ReminderRow {
            due_date: Some("not-a-date".into()), done_at: None,
            due_counter: Some(10_000), current_counter: Some(10_000),
            ..row()
        };
        let out = ReminderOut::from(row);
        assert!(out.due, "a bad due_date must not stop the counter-based due check from firing");
    }
}
