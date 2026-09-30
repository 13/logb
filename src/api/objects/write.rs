//! Writing objects: create, update, and the tombstoning delete with its cascade.

use super::*;

pub(super) async fn create(
    user: AuthUser,
    State(state): State<App>,
    Json(mut body): Json<ObjectInput>,
) -> Result<(StatusCode, Json<ObjectOut>), AppError> {
    body.validate()?;
    let client_uuid = super::normalize_client_uuid(body.client_uuid.take())?;
    if let Some(uuid) = client_uuid.as_deref() {
        // Idempotent on the caller's own live row; a conflict on anyone else's or on a
        // tombstone. Checked outside the write transaction on purpose: a hit answers without
        // ever taking the write lock, and a miss that races another replay trips the unique
        // index on `client_uuid` below, which is answered the same way.
        let existing: Option<(i64, i64, Option<String>)> =
            sqlx::query_as("SELECT id, user_id, deleted_at FROM objects WHERE client_uuid = $1")
                .bind(uuid)
                .fetch_optional(&state.db)
                .await?;
        match existing {
            Some((id, owner, None)) if owner == user.id => {
                // By design a replay answers with the row the first attempt created, before the type and parent checks below.
                let row = load_owned_object(&state, user.id, id).await?;
                return Ok((StatusCode::OK, Json(with_stats(&state, &user, row).await?)));
            }
            Some(_) => return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }
    let now = db::now();
    let archived_at = if body.archived == Some(true) {
        Some(now.clone())
    } else {
        None
    };
    let object_uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state).await?;
    check_type(&mut tx, user.id, &body.type_).await?;
    // Checked inside the transaction, on its connection -- never from the pool -- for the two
    // reasons spelled out on `update`: reads and writes are on separate pools now, so a pool
    // connection taken here would not deadlock, but it costs holding the writer connection
    // longer for no reason, and a check taken before the lock can go stale before the write it
    // guards lands. `flatten()` collapses "absent" and an explicit `null` to the same `None`:
    // on create there is no existing value for the two to mean different things about.
    let parent_id = body.parent_id.flatten();
    if let Some(pid) = parent_id {
        if !record::parent_is_valid(&mut tx, user.id, None, pid).await? {
            return Err(AppError::BadRequest(PARENT_REJECTION.into()));
        }
    }
    // Omitted and `null` both mean "no price" on create, same as `parent_id` above.
    let energy_price_milli = body.energy_price_milli.flatten();
    let fuel_capacity_milli = body.fuel_capacity_milli.flatten();
    // The presence of the neutral fields distinguishes a new resource-aware client from a
    // legacy client that only knows `fuel_unit`. Keep legacy vehicle charging on the `fuel`
    // category; silently inferring a resource kind would change its existing UI and sync shape.
    let uses_resource_model = body.resource_unit.is_some()
        || body.resource_kind.is_some()
        || body.measurement_mode.is_some();
    let resource_unit = body
        .resource_unit
        .clone()
        .flatten()
        .or_else(|| body.fuel_unit.clone());
    let resource_kind = body.resource_kind.clone().flatten().or_else(|| {
        uses_resource_model.then(|| match resource_unit.as_deref() {
                Some("kwh") => Some("electricity".into()),
                Some("l" | "gal") if body.type_ == "home" => Some("heating_fuel".into()),
                Some("l" | "gal") => Some("vehicle_fuel".into()),
                _ => None,
            }).flatten()
    });
    let measurement_mode = body
        .measurement_mode
        .clone()
        .flatten()
        .or_else(|| uses_resource_model.then(|| resource_unit.as_ref().map(|_| "usage".into())).flatten());
    if fuel_capacity_milli.is_some() && !matches!(resource_unit.as_deref(), Some("l") | Some("gal"))
    {
        return Err(AppError::BadRequest(
            "fuel_capacity_milli needs a liquid fuel unit".into(),
        ));
    }
    let legacy_fuel_unit = resource_unit
        .as_ref()
        .filter(|u| u.as_str() != "m3")
        .cloned();
    let row = sqlx::query_as::<_, ObjectRow>(
        "INSERT INTO objects (user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
         energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23) \
         RETURNING id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
         purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
         energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private",
    )
    .bind(user.id).bind(&body.name).bind(&body.type_).bind(&body.counter_unit).bind(&legacy_fuel_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(archived_at).bind(parent_id).bind(&now).bind(&now)
    .bind(&object_uuid).bind(tags::to_json(body.tags.as_deref().unwrap_or_default()))
    .bind(energy_price_milli).bind(body.weight_unit.as_deref().unwrap_or("kg")).bind(fuel_capacity_milli)
    .bind(&resource_unit).bind(&resource_kind).bind(&measurement_mode).bind(body.monthly_target_milli.flatten())
    .bind(body.low_level_pct.flatten()).bind(i64::from(body.private.unwrap_or(false)))
    .fetch_one(&mut *tx).await;
    let row = match row {
        Ok(row) => row,
        // Two replays of one client_uuid racing past the pre-check above: the loser trips the
        // unique index on `client_uuid`. The id is spoken for, so that is the same conflict.
        Err(e)
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
        {
            tx.rollback().await?;
            return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into()));
        }
        Err(e) => return Err(e.into()),
    };
    crate::search_text::refresh_objects(&mut tx, crate::search_text::Rows::Id(row.id)).await?;
    record::record_create(&mut tx, user.id, Entity::Object, &object_uuid, &edited_at).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(with_stats(&state, &user, row).await?)))
}

