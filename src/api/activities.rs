use super::attachments::{self, AttachmentOut};
use super::objects::{load_owned_object, validate_date, ObjectRow};
use crate::auth::AuthUser;
use crate::db;
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use crate::sync::apply::{canonical_edited_at, wins};
use crate::sync::{record, Entity};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

// Kept in sync with the CHECK on activities.category (migrations 0009 and 0011 on SQLite, 0001
// and 0002 on PostgreSQL) and with frontend/src/lib/types.ts's CATEGORIES: four health
// categories alongside the original seven, so a `body` object can log a symptom, treatment,
// appointment or medication; and `reading`, an entry that is nothing but a counter value.
pub const CATEGORIES: [&str; 12] = [
    "maintenance", "repair", "purchase", "inspection", "modification", "fuel", "other",
    "symptom", "treatment", "appointment", "medication", "reading",
];

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
    /// The row's sync identity, so a client can name it in an op without a bootstrap first.
    pub client_uuid: Option<String>,
    /// Stored JSON text, answered as the array it holds -- see `ObjectRow::tags`.
    #[serde(serialize_with = "tags::serialize_json_text")]
    pub tags: String,
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
    /// When an edit was made, for an edit that reaches the server later than that -- one queued
    /// offline. Each field it changes is then written only if nothing newer has changed that
    /// field in the meantime (see `update`). Absent for an ordinary edit, which is made now.
    #[serde(default)]
    pub edited_at: Option<String>,
    /// Identity minted by the client before the server saw the row. A replay carrying the
    /// same value answers with the row the first attempt made. See `super::normalize_client_uuid`.
    #[serde(default)]
    pub client_uuid: Option<String>,
    /// Absent on create means no tags; absent on PATCH keeps the current ones. `validate`
    /// replaces a present list with its normalised form.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
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
        // A reading with no value records nothing at all.
        if self.category == "reading" && self.counter_value.is_none() {
            return Err(AppError::BadRequest("a reading needs counter_value".into()));
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
        if let Some(t) = &self.tags {
            self.tags = Some(tags::normalize(t).map_err(AppError::BadRequest)?);
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
         a.quantity_milli, a.client_op_id, a.created_at, a.updated_at, a.client_uuid, a.tags FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE a.id = $1 AND o.user_id = $2 AND a.deleted_at IS NULL AND o.deleted_at IS NULL",
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
    /// Only entries carrying this tag, compared ignoring case and accents.
    #[serde(default)]
    pub tag: Option<String>,
}

/// The `tag` filter as the key `domain::tags::fold` compares by, or `None` when there is no
/// filter. A blank value is no filter, not a filter nothing matches.
fn wanted_tag(q: &ListQuery) -> Option<String> {
    let tag = q.tag.as_deref()?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!tag.is_empty()).then(|| tags::fold(&tag))
}

