use super::activities::load_owned_activity;
use super::insights::{usage_by_object, Usage};
use super::objects::{load_owned_object, stats, validate_date};
use crate::auth::AuthUser;
use crate::db;
use crate::domain::insights::estimated_date;
use crate::domain::reminder::{
    counter_until, days_until, is_due, is_upcoming, next_due, reading_status, snoozed_date, Every, Repeat,
    KIND_READING, KIND_SERVICE, MAX_EVERY,
};
use crate::error::AppError;
use crate::state::App;
use crate::sync::{record, Entity};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

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
    /// `service` or `reading` -- see `domain::reminder::KIND_READING`.
    pub kind: String,
    /// A reading reminder's interval; both null on a service reminder.
    pub every_n: Option<i64>,
    pub every_unit: Option<String>,
    /// The row's sync identity, so a client can name it in an op without a bootstrap first.
    pub client_uuid: Option<String>,
    // joined
    pub object_name: String,
    /// The object's tags, answered as an array like `ObjectRow::tags`: a reminder listed on the
    /// dashboard, away from its object, still shows which of several similar things it is about.
    #[serde(serialize_with = "crate::domain::tags::serialize_json_text")]
    pub object_tags: String,
    pub counter_unit: Option<String>,
    pub current_counter: Option<i64>,
    /// The date of the object's newest counter reading, up to `reading_horizon`.
    pub last_reading_date: Option<String>,
}

#[derive(Serialize)]
pub struct ReminderOut {
    #[serde(flatten)]
    pub row: ReminderRow,
    pub due: bool,
    /// Days from today until the due date; negative when it has passed. For a reading reminder,
    /// measured to `next_due_date`.
    pub days_until: Option<i64>,
    /// Counter units still to go; negative when passed.
    pub counter_until: Option<i64>,
    /// The date this reminder comes due by date: `due_date` for a service reminder, and the
    /// derived next reading date for a reading reminder, whose `due_date` is only its start.
    pub next_due_date: Option<String>,
    /// For a service reminder with a counter target that is not yet reached: the date the
    /// object's recent usage says the counter will get there. Never makes a reminder due.
    pub estimated_due_date: Option<String>,
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

/// The latest date a counter reading may carry and still count as "the last reading": one day
/// past the server's today.
///
/// The reading form dates a reading by the device's own calendar, the server by
/// `LOGB_TIMEZONE`, and the two disagree for hours every day wherever that is left at its UTC
/// default -- a reading logged at 00:30 in Berlin is dated tomorrow as far as the server knows.
/// Ignoring it would leave the reminder due right after the person did what it asked. One day
/// covers any timezone; a reading dated further out is a typo, and is still ignored.
pub(crate) fn reading_horizon() -> String {
    let today = today();
    today.succ_opt().unwrap_or(today).to_string()
}

impl ReminderOut {
    /// `usage` is the object's latest reading and daily rate, when it has one; it only ever
    /// adds `estimated_due_date`, so a caller without it gets an otherwise identical answer.
    pub fn build(row: ReminderRow, today: NaiveDate, usage: Option<Usage>) -> Self {
        let snoozed_until = row.snoozed_until.as_deref().and_then(parse_date);
        if row.kind == KIND_READING {
            let (due, next) = reading_status(
                today,
                row.due_date.as_deref().and_then(parse_date),
                row.last_reading_date.as_deref().and_then(parse_date),
                Every::from_parts(row.every_n, row.every_unit.as_deref()),
                snoozed_until,
            );
            return ReminderOut {
                due: row.done_at.is_none() && due,
                days_until: days_until(today, next),
                counter_until: None,
                next_due_date: next.map(|d| d.to_string()),
                estimated_due_date: None,
                row,
            };
        }
        let date = row.due_date.as_deref().and_then(parse_date);
        let due = row.done_at.is_none() && is_due(today, row.current_counter, date, row.due_counter, snoozed_until);
        let estimated_due_date = match (due, row.due_counter, usage) {
            (false, Some(target), Some(u)) => estimated_date(u.last, u.rate_milli, target).map(|d| d.to_string()),
            _ => None,
        };
        ReminderOut {
            due,
            days_until: days_until(today, date),
            counter_until: counter_until(row.current_counter, row.due_counter),
            next_due_date: row.due_date.clone(),
            estimated_due_date,
            row,
        }
    }

