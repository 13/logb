//! Writing an archive: every object a person owns, with its activities and attachments.

use crate::api::activities::ActivityRow;
use crate::api::attachments::AttachmentOut;
use crate::api::objects::{load_owned_object, ObjectRow};
use crate::api::reminders::{select_reminders, ReminderRow};
use crate::api::settings;
use crate::auth::AuthUser;
use crate::db;
use crate::domain::custom_type::CUSTOM_PREFIX;
use crate::domain::tags;
use crate::error::AppError;
use crate::state::App;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::io::Write;

use super::*;

pub(super) async fn export(
    user: AuthUser,
    State(state): State<App>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let mut objects: Vec<ObjectRow> = match q.object_id {
        Some(id) => vec![load_owned_object(&state, user.id, id).await?],
        None => sqlx::query_as::<_, ObjectRow>(
            "SELECT id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
             purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags, \
             energy_price_milli, weight_unit, fuel_capacity_milli, resource_unit, resource_kind, measurement_mode, monthly_target_milli, low_level_pct, private \
             FROM objects WHERE user_id = $1 AND deleted_at IS NULL ORDER BY id")
            .bind(user.id).fetch_all(&state.db).await?,
    };
    if q.exclude_body {
        objects.retain(|o| o.type_ != "body");
    }
    if q.object_id.is_none() {
        objects.retain(|o| o.is_private == 0);
    }
    let sha_rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, sha256 FROM files WHERE user_id = $1")
            .bind(user.id)
            .fetch_all(&state.db)
            .await?;
    let sha_by_file: HashMap<i64, String> = sha_rows.into_iter().collect();

    // Three statements for the whole export -- activities, attachments, reminders -- grouped by
    // object here, where there used to be three per object. The order within each object is
    // the one the per-object statements had: activities by date then id, attachments newest
    // first, reminders by id.
    let (scope, scope_id) = match q.object_id {
        Some(id) => ("o.id = $1", id),
        None => ("o.user_id = $1", user.id),
    };
    let mut acts_by_object: HashMap<i64, Vec<ActivityRow>> = HashMap::new();
    for a in sqlx::query_as::<_, ActivityRow>(sqlx::AssertSqlSafe(format!(
        "SELECT a.id, a.object_id, a.date, a.category, a.title, a.notes, a.counter_value, a.cost_cents, a.quantity_milli, a.client_op_id, a.created_at, a.updated_at, a.client_uuid, a.tags, \
         a.start_counter, a.from_place, a.to_place, a.duration_minutes, a.battery_used_pct, a.charged_full, a.weight_grams, a.fuel_level_pct, a.meter_reading_milli, a.period_start, a.period_end, a.estimated, a.meter_reset \
         FROM activities a JOIN objects o ON o.id = a.object_id \
         WHERE {scope} AND o.deleted_at IS NULL AND a.deleted_at IS NULL ORDER BY a.object_id, a.date, a.id")))
        .bind(scope_id).fetch_all(&state.db).await?
    {
        acts_by_object.entry(a.object_id).or_default().push(a);
    }
    let mut atts_by_object: HashMap<i64, Vec<AttachmentOut>> = HashMap::new();
    for a in sqlx::query_as::<_, AttachmentOut>(sqlx::AssertSqlSafe(format!(
        "SELECT a.id, a.object_id, a.activity_id, a.file_id, a.kind, a.caption, a.created_at, \
         f.original_name, f.mime, f.size, f.width, f.height, f.taken_at, a.client_op_id, \
         a.client_uuid, f.client_uuid AS file_uuid \
         FROM attachments a JOIN files f ON f.id = a.file_id JOIN objects o ON o.id = a.object_id \
         WHERE {scope} AND o.deleted_at IS NULL AND a.deleted_at IS NULL \
         ORDER BY a.created_at DESC, a.id DESC")))
        .bind(scope_id).fetch_all(&state.db).await?
    {
        atts_by_object.entry(a.object_id).or_default().push(a);
    }
    let mut rems_by_object: HashMap<i64, Vec<ReminderRow>> = HashMap::new();
    for r in sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(select_reminders(&format!(
        "WHERE {} AND r.deleted_at IS NULL AND o.deleted_at IS NULL ORDER BY r.id",
        scope.replace("$1", "$2")))))
        .bind(crate::api::reminders::reading_horizon(user.today())).bind(scope_id).fetch_all(&state.db).await?
    {
        rems_by_object.entry(r.object_id).or_default().push(r);
    }

    let mut out = Vec::new();
    let mut blobs: Vec<String> = Vec::new();
    for o in objects {
        let acts = acts_by_object.remove(&o.id).unwrap_or_default();
        let atts = atts_by_object.remove(&o.id).unwrap_or_default();
        let rems = rems_by_object.remove(&o.id).unwrap_or_default();
        for a in &atts {
            blobs.push(sha_of(&sha_by_file, a.file_id)?);
        }
        let index_of: HashMap<i64, usize> =
            acts.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        let cover_sha256 = match o
            .cover_attachment_id
            .and_then(|cid| atts.iter().find(|a| a.id == cid))
        {
            Some(a) => Some(sha_of(&sha_by_file, a.file_id)?),
            None => None,
        };
        out.push(ObjectExport {
            tags: tags::from_json(&o.tags),
            name: o.name,
            type_: Some(o.type_),
            category: None,
            counter_unit: o.counter_unit,
            fuel_unit: o.fuel_unit,
            description: o.description,
            purchase_date: o.purchase_date,
            purchase_price_cents: o.purchase_price_cents,
            archived_at: o.archived_at,
            created_at: o.created_at,
            cover_sha256,
            energy_price_milli: o.energy_price_milli,
            weight_unit: o.weight_unit.clone(),
            fuel_capacity_milli: o.fuel_capacity_milli,
            resource_unit: o.resource_unit.clone(),
            resource_kind: o.resource_kind.clone(),
            measurement_mode: o.measurement_mode.clone(),
            monthly_target_milli: o.monthly_target_milli,
            low_level_pct: o.low_level_pct,
            private: o.is_private != 0,
            activities: acts
                .iter()
                .map(|a| {
                    Ok(ActivityExport {
                        date: a.date.clone(),
                        category: a.category.clone(),
                        title: a.title.clone(),
                        notes: a.notes.clone(),
                        counter_value: a.counter_value,
                        cost_cents: a.cost_cents,
                        quantity_milli: a.quantity_milli,
                        created_at: a.created_at.clone(),
                        tags: tags::from_json(&a.tags),
                        start_counter: a.start_counter,
                        from_place: a.from_place.clone(),
                        to_place: a.to_place.clone(),
                        duration_minutes: a.duration_minutes,
                        battery_used_pct: a.battery_used_pct,
                        charged_full: a.charged_full,
                        weight_grams: a.weight_grams,
                        fuel_level_pct: a.fuel_level_pct,
                        meter_reading_milli: a.meter_reading_milli,
                        period_start: a.period_start.clone(),
                        period_end: a.period_end.clone(),
                        estimated: a.estimated,
                        meter_reset: a.meter_reset,
                        attachments: atts
                            .iter()
                            .filter(|x| x.activity_id == Some(a.id))
                            .map(|x| att_export(x, &sha_by_file))
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?,
            attachments: atts
                .iter()
                .filter(|x| x.activity_id.is_none())
                .map(|x| att_export(x, &sha_by_file))
                .collect::<Result<_, _>>()?,
            reminders: rems
                .iter()
                .map(|r| ReminderExport {
                    title: r.title.clone(),
                    notes: r.notes.clone(),
                    due_date: r.due_date.clone(),
                    due_counter: r.due_counter,
                    repeat_months: r.repeat_months,
                    repeat_counter: r.repeat_counter,
                    done_at: r.done_at.clone(),
                    done_activity_index: r
                        .done_activity_id
                        .and_then(|id| index_of.get(&id).copied()),
                    created_at: r.created_at.clone(),
                    snoozed_until: r.snoozed_until.clone(),
                    kind: r.kind.clone(),
                    every_n: r.every_n,
                    every_unit: r.every_unit.clone(),
                    schedule: r.schedule.clone(),
                })
                .collect(),
        });
    }
    let type_rows: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT client_uuid, name, icon, categories, counter_unit FROM object_types \
         WHERE user_id = $1 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    // A one-object export carries only the type that object uses, so importing it elsewhere does
    // not bring along every type the account has.
    let types = type_rows
        .into_iter()
        .filter(|(uuid, ..)| {
            q.object_id.is_none()
                || out
                    .iter()
                    .any(|o| o.type_.as_deref() == Some(&format!("{CUSTOM_PREFIX}{uuid}")))
        })
        .map(
            |(client_uuid, name, icon, categories, counter_unit)| TypeExport {
                client_uuid,
                name,
                icon,
                categories: serde_json::from_str(&categories).unwrap_or_default(),
                counter_unit,
            },
        )
        .collect();
    let data = Export {
        version: 1,
        exported_at: db::now(),
        currency: settings::currency(&state).await?,
        types,
        objects: out,
    };
    blobs.sort();
    blobs.dedup();

    // The archive is built into a scratch file and streamed back from it, so peak memory is
    // one blob rather than the whole library: a few gigabytes of photos used to be held once
    // as the read blobs and again as the finished zip before a single byte was sent.
    let scratch = state.storage.scratch_path("export");
    let storage = state.storage.clone();
    let path = scratch.clone();
    let build = tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        // Serialised here, off the async runtime, and compactly: a library's worth of JSON is
        // megabytes, and the pretty form was a third larger for nobody -- the file is read by
        // `import`, which parses either form. It is compressed in the archive anyway.
        let json = serde_json::to_vec(&data).map_err(|e| AppError::Internal(e.to_string()))?;
        let file = std::fs::File::create(&path)?;
        let mut w = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let deflate = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        w.start_file("data.json", deflate)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        w.write_all(&json)?;
        if let Some(icon) = icon_bytes() {
            w.start_file("icon.svg", deflate)
                .map_err(|e| AppError::Internal(e.to_string()))?;
            w.write_all(&icon)?;
        }
        for sha in &blobs {
            // A blob missing from disk is storage corruption, not a reason to fail the whole
            // export; the entry is simply absent from the archive, as it was before.
            let Ok(mut src) = std::fs::File::open(storage.blob_path(sha)) else {
                continue;
            };
            w.start_file(format!("files/{sha}"), stored)
                .map_err(|e| AppError::Internal(e.to_string()))?;
            std::io::copy(&mut src, &mut w)?;
        }
        w.finish().map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    if let Err(e) = build {
        let _ = tokio::fs::remove_file(&scratch).await;
        return Err(e);
    }

    let file = tokio::fs::File::open(&scratch).await?;
    let len = file.metadata().await?.len();
    // Unlink now: the open handle keeps the data readable for as long as this response takes,
    // and the file cannot outlive the request even if the client disconnects mid-download.
    let _ = tokio::fs::remove_file(&scratch).await;

    let name = format!("attachment; filename=\"logb-export-{}.zip\"", db::today());
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/zip"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&name).unwrap(),
            ),
            (
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&len.to_string()).unwrap(),
            ),
        ],
        Body::from_stream(tokio_util::io::ReaderStream::new(file)),
    )
        .into_response())
}