/// How many activities match the filters, ignoring the page window -- the client needs it to
/// know whether a "load more" button belongs on screen.
pub async fn count_for_object(state: &App, object_id: i64, q: &ListQuery) -> Result<i64, AppError> {
    if let Some(wanted) = wanted_tag(q) {
        // Counted in Rust with the same `carries` the page uses, so the header and the page can
        // never disagree -- see `list_for_object` for why SQL cannot do this match.
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT tags FROM activities WHERE object_id = $1 AND deleted_at IS NULL \
             AND ($2 IS NULL OR category = $2) AND ($3 IS NULL OR date >= $3) AND ($4 IS NULL OR date <= $4)",
        )
        .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to)
        .fetch_all(&state.db).await?;
        return Ok(rows.iter().filter(|(t,)| tags::carries(t, &wanted)).count() as i64);
    }
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM activities WHERE object_id = $1 AND deleted_at IS NULL \
         AND ($2 IS NULL OR category = $2) AND ($3 IS NULL OR date >= $3) AND ($4 IS NULL OR date <= $4)",
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
    // A tag matches ignoring case and accents, which neither backend's LIKE does reliably
    // (SQLite folds ASCII only, and neither strips accents). So with a tag filter SQL returns
    // every row the other filters allow and the match and the page window are applied here. One
    // object's timeline is hundreds of rows at most, so that costs nothing noticeable.
    let wanted = wanted_tag(q);
    let (sql_limit, sql_offset) = if wanted.is_some() { (i64::MAX, 0) } else { (limit, offset) };
    let rows = sqlx::query_as::<_, ActivityRow>(
        "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags \
         FROM activities WHERE object_id = $1 AND deleted_at IS NULL \
         AND ($2 IS NULL OR category = $2) AND ($3 IS NULL OR date >= $3) AND ($4 IS NULL OR date <= $4) \
         ORDER BY date DESC, id DESC LIMIT $5 OFFSET $6",
    )
    .bind(object_id).bind(&q.category).bind(&q.from).bind(&q.to).bind(sql_limit).bind(sql_offset)
    .fetch_all(&state.db).await?;
    Ok(match wanted {
        None => rows,
        Some(wanted) => rows.into_iter()
            .filter(|r| tags::carries(&r.tags, &wanted))
            .skip(offset as usize).take(limit as usize)
            .collect(),
    })
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
    //
    // `a.object_id` is in the GROUP BY only so the subqueries may name it: PostgreSQL refuses
    // an ungrouped outer column inside a subquery, while SQLite allows it. It groups nothing
    // differently -- the WHERE clause has already pinned `object_id` to a single value -- so
    // the rows are the same on both backends.
    let rows = sqlx::query_as::<_, TitleSuggestion>(
        "SELECT a.title, a.category, MAX(a.date) AS last_date, \
           (SELECT x.cost_cents FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category AND x.deleted_at IS NULL \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_cost_cents, \
           (SELECT x.counter_value FROM activities x WHERE x.object_id = a.object_id \
              AND x.title = a.title AND x.category = a.category AND x.deleted_at IS NULL \
              ORDER BY x.date DESC, x.id DESC LIMIT 1) AS last_counter \
         FROM activities a WHERE a.object_id = $1 AND a.deleted_at IS NULL \
         GROUP BY a.object_id, a.title, a.category ORDER BY last_date DESC LIMIT $2",
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
    // A retry after a lost response must resolve to the row the first attempt made. A
    // tombstoned row is deliberately not that row: the op id stays taken (the unique index
    // spans tombstones too), but the activity it named is gone, and handing a deleted row
    // back as if it were live would leak it. `op_id_conflict` is what that case becomes.
    if let Some(op) = body.client_op_id.as_deref() {
        if let Some(existing) = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
             quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags \
             FROM activities WHERE client_op_id = $1 AND deleted_at IS NULL",
        )
        .bind(op)
        .fetch_optional(&state.db)
        .await?
        {
            return op_id_row_response(&state, existing, object_id).await;
        }
    }
    let client_uuid = super::normalize_client_uuid(body.client_uuid.take())?;
    if let Some(uuid) = client_uuid.as_deref() {
        // Idempotent on the caller's own live row under this object; a conflict on anyone
        // else's, on another object's, or on a tombstone -- see `objects::create`.
        let existing: Option<(i64, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT a.id, a.object_id, o.user_id, a.deleted_at FROM activities a \
             JOIN objects o ON o.id = a.object_id WHERE a.client_uuid = $1")
            .bind(uuid).fetch_optional(&state.db).await?;
        match existing {
            Some((id, oid, owner, None)) if owner == user.id && oid == object_id => {
                let row = load_owned_activity(&state, user.id, id).await?;
                return Ok((StatusCode::OK, Json(one_out(&state, row).await?)).into_response());
            }
            Some(_) => return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }
    let now = db::now();
    let activity_uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let inserted = sqlx::query_as::<_, ActivityRow>(
        "INSERT INTO activities (object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
         RETURNING id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags",
    )
    .bind(object_id).bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(&body.client_op_id).bind(&now).bind(&now)
    .bind(&activity_uuid).bind(tags::to_json(body.tags.as_deref().unwrap_or_default()))
    .fetch_one(&mut *tx).await;
    let row = match inserted {
        Ok(row) => row,
        // Two concurrent requests carrying the same client_op_id -- e.g. several browser tabs
        // sharing one offline outbox, all flushing on reconnect -- can both pass the pre-check
        // above before either has inserted. The loser's INSERT then trips the partial unique
        // index instead of the pre-check catching it; treat that exactly like the pre-check
        // would have, by adopting the winner's row rather than failing the request.
        Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
            tx.rollback().await?;
            // Without an op id the only unique index this insert can trip is `client_uuid`:
            // a replay raced past the pre-check above, and the id is spoken for either way.
            let Some(op) = body.client_op_id.as_deref() else {
                return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into()));
            };
            let winner = sqlx::query_as::<_, ActivityRow>(
                "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
                 quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags \
                 FROM activities WHERE client_op_id = $1 AND deleted_at IS NULL",
            )
            .bind(op)
            .fetch_optional(&state.db).await?;
            // `fetch_optional` rather than `fetch_one`: the row holding this op id may be a
            // tombstone, which the filter above hides. That is not a 500 -- the id really is
            // taken, so say so.
            let Some(winner) = winner else { return Err(super::op_id_conflict()) };
            return op_id_row_response(&state, winner, object_id).await;
        }
        Err(e) => return Err(e.into()),
    };
    record::record_create(&mut tx, user.id, Entity::Activity, &activity_uuid, &edited_at).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(one_out(&state, row).await?)).into_response())
}