pub(super) async fn update(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
    Json(mut body): Json<ObjectInput>,
) -> Result<Json<ObjectOut>, AppError> {
    // Request-shape validation first, and it is the only thing that happens before the write
    // lock: it reads no database state at all, so a malformed body can be answered 400 without
    // stalling every other writer in the instance for the length of a transaction.
    body.validate()?;

    let mut tx = db::begin_write(&state).await?;

    // EVERY read this handler makes is made here, inside the transaction holding the write
    // lock, and on that transaction's own connection. Both halves of that are load-bearing.
    //
    // Inside, because a PATCH does not write only what the client sent: `archived_at`,
    // `cover_attachment_id` and `parent_id` are each *carried over* from `existing` when the
    // client omits the field, so a value read before the lock was granted is written back
    // afterwards as though the client had asked for it. For `parent_id` that is not merely a
    // lost update, it is a corrupt tree. Start with `A.parent_id = P`, and let three requests
    // overlap (the pool is 4 on SQLite, 16 on PostgreSQL, so they do):
    //
    //   1. `PATCH /objects/A {"name": "X"}` -- no `parent_id` key -- reads `A.parent_id = P`,
    //      then waits for the lock.
    //   2. `PATCH /objects/A {"parent_id": null}` commits. `A` is a root.
    //   3. `PATCH /objects/P {"parent_id": A}` commits: `A`'s ancestors are just `{A}`, so the
    //      cycle check passes honestly.
    //   4. Request 1 wakes and writes the parent it read in step 1.
    //
    // `A.parent_id = P` and `P.parent_id = A`: a cycle that `record::parent_is_valid` was never
    // asked about, because request 1 sent no parent to validate. Nothing in the app can see it
    // (both rows drop out of the root-only list), nothing purges it (the guard in `sync::feed`
    // holds each row back for the other, forever, silently) and `--copy-to` cannot order the
    // pair, so the documented SQLite-to-PostgreSQL migration aborts on the foreign key. The
    // shipped PWA always sends `parent_id`; a hand-written client against the bearer-token API
    // is exactly what omits an optional field.
    //
    // On `tx`'s connection, because reads and writes are on separate pools now: a second pool
    // connection taken here would not fail or hang, but it would read from outside this
    // transaction while holding the writer connection open longer than it needs to be.
    let existing = load_owned_object_on(&mut tx, user.id, id).await?;
    check_type(&mut tx, user.id, &body.type_).await?;
    // `km` and `mi` may always trade places; only a change that leaves that pair (to `h`, or to
    // no counter at all) is checked against the object's trips -- see
    // `COUNTER_UNIT_TRIP_REJECTION`'s doc comment for why one may never exist without the other.
    if !matches!(body.counter_unit.as_deref(), Some("km") | Some("mi")) {
        let has_trip: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM activities WHERE object_id = $1 AND category = 'trip' AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if has_trip.is_some() {
            return Err(AppError::BadRequest(COUNTER_UNIT_TRIP_REJECTION.into()));
        }
    }
    let resource_unit = match body.resource_unit.clone() {
        Some(value) => value,
        // A legacy client has no `resource_unit` key and resends `fuel_unit` in full, including
        // null when clearing it. Honour that shape; new clients always send `resource_unit`.
        None => body.fuel_unit.clone(),
    };
    if resource_unit != existing.resource_unit {
        let has_fuel_history: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM activities WHERE object_id = $1 AND deleted_at IS NULL AND (quantity_milli IS NOT NULL OR fuel_level_pct IS NOT NULL) LIMIT 1",
        )
        .bind(id).fetch_optional(&mut *tx).await?;
        if has_fuel_history.is_some() {
            return Err(AppError::BadRequest(FUEL_UNIT_HISTORY_REJECTION.into()));
        }
    }
    let archived_at = match body.archived {
        Some(true) => existing.archived_at.clone().or_else(|| Some(db::now())),
        Some(false) => None,
        None => existing.archived_at.clone(),
    };
    let cover_attachment_id = match body.cover_attachment_id {
        None => existing.cover_attachment_id,
        Some(None) => None,
        Some(Some(cover)) => {
            let ok: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE id = $1 AND object_id = $2 AND kind = 'photo' AND deleted_at IS NULL")
                .bind(cover).bind(id).fetch_optional(&mut *tx).await?;
            if ok.is_none() {
                return Err(AppError::BadRequest(
                    "cover_attachment_id must be a photo of this object".into(),
                ));
            }
            Some(cover)
        }
    };
    // The cycle check, on the same connection and under the same lock, so a tree that passes it
    // here is still that tree when the `UPDATE` below lands. `sync::apply`'s `Set` path asks
    // `record::parent_is_valid` the identical question, from inside its own `begin_write`.
    let parent_id = match body.parent_id {
        None => existing.parent_id,
        Some(None) => None,
        Some(Some(pid)) => {
            if !record::parent_is_valid(&mut tx, user.id, Some(id), pid).await? {
                return Err(AppError::BadRequest(PARENT_REJECTION.into()));
            }
            Some(pid)
        }
    };
    // A price omitted on PATCH keeps its stored value (three-state, like `cover_attachment_id`
    // above) -- so the other half of `ObjectInput::validate`'s rule, which only ever sees an
    // explicit price in the body, cannot by itself catch a PATCH that clears `fuel_unit` while
    // leaving an already-stored price untouched. Checked here instead, against the row this
    // transaction is holding the write lock on -- `sync::apply`'s `Set` path asks the identical
    // question, from inside its own `begin_write`, for the same reason `parent_id`'s cycle
    // check above does.
    let energy_price_milli = match body.energy_price_milli {
        None => existing.energy_price_milli,
        Some(v) => v,
    };
    if energy_price_milli.is_some() && resource_unit.is_none() {
        return Err(AppError::BadRequest(ENERGY_PRICE_NEEDS_FUEL_UNIT.into()));
    }
    let fuel_capacity_milli = match body.fuel_capacity_milli {
        None => existing.fuel_capacity_milli,
        Some(v) => v,
    };
    if fuel_capacity_milli.is_some() && !matches!(resource_unit.as_deref(), Some("l") | Some("gal"))
    {
        return Err(AppError::BadRequest(
            "fuel_capacity_milli needs a liquid fuel unit".into(),
        ));
    }
    let resource_kind = body
        .resource_kind
        .clone()
        .unwrap_or(existing.resource_kind.clone());
    let measurement_mode = body
        .measurement_mode
        .clone()
        .unwrap_or(existing.measurement_mode.clone());
    let monthly_target_milli = body
        .monthly_target_milli
        .unwrap_or(existing.monthly_target_milli);
    let low_level_pct = body.low_level_pct.unwrap_or(existing.low_level_pct);
    let private = body.private.map(i64::from).unwrap_or(existing.is_private);
    let legacy_fuel_unit = resource_unit
        .as_ref()
        .filter(|u| u.as_str() != "m3")
        .cloned();

    // Only fields whose value actually differs are logged -- see `record::record_update` --
    // so a PATCH that rewrites a field with its existing value produces no `changes` row.
    // Every value is settled before this point, `parent_id` included, so the diff and the
    // single `UPDATE` below always agree about what is being written.
    let mut changed: Vec<(&str, serde_json::Value)> = Vec::new();
    if let Some(unit) = &body.weight_unit {
        if unit != &existing.weight_unit {
            changed.push(("weight_unit", json!(unit)));
        }
    }
    if body.name != existing.name {
        changed.push(("name", json!(body.name)));
    }
    if body.type_ != existing.type_ {
        changed.push(("type", json!(body.type_)));
    }
    if body.counter_unit != existing.counter_unit {
        changed.push(("counter_unit", json!(body.counter_unit)));
    }
    if legacy_fuel_unit != existing.fuel_unit {
        changed.push(("fuel_unit", json!(legacy_fuel_unit)));
    }
    if resource_unit != existing.resource_unit {
        changed.push(("resource_unit", json!(resource_unit)));
    }
    if resource_kind != existing.resource_kind {
        changed.push(("resource_kind", json!(resource_kind)));
    }
    if measurement_mode != existing.measurement_mode {
        changed.push(("measurement_mode", json!(measurement_mode)));
    }
    if monthly_target_milli != existing.monthly_target_milli {
        changed.push(("monthly_target_milli", json!(monthly_target_milli)));
    }
    if low_level_pct != existing.low_level_pct {
        changed.push(("low_level_pct", json!(low_level_pct)));
    }
    if private != existing.is_private {
        changed.push(("private", json!(private)));
    }
    if body.description != existing.description {
        changed.push(("description", json!(body.description)));
    }
    if body.purchase_date != existing.purchase_date {
        changed.push(("purchase_date", json!(body.purchase_date)));
    }
    if body.purchase_price_cents != existing.purchase_price_cents {
        changed.push(("purchase_price_cents", json!(body.purchase_price_cents)));
    }
    if archived_at != existing.archived_at {
        changed.push(("archived_at", json!(archived_at)));
    }
    if cover_attachment_id != existing.cover_attachment_id {
        changed.push(("cover_attachment_id", json!(cover_attachment_id)));
    }
    if parent_id != existing.parent_id {
        changed.push(("parent_id", json!(parent_id)));
    }
    // Absent keeps the stored tags. The change is logged as the JSON text the column holds, the
    // same shape a sync `set` op carries for any other text field.
    let tags = body
        .tags
        .as_deref()
        .map(tags::to_json)
        .unwrap_or_else(|| existing.tags.clone());
    if tags != existing.tags {
        changed.push(("tags", json!(tags)));
    }
    if energy_price_milli != existing.energy_price_milli {
        changed.push(("energy_price_milli", json!(energy_price_milli)));
    }
    if fuel_capacity_milli != existing.fuel_capacity_milli {
        changed.push(("fuel_capacity_milli", json!(fuel_capacity_milli)));
    }

    sqlx::query(
        "UPDATE objects SET name = $1, type = $2, counter_unit = $3, fuel_unit = $4, description = $5, purchase_date = $6, \
         purchase_price_cents = $7, archived_at = $8, cover_attachment_id = $9, parent_id = $10, updated_at = $11, tags = $12, \
         energy_price_milli = $13, weight_unit = $14, fuel_capacity_milli = $15, resource_unit = $16, resource_kind = $17, \
         measurement_mode = $18, monthly_target_milli = $19, low_level_pct = $20, private = $21 \
         WHERE id = $22 AND deleted_at IS NULL",
    )
    .bind(&body.name).bind(&body.type_).bind(&body.counter_unit).bind(&legacy_fuel_unit).bind(&body.description)
    .bind(&body.purchase_date).bind(body.purchase_price_cents).bind(&archived_at)
    .bind(cover_attachment_id).bind(parent_id).bind(db::now()).bind(&tags)
    .bind(energy_price_milli).bind(body.weight_unit.as_deref().unwrap_or(&existing.weight_unit)).bind(fuel_capacity_milli)
    .bind(&resource_unit).bind(&resource_kind).bind(&measurement_mode).bind(monthly_target_milli).bind(low_level_pct).bind(private).bind(id)
    .execute(&mut *tx).await?;
    crate::search_text::refresh_objects(&mut tx, crate::search_text::Rows::Id(id)).await?;
    if !changed.is_empty() {
        let uuid = record::uuid_of(&mut tx, Entity::Object, id).await?;
        record::record_update(
            &mut tx,
            user.id,
            Entity::Object,
            &uuid,
            &changed,
            &record::edited_at_now(),
        )
        .await?;
    }
    tx.commit().await?;

    let row = load_owned_object(&state, user.id, id).await?;
    Ok(Json(with_stats(&state, &user, row).await?))
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
pub(super) async fn delete(
    user: AuthUser,
    State(state): State<App>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let now = db::now();
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state).await?;
    let affected = sqlx::query(
        "UPDATE objects SET deleted_at = $1, updated_at = $2 \
         WHERE id = $3 AND user_id = $4 AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
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