    /// The nearer of the real due date and the usage estimate, in days from today -- what the
    /// upcoming lookahead measures, so a mileage-only service shows up once it is near.
    fn soonest_days(&self, today: NaiveDate) -> Option<i64> {
        let estimated = days_until(today, self.estimated_due_date.as_deref().and_then(parse_date));
        match (self.days_until, estimated) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

impl From<ReminderRow> for ReminderOut {
    fn from(row: ReminderRow) -> Self {
        ReminderOut::build(row, today(), None)
    }
}

/// The reminder columns every read shares, with `where_and_order` appended. `$1` is always
/// `reading_horizon()` (for `last_reading_date`), so a caller's own parameters start at `$2`.
///
/// One copy, so a column added to `ReminderRow` cannot reach one read and not another.
pub(crate) fn select_reminders(where_and_order: &str) -> String {
    format!(
        "SELECT r.id, r.object_id, r.title, r.notes, r.due_date, r.due_counter, r.repeat_months, \
         r.repeat_counter, r.done_at, r.done_activity_id, r.created_at, r.snoozed_until, \
         r.kind, r.every_n, r.every_unit, r.client_uuid, o.name AS object_name, o.tags AS object_tags, o.counter_unit, \
         (SELECT MAX(counter_value) FROM activities a WHERE a.object_id = o.id AND a.deleted_at IS NULL) AS current_counter, \
         (SELECT MAX(a.date) FROM activities a WHERE a.object_id = o.id AND a.deleted_at IS NULL \
            AND a.counter_value IS NOT NULL AND a.date <= $1) AS last_reading_date \
         FROM reminders r JOIN objects o ON o.id = r.object_id {where_and_order}"
    )
}

async fn load_owned(state: &App, user_id: i64, id: i64) -> Result<ReminderRow, AppError> {
    // The only text spliced in is the literal clause below; the ids stay bind parameters.
    sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(select_reminders(
        "WHERE r.id = $2 AND o.user_id = $3 AND r.deleted_at IS NULL AND o.deleted_at IS NULL",
    )))
    .bind(reading_horizon()).bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

/// A single reminder as the API answers it, with its object's usage for the estimate.
async fn out(state: &App, user_id: i64, row: ReminderRow) -> Result<ReminderOut, AppError> {
    let usage = usage_by_object(state, Some(user_id), Some(row.object_id)).await?.remove(&row.object_id);
    Ok(ReminderOut::build(row, today(), usage))
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
    #[serde(default = "service_kind")]
    pub kind: String,
    #[serde(default)]
    pub every_n: Option<i64>,
    #[serde(default)]
    pub every_unit: Option<String>,
    /// Identity minted by the client before the server saw the row. A replay carrying the
    /// same value answers with the row the first attempt made. See `super::normalize_client_uuid`.
    #[serde(default)]
    pub client_uuid: Option<String>,
}

pub(crate) fn service_kind() -> String {
    KIND_SERVICE.to_string()
}

impl ReminderInput {
    pub(crate) fn validate(&mut self, counter_unit: Option<&str>) -> Result<(), AppError> {
        self.title = self.title.trim().to_string();
        if self.title.is_empty() { return Err(AppError::BadRequest("title is required".into())); }
        if let Some(d) = &self.due_date { validate_date(d)?; }
        match self.kind.as_str() {
            KIND_READING => {
                if counter_unit.is_none() {
                    return Err(AppError::BadRequest("this object has no counter".into()));
                }
                if self.due_counter.is_some() || self.repeat_months.is_some() || self.repeat_counter.is_some() {
                    return Err(AppError::BadRequest(
                        "a reading reminder takes every_n and every_unit, not due_counter or repeat_*".into(),
                    ));
                }
                if Every::from_parts(self.every_n, self.every_unit.as_deref()).is_none() {
                    return Err(AppError::BadRequest(format!(
                        "every_n must be 1..{MAX_EVERY} and every_unit week or month"
                    )));
                }
                // The start is optional on the way in: "from today" is what leaving it out means.
                if self.due_date.is_none() { self.due_date = Some(db::today()); }
            }
            KIND_SERVICE => {
                if self.every_n.is_some() || self.every_unit.is_some() {
                    return Err(AppError::BadRequest("every_n and every_unit belong to a reading reminder".into()));
                }
                if self.due_date.is_none() && self.due_counter.is_none() {
                    return Err(AppError::BadRequest("due_date or due_counter is required".into()));
                }
                if (self.due_counter.is_some() || self.repeat_counter.is_some()) && counter_unit.is_none() {
                    return Err(AppError::BadRequest("this object has no counter".into()));
                }
                if matches!(self.due_counter, Some(c) if c < 0) { return Err(AppError::BadRequest("due_counter must be >= 0".into())); }
                if matches!(self.repeat_months, Some(m) if m <= 0) { return Err(AppError::BadRequest("repeat_months must be > 0".into())); }
                if matches!(self.repeat_counter, Some(c) if c <= 0) { return Err(AppError::BadRequest("repeat_counter must be > 0".into())); }
            }
            _ => return Err(AppError::BadRequest("kind must be service or reading".into())),
        }
        Ok(())
    }
}

/// Inserts a reminder row inside the caller's transaction and returns `(id, client_uuid)`, so
/// every caller can log the create in the same transaction as the write (see `sync::record`).
async fn insert(
    tx: &mut sqlx::AnyConnection,
    object_id: i64,
    b: &ReminderInput,
    uuid: String,
) -> Result<(i64, String), AppError> {
    let inserted: Result<(i64,), sqlx::Error> = sqlx::query_as(
        "INSERT INTO reminders (object_id, title, notes, due_date, due_counter, repeat_months, repeat_counter, created_at, client_uuid, kind, every_n, every_unit) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id",
    )
    .bind(object_id).bind(&b.title).bind(&b.notes).bind(&b.due_date).bind(b.due_counter)
    .bind(b.repeat_months).bind(b.repeat_counter).bind(db::now()).bind(&uuid)
    .bind(&b.kind).bind(b.every_n).bind(&b.every_unit)
    .fetch_one(&mut *tx).await;
    let (id,) = match inserted {
        Ok(row) => row,
        // A replay of one client_uuid racing past `create`'s pre-check trips the unique index
        // on `client_uuid`; the id is spoken for, so that is the same conflict.
        Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
            return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into()));
        }
        Err(e) => return Err(e.into()),
    };
    Ok((id, uuid))
}