/// The row a client_op_id lookup found -- whether from the pre-check or after losing an
/// insert race -- may belong to a different object than the one being posted to; that's a
/// 409, not this object's row.
async fn op_id_row_response(state: &App, existing: ActivityRow, object_id: i64) -> Result<Response, AppError> {
    if existing.object_id != object_id {
        return Err(super::op_id_conflict());
    }
    Ok((StatusCode::OK, Json(one_out(state, existing).await?)).into_response())
}

async fn read(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Json<ActivityOut>, AppError> {
    let row = load_owned_activity(&state, user.id, id).await?;
    Ok(Json(one_out(&state, row).await?))
}

/// Whether `field` of this activity was changed after `at` by anything else -- a later edit from
/// this or another browser, or a synced device. The same last-write-wins rule a sync `set` op is
/// held to (`sync::apply::wins`), against the same `field_clock`.
async fn changed_since(tx: &mut sqlx::AnyConnection, uuid: &str, field: &str, at: &str) -> Result<bool, AppError> {
    let stored: Option<(String, String)> = sqlx::query_as(
        "SELECT edited_at, device_id FROM field_clock WHERE entity = $1 AND entity_uuid = $2 AND field = $3")
        .bind(Entity::Activity.as_str()).bind(uuid).bind(field)
        .fetch_optional(&mut *tx).await?;
    Ok(stored.is_some_and(|(stored_at, stored_device)| !wins(at, record::DEVICE_ID, &stored_at, &stored_device)))
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(mut body): Json<ActivityInput>) -> Result<Json<ActivityOut>, AppError> {
    let existing = load_owned_activity(&state, user.id, id).await?;
    let object = load_owned_object(&state, user.id, existing.object_id).await?;
    body.validate(&object)?;

    // An edit made offline and sent now carries the moment it was made. Capped at now, so a
    // device whose clock runs ahead cannot make its edit beat every later one for hours.
    let edited_at = match body.edited_at.as_deref() {
        Some(raw) => {
            let at = canonical_edited_at(raw)
                .ok_or_else(|| AppError::BadRequest("edited_at must be an RFC 3339 timestamp".into()))?;
            Some(at.min(record::edited_at_now()))
        }
        None => None,
    };

    let mut tx = db::begin_write(&state.db, state.backend).await?;
    if let Some(at) = &edited_at {
        // Field by field, not all or nothing: a title fixed on the phone at the garage and a cost
        // typed in on the desktop that evening are both kept, whichever arrives last.
        let uuid = record::uuid_of(&mut tx, Entity::Activity, id).await?;
        if body.date != existing.date && changed_since(&mut tx, &uuid, "date", at).await? { body.date = existing.date.clone(); }
        if body.category != existing.category && changed_since(&mut tx, &uuid, "category", at).await? { body.category = existing.category.clone(); }
        if body.title != existing.title && changed_since(&mut tx, &uuid, "title", at).await? { body.title = existing.title.clone(); }
        if body.notes != existing.notes && changed_since(&mut tx, &uuid, "notes", at).await? { body.notes = existing.notes.clone(); }
        if body.counter_value != existing.counter_value && changed_since(&mut tx, &uuid, "counter_value", at).await? { body.counter_value = existing.counter_value; }
        if body.cost_cents != existing.cost_cents && changed_since(&mut tx, &uuid, "cost_cents", at).await? { body.cost_cents = existing.cost_cents; }
        if body.quantity_milli != existing.quantity_milli && changed_since(&mut tx, &uuid, "quantity_milli", at).await? { body.quantity_milli = existing.quantity_milli; }
        // `None` is "keep the stored tags", so losing to a newer edit is spelled by dropping them.
        if body.tags.as_deref().is_some_and(|t| tags::to_json(t) != existing.tags) && changed_since(&mut tx, &uuid, "tags", at).await? { body.tags = None; }
        // Keeping some fields and not others can combine into something no single edit said --
        // a reading without its value -- so the merge is held to the same rules as either edit.
        body.validate(&object)?;
    }

    // Only fields whose value actually differs are logged -- a PATCH that rewrites a field
    // with its existing value produces no `changes` row (see `record::record_update`).
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if body.date != existing.date { changed.push(("date", json!(body.date))); }
    if body.category != existing.category { changed.push(("category", json!(body.category))); }
    if body.title != existing.title { changed.push(("title", json!(body.title))); }
    if body.notes != existing.notes { changed.push(("notes", json!(body.notes))); }
    if body.counter_value != existing.counter_value { changed.push(("counter_value", json!(body.counter_value))); }
    if body.cost_cents != existing.cost_cents { changed.push(("cost_cents", json!(body.cost_cents))); }
    if body.quantity_milli != existing.quantity_milli { changed.push(("quantity_milli", json!(body.quantity_milli))); }
    // Logged as the JSON text the column holds, as `objects::update` does.
    let tags = body.tags.as_deref().map(tags::to_json).unwrap_or_else(|| existing.tags.clone());
    if tags != existing.tags { changed.push(("tags", json!(tags))); }

    sqlx::query(
        "UPDATE activities SET date = $1, category = $2, title = $3, notes = $4, counter_value = $5, cost_cents = $6, quantity_milli = $7, updated_at = $8, tags = $9 WHERE id = $10 AND deleted_at IS NULL",
    )
    .bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(db::now()).bind(&tags).bind(id)
    .execute(&mut *tx).await?;
    if !changed.is_empty() {
        let uuid = record::uuid_of(&mut tx, Entity::Activity, id).await?;
        // The clock records when the edit was made, so a still-older queued edit arriving after
        // this one loses to it too.
        let clock = edited_at.clone().unwrap_or_else(record::edited_at_now);
        record::record_update(&mut tx, user.id, Entity::Activity, &uuid, &changed, &clock).await?;
    }
    tx.commit().await?;

    let row = load_owned_activity(&state, user.id, id).await?;
    Ok(Json(one_out(&state, row).await?))
}

/// As with objects, the row is tombstoned rather than removed, and the cascades `ON DELETE
/// CASCADE` / `ON DELETE SET NULL` used to provide -- this activity's attachments, and any
/// reminder `done_activity_id` points at it -- are written out by hand, in one transaction so
/// a half-applied delete cannot survive a failure.
///
/// The cover subquery deliberately does *not* skip tombstoned attachments: an object still
/// pointing at one has a stale cover, and clearing it is the whole point of the statement.
async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    load_owned_activity(&state, user.id, id).await?;
    let now = db::now();
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let affected = sqlx::query("UPDATE activities SET deleted_at = $1, updated_at = $2 WHERE id = $3 AND deleted_at IS NULL")
        .bind(&now).bind(&now).bind(id)
        .execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    // `record::cascade_activity` is the single copy of this cascade (the object's cover, a
    // reminder's `done_activity_id`, and the activity's attachments), shared with
    // `sync::apply::apply_op`'s `delete` handling -- see the module docs on `sync::record`.
    let activity_uuid = record::uuid_of(&mut tx, Entity::Activity, id).await?;
    let cascaded = record::cascade_activity(&mut tx, user.id, &activity_uuid, &now, &edited_at).await?;
    record::record_delete(&mut tx, user.id, Entity::Activity, &activity_uuid, &edited_at).await?;
    record::log_cascade(&mut tx, user.id, &edited_at, record::DEVICE_ID, &cascaded).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
