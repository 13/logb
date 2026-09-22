//! Creating, changing and deleting one activity.

use crate::api::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::db;
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use crate::sync::apply::{canonical_edited_at, wins};
use crate::sync::{record, Entity};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use super::*;

pub(crate) async fn create(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
    Json(mut body): Json<ActivityInput>,
) -> Result<Response, AppError> {
    let object = load_owned_object(&state, user.id, object_id).await?;
    body.validate(&object)?;
    // A blank (or all-whitespace) client_op_id means no idempotency was requested, not a
    // real, indexable id -- see `crate::api::normalize_op_id` for why that distinction matters.
    body.client_op_id = crate::api::normalize_op_id(body.client_op_id.take());
    // A retry after a lost response must resolve to the row the first attempt made. A
    // tombstoned row is deliberately not that row: the op id stays taken (the unique index
    // spans tombstones too), but the activity it named is gone, and handing a deleted row
    // back as if it were live would leak it. `op_id_conflict` is what that case becomes.
    if let Some(op) = body.client_op_id.as_deref() {
        if let Some(existing) = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
             quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
             start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams, fuel_level_pct, meter_reading_milli, period_start, period_end, estimated, meter_reset \
             FROM activities WHERE client_op_id = $1 AND deleted_at IS NULL",
        )
        .bind(op)
        .fetch_optional(&state.db)
        .await?
        {
            return op_id_row_response(&state, existing, object_id).await;
        }
    }
    let client_uuid = crate::api::normalize_client_uuid(body.client_uuid.take())?;
    if let Some(uuid) = client_uuid.as_deref() {
        // Idempotent on the caller's own live row under this object; a conflict on anyone
        // else's, on another object's, or on a tombstone -- see `objects::create`.
        let existing: Option<(i64, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT a.id, a.object_id, o.user_id, a.deleted_at FROM activities a \
             JOIN objects o ON o.id = a.object_id WHERE a.client_uuid = $1",
        )
        .bind(uuid)
        .fetch_optional(&state.db)
        .await?;
        match existing {
            Some((id, oid, owner, None)) if owner == user.id && oid == object_id => {
                let row = load_owned_activity(&state, user.id, id).await?;
                return Ok((StatusCode::OK, Json(one_out(&state, row).await?)).into_response());
            }
            Some(_) => return Err(AppError::Conflict(crate::api::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }
    let now = db::now();
    let activity_uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    if body.category == "usage" {
        if let (Some(start), Some(end)) = (
            body.period_start.as_ref().and_then(|v| v.as_ref()),
            body.period_end.as_ref().and_then(|v| v.as_ref()),
        ) {
            let overlap: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM activities WHERE object_id = $1 AND category = 'usage' AND deleted_at IS NULL \
                 AND period_start IS NOT NULL AND period_end IS NOT NULL AND period_start <= $2 AND period_end >= $3 LIMIT 1")
                .bind(object_id).bind(end).bind(start).fetch_optional(&mut *tx).await?;
            if overlap.is_some() {
                return Err(AppError::Conflict(
                    "this billing period overlaps an existing usage entry".into(),
                ));
            }
        }
    }
    let inserted = sqlx::query_as::<_, ActivityRow>(
        "INSERT INTO activities (object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
         start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams, fuel_level_pct, meter_reading_milli, period_start, period_end, estimated, meter_reset) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26) \
         RETURNING id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
         start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams, fuel_level_pct, meter_reading_milli, period_start, period_end, estimated, meter_reset",
    )
    .bind(object_id).bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(&body.client_op_id).bind(&now).bind(&now)
    .bind(&activity_uuid).bind(tags::to_json(body.tags.as_deref().unwrap_or_default()))
    .bind(body.start_counter.flatten()).bind(body.from_place.clone().flatten()).bind(body.to_place.clone().flatten())
    .bind(body.duration_minutes.flatten()).bind(body.battery_used_pct.flatten())
    // Absent on create means "not full" -- see `ActivityInput::charged_full`'s doc comment.
    .bind(body.charged_full.unwrap_or(0)).bind(body.weight_grams.flatten()).bind(body.fuel_level_pct.flatten())
    .bind(body.meter_reading_milli.flatten()).bind(body.period_start.clone().flatten()).bind(body.period_end.clone().flatten())
    .bind(body.estimated.unwrap_or(0)).bind(body.meter_reset.unwrap_or(0))
    .fetch_one(&mut *tx).await;
    let row = match inserted {
        Ok(row) => row,
        // Two concurrent requests carrying the same client_op_id -- e.g. several browser tabs
        // sharing one offline outbox, all flushing on reconnect -- can both pass the pre-check
        // above before either has inserted. The loser's INSERT then trips the partial unique
        // index instead of the pre-check catching it; treat that exactly like the pre-check
        // would have, by adopting the winner's row rather than failing the request.
        Err(e)
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
        {
            tx.rollback().await?;
            // Without an op id the only unique index this insert can trip is `client_uuid`:
            // a replay raced past the pre-check above, and the id is spoken for either way.
            let Some(op) = body.client_op_id.as_deref() else {
                return Err(AppError::Conflict(crate::api::CLIENT_UUID_TAKEN.into()));
            };
            let winner = sqlx::query_as::<_, ActivityRow>(
                "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, \
                 quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags, \
                 start_counter, from_place, to_place, duration_minutes, battery_used_pct, charged_full, weight_grams, fuel_level_pct, meter_reading_milli, period_start, period_end, estimated, meter_reset \
                 FROM activities WHERE client_op_id = $1 AND deleted_at IS NULL",
            )
            .bind(op)
            .fetch_optional(&state.db).await?;
            // `fetch_optional` rather than `fetch_one`: the row holding this op id may be a
            // tombstone, which the filter above hides. That is not a 500 -- the id really is
            // taken, so say so.
            let Some(winner) = winner else {
                return Err(crate::api::op_id_conflict());
            };
            return op_id_row_response(&state, winner, object_id).await;
        }
        Err(e) => return Err(e.into()),
    };
    record::record_create(
        &mut tx,
        user.id,
        Entity::Activity,
        &activity_uuid,
        &edited_at,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(one_out(&state, row).await?)).into_response())
}