async fn list(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>) -> Result<Json<Vec<ReminderOut>>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let rows = sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(select_reminders(
        "WHERE r.object_id = $2 AND r.deleted_at IS NULL AND o.deleted_at IS NULL \
         ORDER BY r.done_at IS NOT NULL, r.due_date IS NULL, r.due_date, r.due_counter, r.id",
    )))
    .bind(reading_horizon()).bind(object_id).fetch_all(&state.db).await?;
    let usage = usage_by_object(&state, Some(user.id), Some(object_id)).await?.remove(&object_id);
    let today = today();
    Ok(Json(rows.into_iter().map(|r| ReminderOut::build(r, today, usage)).collect()))
}

/// Reminders that are due, plus those coming due within `within_days`. `0` -- the default --
/// reproduces the old behaviour exactly, which is what the daily digest wants.
pub async fn due_for_user(state: &App, user_id: i64, within_days: i64) -> Result<Vec<ReminderOut>, AppError> {
    let rows = sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(select_reminders(
        "WHERE o.user_id = $2 AND r.done_at IS NULL AND o.archived_at IS NULL \
           AND r.deleted_at IS NULL AND o.deleted_at IS NULL \
         ORDER BY r.due_date IS NULL, r.due_date, r.id",
    )))
    .bind(reading_horizon()).bind(user_id).fetch_all(&state.db).await?;
    let today = today();
    let usage: HashMap<i64, Usage> = if within_days > 0 {
        usage_by_object(state, Some(user_id), None).await?
    } else {
        // The estimate only ever feeds the lookahead, and a zero-day window has none.
        HashMap::new()
    };
    let mut out: Vec<ReminderOut> = rows
        .into_iter()
        .map(|r| {
            let u = usage.get(&r.object_id).copied();
            ReminderOut::build(r, today, u)
        })
        .filter(|r| r.row.done_at.is_none())
        .filter(|r| {
            let snoozed = r.row.snoozed_until.as_deref().and_then(parse_date);
            is_upcoming(today, r.due, r.soonest_days(today), within_days, snoozed)
        })
        .collect();
    // A reading reminder's real date is derived, not `due_date`, so the SQL order is only a
    // first pass: due first, then soonest.
    out.sort_by_key(|r| (!r.due, r.soonest_days(today).unwrap_or(i64::MAX)));
    Ok(out)
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
    let client_uuid = super::normalize_client_uuid(body.client_uuid.take())?;
    if let Some(uuid) = client_uuid.as_deref() {
        // Idempotent on the caller's own live row under this object; a conflict on anyone
        // else's, on another object's, or on a tombstone -- see `objects::create`.
        let existing: Option<(i64, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT r.id, r.object_id, o.user_id, r.deleted_at FROM reminders r \
             JOIN objects o ON o.id = r.object_id WHERE r.client_uuid = $1")
            .bind(uuid).fetch_optional(&state.db).await?;
        match existing {
            Some((id, oid, owner, None)) if owner == user.id && oid == object_id => {
                let row = load_owned(&state, user.id, id).await?;
                return Ok((StatusCode::OK, Json(out(&state, user.id, row).await?)));
            }
            Some(_) => return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }
    let uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let (id, uuid) = insert(&mut tx, object_id, &body, uuid).await?;
    record::record_create(&mut tx, user.id, Entity::Reminder, &uuid, &edited_at).await?;
    tx.commit().await?;
    let row = load_owned(&state, user.id, id).await?;
    Ok((StatusCode::CREATED, Json(out(&state, user.id, row).await?)))
}

async fn read(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ReminderOut>, AppError> {
    let row = load_owned(&state, user.id, id).await?;
    Ok(Json(out(&state, user.id, row).await?))
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(mut body): Json<ReminderInput>) -> Result<Json<ReminderOut>, AppError> {
    let existing = load_owned(&state, user.id, id).await?;
    // A reminder does not turn into the other kind: the two keep different fields, and a
    // service reminder's done history means nothing on a reading one. Delete and re-create.
    if body.kind != existing.kind {
        return Err(AppError::BadRequest("kind cannot change".into()));
    }
    body.validate(existing.counter_unit.as_deref())?;

    // Only fields whose value actually differs are logged (see `record::record_update`).
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if body.title != existing.title { changed.push(("title", json!(body.title))); }
    if body.notes != existing.notes { changed.push(("notes", json!(body.notes))); }
    if body.due_date != existing.due_date { changed.push(("due_date", json!(body.due_date))); }
    if body.due_counter != existing.due_counter { changed.push(("due_counter", json!(body.due_counter))); }
    if body.repeat_months != existing.repeat_months { changed.push(("repeat_months", json!(body.repeat_months))); }
    if body.repeat_counter != existing.repeat_counter { changed.push(("repeat_counter", json!(body.repeat_counter))); }
    if body.every_n != existing.every_n { changed.push(("every_n", json!(body.every_n))); }
    if body.every_unit != existing.every_unit { changed.push(("every_unit", json!(body.every_unit))); }

    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query(
        "UPDATE reminders SET title = $1, notes = $2, due_date = $3, due_counter = $4, repeat_months = $5, repeat_counter = $6, \
         every_n = $7, every_unit = $8 WHERE id = $9 AND deleted_at IS NULL",
    )
    .bind(&body.title).bind(&body.notes).bind(&body.due_date).bind(body.due_counter)
    .bind(body.repeat_months).bind(body.repeat_counter).bind(body.every_n).bind(&body.every_unit).bind(id)
    .execute(&mut *tx).await?;
    if !changed.is_empty() {
        let uuid = record::uuid_of(&mut tx, Entity::Reminder, id).await?;
        record::record_update(&mut tx, user.id, Entity::Reminder, &uuid, &changed, &record::edited_at_now()).await?;
    }
    tx.commit().await?;
    let row = load_owned(&state, user.id, id).await?;
    Ok(Json(out(&state, user.id, row).await?))
}

/// Tombstoned rather than removed, so an offline client learns the reminder is gone. A
/// reminder has no children of its own, so there is no cascade to write out here.
async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    load_owned(&state, user.id, id).await?;
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let affected = sqlx::query("UPDATE reminders SET deleted_at = $1 WHERE id = $2 AND deleted_at IS NULL")
        .bind(db::now()).bind(id).execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    let uuid = record::uuid_of(&mut tx, Entity::Reminder, id).await?;
    record::record_delete(&mut tx, user.id, Entity::Reminder, &uuid, &edited_at).await?;
    tx.commit().await?;
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
    if r.kind == KIND_READING {
        // Marking one done would stop it for good -- `done_at` wins over everything -- when what
        // the user means is "I logged it", which the reading itself already says.
        return Err(AppError::Conflict("a reading reminder is satisfied by logging a reading, not marked done".into()));
    }
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
    let done_at = db::now();
    let edited_at = record::edited_at_now();

    // Computed before `begin()`, not after: `stats` acquires its own pooled connection, and
    // the pool is `max_connections(4)` (`db.rs`). Calling it while this handler's transaction
    // already holds the write lock lets four concurrent `done` calls each hold a connection
    // and block waiting for a fifth -- a deadlock until the acquire timeout, then a 500. The
    // only field of `stats`'s result this call reads is `current_counter`
    // (`MAX(activities.counter_value)`), and this transaction never writes `activities` -- so
    // hoisting the read changes nothing about `next_due`'s result. `stats`'s query also reads
    // `reminders`, for `due_reminder_count`, which this transaction DOES write (the UPDATE
    // below); that column is simply never looked at here, so its staleness is harmless.
    let base_date = activity.as_ref().and_then(|a| parse_date(&a.date)).unwrap_or_else(today);
    // Only fall back to the object's highest reading when the linked activity has none:
    // `.or(..)` on an awaited value would run the stats query even in the common case.
    let base_counter = match activity.as_ref().and_then(|a| a.counter_value) {
        Some(c) => Some(c),
        None => stats(&state, r.object_id).await?.current_counter,
    };
    let repeat = Repeat { months: r.repeat_months.map(|m| m as u32), counter: r.repeat_counter };
    let next_plan = next_due(base_date, base_counter, r.due_counter, repeat);

    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("UPDATE reminders SET done_at = $1, done_activity_id = $2 WHERE id = $3 AND deleted_at IS NULL")
        .bind(&done_at).bind(body.activity_id).bind(id)
        .execute(&mut *tx).await?;
    // `done_at` always changes: `r.done_at.is_some()` was already rejected above, so the old
    // value was NULL. `done_activity_id` only changes when the caller actually linked one --
    // leaving a reminder marked done without an activity keeps it NULL, which is not a change.
    let mut changed = vec![("done_at", json!(done_at))];
    if body.activity_id.is_some() {
        changed.push(("done_activity_id", json!(body.activity_id)));
    }
    let reminder_uuid = record::uuid_of(&mut tx, Entity::Reminder, id).await?;
    record::record_update(&mut tx, user.id, Entity::Reminder, &reminder_uuid, &changed, &edited_at).await?;

    let next_id = match next_plan {
        Some((date, counter)) => {
            let input = ReminderInput {
                title: r.title.clone(), notes: r.notes.clone(),
                due_date: date.map(|d| d.to_string()), due_counter: counter,
                repeat_months: r.repeat_months, repeat_counter: r.repeat_counter,
                kind: KIND_SERVICE.to_string(), every_n: None, every_unit: None,
                client_uuid: None,
            };
            let (nid, nuuid) = insert(&mut tx, r.object_id, &input, uuid::Uuid::new_v4().to_string()).await?;
            record::record_create(&mut tx, user.id, Entity::Reminder, &nuuid, &edited_at).await?;
            Some(nid)
        }
        None => None,
    };
    tx.commit().await?;

    let next = match next_id {
        Some(nid) => {
            let row = load_owned(&state, user.id, nid).await?;
            Some(out(&state, user.id, row).await?)
        }
        None => None,
    };
    let done_row = load_owned(&state, user.id, id).await?;
    Ok(Json(DoneOut { done: out(&state, user.id, done_row).await?, next }))
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
///
/// A reading reminder is measured from its derived next date, not from `due_date` (its start):
/// "skip this month" on a reading due next week lands a month after next week.
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
    let today = today();
    let current_due = ReminderOut::build(r.clone(), today, None).next_due_date.as_deref().and_then(parse_date);
    let until = snoozed_date(today, current_due, body.days).to_string();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("UPDATE reminders SET snoozed_until = $1 WHERE id = $2 AND deleted_at IS NULL")
        .bind(&until)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if r.snoozed_until.as_deref() != Some(until.as_str()) {
        let uuid = record::uuid_of(&mut tx, Entity::Reminder, id).await?;
        record::record_update(
            &mut tx, user.id, Entity::Reminder, &uuid, &[("snoozed_until", json!(until))],
            &record::edited_at_now(),
        )
        .await?;
    }
    tx.commit().await?;
    let row = load_owned(&state, user.id, id).await?;
    Ok(Json(out(&state, user.id, row).await?))
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
/// (`done_at.is_some()` always wins in `ReminderOut::build`), so there is nothing to guard.
async fn unsnooze(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ReminderOut>, AppError> {
    let r = load_owned(&state, user.id, id).await?;
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("UPDATE reminders SET snoozed_until = NULL WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if r.snoozed_until.is_some() {
        let uuid = record::uuid_of(&mut tx, Entity::Reminder, id).await?;
        record::record_update(
            &mut tx, user.id, Entity::Reminder, &uuid, &[("snoozed_until", serde_json::Value::Null)],
            &record::edited_at_now(),
        )
        .await?;
    }
    tx.commit().await?;
    let row = load_owned(&state, user.id, id).await?;
    Ok(Json(out(&state, user.id, row).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::insights::Reading;

    /// A `ReminderRow` with sensible defaults, so each test only sets the fields it cares about.
    fn row() -> ReminderRow {
        ReminderRow {
            id: 1, object_id: 1, title: "Oil change".into(), notes: "".into(),
            due_date: None, due_counter: None, repeat_months: None, repeat_counter: None,
            done_at: None, done_activity_id: None, created_at: "2024-01-01T00:00:00Z".into(),
            snoozed_until: None, kind: KIND_SERVICE.into(), every_n: None, every_unit: None, client_uuid: None,
            object_name: "Golf".into(), object_tags: "[]".into(), counter_unit: None, current_counter: None, last_reading_date: None,
        }
    }

    fn d(s: &str) -> NaiveDate { parse_date(s).unwrap() }

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

    #[test]
    fn a_reading_reminder_reports_its_derived_date_not_its_start() {
        let reading = ReminderRow {
            kind: KIND_READING.into(), every_n: Some(1), every_unit: Some("month".into()),
            due_date: Some("2026-01-01".into()), last_reading_date: Some("2026-09-01".into()),
            ..row()
        };
        let out = ReminderOut::build(reading, d("2026-09-13"), None);
        assert!(!out.due);
        assert_eq!(out.next_due_date.as_deref(), Some("2026-10-01"));
        assert_eq!(out.days_until, Some(18));
    }

    #[test]
    fn usage_estimates_a_counter_target_but_never_makes_it_due() {
        let service = ReminderRow { due_counter: Some(60_000), current_counter: Some(59_000), ..row() };
        let usage = Usage { last: Reading { date: d("2026-09-01"), counter: 59_000 }, rate_milli: 25_000 };
        let out = ReminderOut::build(service, d("2026-09-13"), Some(usage));
        assert!(!out.due);
        assert_eq!(out.estimated_due_date.as_deref(), Some("2026-10-11"));
        assert_eq!(out.soonest_days(d("2026-09-13")), Some(28));
    }
}