/// The row a client_op_id lookup found -- whether from the pre-check or after losing an
/// insert race -- may belong to a different object than the one being posted to; that's a
/// 409, not this object's row.
async fn op_id_row_response(
    state: &App,
    existing: ActivityRow,
    object_id: i64,
) -> Result<Response, AppError> {
    if existing.object_id != object_id {
        return Err(crate::api::op_id_conflict());
    }
    Ok((StatusCode::OK, Json(one_out(state, existing).await?)).into_response())
}

pub(crate) async fn read(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<Json<ActivityOut>, AppError> {
    let row = load_owned_activity(&state, user.id, id).await?;
    Ok(Json(one_out(&state, row).await?))
}

/// Whether `field` of this activity was changed after `at` by anything else -- a later edit from
/// this or another browser, or a synced device. The same last-write-wins rule a sync `set` op is
/// held to (`sync::apply::wins`), against the same `field_clock`.
async fn changed_since(
    tx: &mut sqlx::AnyConnection,
    uuid: &str,
    field: &str,
    at: &str,
) -> Result<bool, AppError> {
    let stored: Option<(String, String)> = sqlx::query_as(
        "SELECT edited_at, device_id FROM field_clock WHERE entity = $1 AND entity_uuid = $2 AND field = $3")
        .bind(Entity::Activity.as_str()).bind(uuid).bind(field)
        .fetch_optional(&mut *tx).await?;
    Ok(stored.is_some_and(|(stored_at, stored_device)| {
        !wins(at, record::DEVICE_ID, &stored_at, &stored_device)
    }))
}

pub(crate) async fn update(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    Json(mut body): Json<ActivityInput>,
) -> Result<Json<ActivityOut>, AppError> {
    let existing = load_owned_activity(&state, user.id, id).await?;
    let object = load_owned_object(&state, user.id, existing.object_id).await?;
    // Trip fields are three-state on PATCH (see `ActivityInput`'s doc comment on
    // `start_counter`): a key the client omitted must keep its stored value, unlike every other
    // field of this handler, which a PATCH always resends in full. Merged in before `validate`
    // so its `.flatten()` reads the stored value back exactly as if the client had sent it.
    if body.start_counter.is_none() {
        body.start_counter = Some(existing.start_counter);
    }
    if body.from_place.is_none() {
        body.from_place = Some(existing.from_place.clone());
    }
    if body.to_place.is_none() {
        body.to_place = Some(existing.to_place.clone());
    }
    if body.duration_minutes.is_none() {
        body.duration_minutes = Some(existing.duration_minutes);
    }
    if body.battery_used_pct.is_none() {
        body.battery_used_pct = Some(existing.battery_used_pct);
    }
    // `charged_full` keeps its stored value when omitted too (see `ActivityInput`'s doc comment
    // on the field), merged in the same way and for the same reason as the trip fields above.
    if body.weight_grams.is_none() {
        body.weight_grams = Some(existing.weight_grams);
    }
    if body.fuel_level_pct.is_none() {
        body.fuel_level_pct = Some(existing.fuel_level_pct);
    }
    if body.meter_reading_milli.is_none() {
        body.meter_reading_milli = Some(existing.meter_reading_milli);
    }
    if body.period_start.is_none() {
        body.period_start = Some(existing.period_start.clone());
    }
    if body.period_end.is_none() {
        body.period_end = Some(existing.period_end.clone());
    }
    if body.estimated.is_none() {
        body.estimated = Some(existing.estimated);
    }
    if body.meter_reset.is_none() {
        body.meter_reset = Some(existing.meter_reset);
    }
    if body.charged_full.is_none() {
        body.charged_full = Some(existing.charged_full);
    }
    body.validate(&object)?;

    // An edit made offline and sent now carries the moment it was made. Capped at now, so a
    // device whose clock runs ahead cannot make its edit beat every later one for hours.
    let edited_at = match body.edited_at.as_deref() {
        Some(raw) => {
            let at = canonical_edited_at(raw).ok_or_else(|| {
                AppError::BadRequest("edited_at must be an RFC 3339 timestamp".into())
            })?;
            Some(at.min(record::edited_at_now()))
        }
        None => None,
    };

    let mut tx = db::begin_write(&state.db, state.backend).await?;
    if body.category == "usage" {
        if let (Some(start), Some(end)) = (
            body.period_start.as_ref().and_then(|v| v.as_ref()),
            body.period_end.as_ref().and_then(|v| v.as_ref()),
        ) {
            let overlap: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM activities WHERE object_id = $1 AND id <> $2 AND category = 'usage' AND deleted_at IS NULL \
                 AND period_start IS NOT NULL AND period_end IS NOT NULL AND period_start <= $3 AND period_end >= $4 LIMIT 1")
                .bind(existing.object_id).bind(id).bind(end).bind(start).fetch_optional(&mut *tx).await?;
            if overlap.is_some() {
                return Err(AppError::Conflict(
                    "this billing period overlaps an existing usage entry".into(),
                ));
            }
        }
    }
    if let Some(at) = &edited_at {
        // Field by field, not all or nothing: a title fixed on the phone at the garage and a cost
        // typed in on the desktop that evening are both kept, whichever arrives last.
        let uuid = record::uuid_of(&mut tx, Entity::Activity, id).await?;
        if body.date != existing.date && changed_since(&mut tx, &uuid, "date", at).await? {
            body.date = existing.date.clone();
        }
        if body.category != existing.category
            && changed_since(&mut tx, &uuid, "category", at).await?
        {
            body.category = existing.category.clone();
        }
        if body.title != existing.title && changed_since(&mut tx, &uuid, "title", at).await? {
            body.title = existing.title.clone();
        }
        if body.notes != existing.notes && changed_since(&mut tx, &uuid, "notes", at).await? {
            body.notes = existing.notes.clone();
        }
        if body.counter_value != existing.counter_value
            && changed_since(&mut tx, &uuid, "counter_value", at).await?
        {
            body.counter_value = existing.counter_value;
        }
        if body.cost_cents != existing.cost_cents
            && changed_since(&mut tx, &uuid, "cost_cents", at).await?
        {
            body.cost_cents = existing.cost_cents;
        }
        if body.quantity_milli != existing.quantity_milli
            && changed_since(&mut tx, &uuid, "quantity_milli", at).await?
        {
            body.quantity_milli = existing.quantity_milli;
        }
        // `None` is "keep the stored tags", so losing to a newer edit is spelled by dropping them.
        if body
            .tags
            .as_deref()
            .is_some_and(|t| tags::to_json(t) != existing.tags)
            && changed_since(&mut tx, &uuid, "tags", at).await?
        {
            body.tags = None;
        }
        if body.start_counter.flatten() != existing.start_counter
            && changed_since(&mut tx, &uuid, "start_counter", at).await?
        {
            body.start_counter = Some(existing.start_counter);
        }
        if body.from_place.clone().flatten() != existing.from_place
            && changed_since(&mut tx, &uuid, "from_place", at).await?
        {
            body.from_place = Some(existing.from_place.clone());
        }
        if body.to_place.clone().flatten() != existing.to_place
            && changed_since(&mut tx, &uuid, "to_place", at).await?
        {
            body.to_place = Some(existing.to_place.clone());
        }
        if body.duration_minutes.flatten() != existing.duration_minutes
            && changed_since(&mut tx, &uuid, "duration_minutes", at).await?
        {
            body.duration_minutes = Some(existing.duration_minutes);
        }
        if body.battery_used_pct.flatten() != existing.battery_used_pct
            && changed_since(&mut tx, &uuid, "battery_used_pct", at).await?
        {
            body.battery_used_pct = Some(existing.battery_used_pct);
        }
        if body.weight_grams.flatten() != existing.weight_grams
            && changed_since(&mut tx, &uuid, "weight_grams", at).await?
        {
            body.weight_grams = Some(existing.weight_grams);
        }
        if body.fuel_level_pct.flatten() != existing.fuel_level_pct
            && changed_since(&mut tx, &uuid, "fuel_level_pct", at).await?
        {
            body.fuel_level_pct = Some(existing.fuel_level_pct);
        }
        if body.meter_reading_milli.flatten() != existing.meter_reading_milli
            && changed_since(&mut tx, &uuid, "meter_reading_milli", at).await?
        {
            body.meter_reading_milli = Some(existing.meter_reading_milli);
        }
        if body.period_start.clone().flatten() != existing.period_start
            && changed_since(&mut tx, &uuid, "period_start", at).await?
        {
            body.period_start = Some(existing.period_start.clone());
        }
        if body.period_end.clone().flatten() != existing.period_end
            && changed_since(&mut tx, &uuid, "period_end", at).await?
        {
            body.period_end = Some(existing.period_end.clone());
        }
        if body.estimated != Some(existing.estimated)
            && changed_since(&mut tx, &uuid, "estimated", at).await?
        {
            body.estimated = Some(existing.estimated);
        }
        if body.meter_reset != Some(existing.meter_reset)
            && changed_since(&mut tx, &uuid, "meter_reset", at).await?
        {
            body.meter_reset = Some(existing.meter_reset);
        }
        if body.charged_full != Some(existing.charged_full)
            && changed_since(&mut tx, &uuid, "charged_full", at).await?
        {
            body.charged_full = Some(existing.charged_full);
        }
        // Keeping some fields and not others can combine into something no single edit said --
        // a reading without its value, or a trip missing its start -- so the merge is held to
        // the same rules as either edit.
        body.validate(&object)?;
    }

    // Resolved once, after every merge above has had its say, and reused for both the diff and
    // the bind below -- see the doc comment on `ActivityInput::start_counter` for the three
    // states `.flatten()` collapses.
    let start_counter = body.start_counter.flatten();
    let from_place = body.from_place.clone().flatten();
    let to_place = body.to_place.clone().flatten();
    let duration_minutes = body.duration_minutes.flatten();
    let battery_used_pct = body.battery_used_pct.flatten();
    let weight_grams = body.weight_grams.flatten();
    let fuel_level_pct = body.fuel_level_pct.flatten();
    let meter_reading_milli = body.meter_reading_milli.flatten();
    let period_start = body.period_start.clone().flatten();
    let period_end = body.period_end.clone().flatten();
    let estimated = body.estimated.unwrap_or(0);
    let meter_reset = body.meter_reset.unwrap_or(0);
    let charged_full = body.charged_full.unwrap_or(0);

    // Only fields whose value actually differs are logged -- a PATCH that rewrites a field
    // with its existing value produces no `changes` row (see `record::record_update`).
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if body.date != existing.date {
        changed.push(("date", json!(body.date)));
    }
    if body.category != existing.category {
        changed.push(("category", json!(body.category)));
    }
    if body.title != existing.title {
        changed.push(("title", json!(body.title)));
    }
    if body.notes != existing.notes {
        changed.push(("notes", json!(body.notes)));
    }
    if body.counter_value != existing.counter_value {
        changed.push(("counter_value", json!(body.counter_value)));
    }
    if body.cost_cents != existing.cost_cents {
        changed.push(("cost_cents", json!(body.cost_cents)));
    }
    if body.quantity_milli != existing.quantity_milli {
        changed.push(("quantity_milli", json!(body.quantity_milli)));
    }
    // Logged as the JSON text the column holds, as `objects::update` does.
    let tags = body
        .tags
        .as_deref()
        .map(tags::to_json)
        .unwrap_or_else(|| existing.tags.clone());
    if tags != existing.tags {
        changed.push(("tags", json!(tags)));
    }
    if start_counter != existing.start_counter {
        changed.push(("start_counter", json!(start_counter)));
    }
    if from_place != existing.from_place {
        changed.push(("from_place", json!(from_place)));
    }
    if to_place != existing.to_place {
        changed.push(("to_place", json!(to_place)));
    }
    if duration_minutes != existing.duration_minutes {
        changed.push(("duration_minutes", json!(duration_minutes)));
    }
    if battery_used_pct != existing.battery_used_pct {
        changed.push(("battery_used_pct", json!(battery_used_pct)));
    }
    if weight_grams != existing.weight_grams {
        changed.push(("weight_grams", json!(weight_grams)));
    }
    if fuel_level_pct != existing.fuel_level_pct {
        changed.push(("fuel_level_pct", json!(fuel_level_pct)));
    }
    if meter_reading_milli != existing.meter_reading_milli {
        changed.push(("meter_reading_milli", json!(meter_reading_milli)));
    }
    if period_start != existing.period_start {
        changed.push(("period_start", json!(period_start)));
    }
    if period_end != existing.period_end {
        changed.push(("period_end", json!(period_end)));
    }
    if estimated != existing.estimated {
        changed.push(("estimated", json!(estimated)));
    }
    if meter_reset != existing.meter_reset {
        changed.push(("meter_reset", json!(meter_reset)));
    }
    if charged_full != existing.charged_full {
        changed.push(("charged_full", json!(charged_full)));
    }

    sqlx::query(
        "UPDATE activities SET date = $1, category = $2, title = $3, notes = $4, counter_value = $5, cost_cents = $6, quantity_milli = $7, updated_at = $8, tags = $9, \
         start_counter = $10, from_place = $11, to_place = $12, duration_minutes = $13, battery_used_pct = $14, charged_full = $15, weight_grams = $16, fuel_level_pct = $17, \
         meter_reading_milli = $18, period_start = $19, period_end = $20, estimated = $21, meter_reset = $22 WHERE id = $23 AND deleted_at IS NULL",
    )
    .bind(&body.date).bind(&body.category).bind(&body.title).bind(&body.notes)
    .bind(body.counter_value).bind(body.cost_cents).bind(body.quantity_milli).bind(db::now()).bind(&tags)
    .bind(start_counter).bind(&from_place).bind(&to_place).bind(duration_minutes).bind(battery_used_pct)
    .bind(charged_full).bind(weight_grams).bind(fuel_level_pct).bind(meter_reading_milli)
    .bind(&period_start).bind(&period_end).bind(estimated).bind(meter_reset)
    .bind(id)
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
pub(crate) async fn delete(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
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
    let cascaded =
        record::cascade_activity(&mut tx, user.id, &activity_uuid, &now, &edited_at).await?;
    record::record_delete(
        &mut tx,
        user.id,
        Entity::Activity,
        &activity_uuid,
        &edited_at,
    )
    .await?;
    record::log_cascade(&mut tx, user.id, &edited_at, record::DEVICE_ID, &cascaded).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
