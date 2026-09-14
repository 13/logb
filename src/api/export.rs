use super::attachments::{self, AttachmentOut};
use super::objects::{load_owned_object, ObjectInput, ObjectRow};
use super::activities::{ActivityInput, ActivityRow};
use super::reminders::{select_reminders, ReminderInput, ReminderRow};
use super::settings;
use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::files;
use crate::object_type::{self, Legacy};
use crate::state::App;
use crate::sync::{record, Entity};
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Any;
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

pub fn router(max_import_bytes: usize) -> Router<App> {
    Router::new()
        .route("/export", get(export))
        .route("/import", post(import))
        .layer(DefaultBodyLimit::max(max_import_bytes))
}

#[derive(Serialize, Deserialize)]
struct AttachmentExport {
    sha256: String,
    original_name: String,
    mime: String,
    kind: String,
    caption: String,
    taken_at: Option<String>,
    created_at: String,
}

#[derive(Serialize, Deserialize)]
struct ActivityExport {
    date: String,
    category: String,
    title: String,
    notes: String,
    counter_value: Option<i64>,
    cost_cents: Option<i64>,
    #[serde(default)]
    quantity_milli: Option<i64>,
    created_at: String,
    attachments: Vec<AttachmentExport>,
}

#[derive(Serialize, Deserialize)]
struct ReminderExport {
    title: String,
    notes: String,
    due_date: Option<String>,
    due_counter: Option<i64>,
    repeat_months: Option<i64>,
    repeat_counter: Option<i64>,
    done_at: Option<String>,
    done_activity_index: Option<usize>,
    created_at: String,
    // Added after version 1 archives already existed in the wild; `#[serde(default)]` lets
    // those older archives import as reminders that simply were never snoozed.
    #[serde(default)]
    snoozed_until: Option<String>,
    // Added with reading reminders; an older archive's reminders are all service reminders.
    #[serde(default = "super::reminders::service_kind")]
    kind: String,
    #[serde(default)]
    every_n: Option<i64>,
    #[serde(default)]
    every_unit: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ObjectExport {
    name: String,
    /// Archives written before object types carry `category` instead. Both are optional (and
    /// omitted from an archive this app writes today, via `skip_serializing_if`) so one archive
    /// format does not become two structs; on import, exactly one is expected to be present --
    /// see `resolve_type`, which decides what happens otherwise.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    counter_unit: Option<String>,
    #[serde(default)]
    fuel_unit: Option<String>,
    description: String,
    purchase_date: Option<String>,
    purchase_price_cents: Option<i64>,
    archived_at: Option<String>,
    created_at: String,
    cover_sha256: Option<String>,
    activities: Vec<ActivityExport>,
    attachments: Vec<AttachmentExport>,
    reminders: Vec<ReminderExport>,
}

#[derive(Serialize, Deserialize)]
struct Export {
    version: u32,
    exported_at: String,
    currency: String,
    objects: Vec<ObjectExport>,
}

/// The `files` row an attachment points at always belongs to the same user (attachments hang
/// off that user's objects), so a miss here means the two tables disagree. Report it as an
/// internal error rather than panicking on a bare `HashMap` index.
fn sha_of(sha_by_file: &HashMap<i64, String>, file_id: i64) -> Result<String, AppError> {
    sha_by_file.get(&file_id).cloned()
        .ok_or_else(|| AppError::Internal(format!("attachment references unknown file {file_id}")))
}

fn att_export(a: &AttachmentOut, sha_by_file: &HashMap<i64, String>) -> Result<AttachmentExport, AppError> {
    Ok(AttachmentExport {
        sha256: sha_of(sha_by_file, a.file_id)?,
        original_name: a.original_name.clone(),
        mime: a.mime.clone(),
        kind: a.kind.clone(),
        caption: a.caption.clone(),
        taken_at: a.taken_at.clone(),
        created_at: a.created_at.clone(),
    })
}

#[derive(Deserialize)]
pub struct ExportQuery {
    pub object_id: Option<i64>,
}

/// The app icon, taken from the embedded SPA build rather than a second copy in the tree.
///
/// Absent in a backend-only build (`frontend/dist/` empty, as in a plain `cargo test` before
/// the frontend has ever been built), which is not a reason to fail an export -- the archive is
/// simply without it, the same way a blob missing from disk is skipped below.
fn icon_bytes() -> Option<Vec<u8>> {
    crate::spa::Assets::get("icon.svg").map(|f| f.data.into_owned())
}

async fn export(user: AuthUser, State(state): State<App>, Query(q): Query<ExportQuery>) -> Result<Response, AppError> {
    let objects: Vec<ObjectRow> = match q.object_id {
        Some(id) => vec![load_owned_object(&state, user.id, id).await?],
        None => sqlx::query_as::<_, ObjectRow>(
            "SELECT id, user_id, name, type, counter_unit, fuel_unit, description, purchase_date, \
             purchase_price_cents, archived_at, cover_attachment_id, parent_id, created_at, updated_at, client_uuid, tags \
             FROM objects WHERE user_id = $1 AND deleted_at IS NULL ORDER BY id")
            .bind(user.id).fetch_all(&state.db).await?,
    };
    let sha_rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, sha256 FROM files WHERE user_id = $1")
        .bind(user.id).fetch_all(&state.db).await?;
    let sha_by_file: HashMap<i64, String> = sha_rows.into_iter().collect();

    let mut out = Vec::new();
    let mut blobs: Vec<String> = Vec::new();
    for o in objects {
        let acts = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, client_op_id, created_at, updated_at, client_uuid, tags \
             FROM activities WHERE object_id = $1 AND deleted_at IS NULL ORDER BY date, id")
            .bind(o.id).fetch_all(&state.db).await?;
        let atts = attachments::for_object(&state, o.id).await?;
        let rems = sqlx::query_as::<_, ReminderRow>(sqlx::AssertSqlSafe(select_reminders(
            "WHERE r.object_id = $2 AND r.deleted_at IS NULL AND o.deleted_at IS NULL ORDER BY r.id")))
            .bind(super::reminders::reading_horizon()).bind(o.id).fetch_all(&state.db).await?;
        for a in &atts { blobs.push(sha_of(&sha_by_file, a.file_id)?); }
        let index_of: HashMap<i64, usize> = acts.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        let cover_sha256 = match o.cover_attachment_id.and_then(|cid| atts.iter().find(|a| a.id == cid)) {
            Some(a) => Some(sha_of(&sha_by_file, a.file_id)?),
            None => None,
        };
        out.push(ObjectExport {
            name: o.name, type_: Some(o.type_), category: None, counter_unit: o.counter_unit, fuel_unit: o.fuel_unit, description: o.description,
            purchase_date: o.purchase_date, purchase_price_cents: o.purchase_price_cents,
            archived_at: o.archived_at, created_at: o.created_at, cover_sha256,
            activities: acts.iter().map(|a| Ok(ActivityExport {
                date: a.date.clone(), category: a.category.clone(), title: a.title.clone(), notes: a.notes.clone(),
                counter_value: a.counter_value, cost_cents: a.cost_cents, quantity_milli: a.quantity_milli, created_at: a.created_at.clone(),
                attachments: atts.iter().filter(|x| x.activity_id == Some(a.id)).map(|x| att_export(x, &sha_by_file)).collect::<Result<_, _>>()?,
            })).collect::<Result<Vec<_>, AppError>>()?,
            attachments: atts.iter().filter(|x| x.activity_id.is_none()).map(|x| att_export(x, &sha_by_file)).collect::<Result<_, _>>()?,
            reminders: rems.iter().map(|r| ReminderExport {
                title: r.title.clone(), notes: r.notes.clone(), due_date: r.due_date.clone(), due_counter: r.due_counter,
                repeat_months: r.repeat_months, repeat_counter: r.repeat_counter, done_at: r.done_at.clone(),
                done_activity_index: r.done_activity_id.and_then(|id| index_of.get(&id).copied()),
                created_at: r.created_at.clone(), snoozed_until: r.snoozed_until.clone(),
                kind: r.kind.clone(), every_n: r.every_n, every_unit: r.every_unit.clone(),
            }).collect(),
        });
    }
    let data = Export { version: 1, exported_at: db::now(), currency: settings::currency(&state).await?, objects: out };
    let json = serde_json::to_vec_pretty(&data).map_err(|e| AppError::Internal(e.to_string()))?;

    blobs.sort();
    blobs.dedup();

    // The archive is built into a scratch file and streamed back from it, so peak memory is
    // one blob rather than the whole library: a few gigabytes of photos used to be held once
    // as the read blobs and again as the finished zip before a single byte was sent.
    let scratch = state.storage.scratch_path("export");
    let storage = state.storage.clone();
    let path = scratch.clone();
    let build = tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let file = std::fs::File::create(&path)?;
        let mut w = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        w.start_file("data.json", deflate).map_err(|e| AppError::Internal(e.to_string()))?;
        w.write_all(&json)?;
        if let Some(icon) = icon_bytes() {
            w.start_file("icon.svg", deflate).map_err(|e| AppError::Internal(e.to_string()))?;
            w.write_all(&icon)?;
        }
        for sha in &blobs {
            // A blob missing from disk is storage corruption, not a reason to fail the whole
            // export; the entry is simply absent from the archive, as it was before.
            let Ok(mut src) = std::fs::File::open(storage.blob_path(sha)) else { continue };
            w.start_file(format!("files/{sha}"), stored).map_err(|e| AppError::Internal(e.to_string()))?;
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
            (header::CONTENT_TYPE, HeaderValue::from_static("application/zip")),
            (header::CONTENT_DISPOSITION, HeaderValue::from_str(&name).unwrap()),
            (header::CONTENT_LENGTH, HeaderValue::from_str(&len.to_string()).unwrap()),
        ],
        Body::from_stream(tokio_util::io::ReaderStream::new(file)),
    ).into_response())
}

#[derive(Serialize)]
pub struct ImportCounts {
    pub objects: usize,
    pub activities: usize,
    pub attachments: usize,
    pub reminders: usize,
}

/// Appends `extra` to `description` on its own line, exactly the way the migration's second
/// `CASE` joins an unmapped `category` onto the row's `description`:
///
/// ```sql
/// -- `ws` below is `char(9)||char(10)||char(13)||' '`, the character set the migration names
/// -- so that SQLite's trim strips what Rust's `.trim()` strips.
/// WHEN trim(category, ws) = '' THEN description           -- raw
/// WHEN trim(description, ws) = '' THEN trim(category, ws) -- category trimmed, description dropped
/// ELSE description || char(10) || trim(category, ws)      -- description RAW, category trimmed
/// ```
///
/// `description` is used raw everywhere -- `.trim()` is only ever consulted to test for
/// emptiness, never to change what gets stored -- and `extra` is trimmed before either being
/// used alone or appended. Getting this backwards (trimming `description`) is exactly the bug
/// this helper exists to not repeat: it would silently strip whitespace the migration leaves
/// alone, so a restored backup and a migrated database would disagree about the same row.
fn join_unmapped(description: &str, extra: &str) -> String {
    let extra = extra.trim();
    if extra.is_empty() {
        description.to_string()
    } else if description.trim().is_empty() {
        extra.to_string()
    } else {
        format!("{description}\n{extra}")
    }
}

/// Resolves an imported object's type and description from whichever of `type`/`category` the
/// archive carries, applying the same rule `migrations/sqlite/0009_object_types.sql` applied to
/// existing rows when it did this once for the whole database:
///
/// - A present `type` wins outright, `category` (if also present) is ignored, whether or not
///   `type` is legal. Presence of `type` at all -- even a typo or a future value this build
///   does not know -- means this archive understands the current schema (or is a hand-edit of
///   one written by it), so `category`, the pre-`type` fallback, is consulted only when `type`
///   is missing entirely.
///   - Legal: the type is used as-is, `description` untouched.
///   - Illegal: falls back to `other`, and the string itself is not discarded -- it is joined
///     onto the description by [`join_unmapped`], the same rule an unmapped legacy `category`
///     uses below. An unrecognised descriptor is treated the same whichever field it arrived
///     in.
/// - No `type` at all means a pre-object-types archive; `category` is looked up the same way the
///   migration's first `CASE` did. Text that maps is silently translated (`object_type::LEGACY`
///   agrees with the migration's word list). Text that does not map is appended to the
///   description on its own line via [`join_unmapped`], so nothing the user typed is destroyed
///   by an import they did not know would touch this field -- the same rule the migration's
///   second `CASE` applied.
/// - Neither field present is a corrupt or hand-written archive; `other` with the description
///   untouched, same as a legal `type`.
fn resolve_type(o: &ObjectExport) -> (String, String) {
    match (o.type_.as_deref(), o.category.as_deref()) {
        (Some(t), _) if object_type::is_valid(t) => (t.to_string(), o.description.clone()),
        (Some(t), _) => ("other".to_string(), join_unmapped(&o.description, t)),
        (None, Some(c)) => match object_type::from_legacy(c) {
            Legacy::Mapped(t) => (t.to_string(), o.description.clone()),
            Legacy::Unmapped => ("other".to_string(), join_unmapped(&o.description, c)),
        },
        (None, None) => ("other".to_string(), o.description.clone()),
    }
}

/// Reads one archive entry into `out`, drawing from a decompression budget shared by the whole
/// archive and failing with 413 the moment it would be exceeded.
///
/// The cap is applied to the bytes actually produced, not to the entry's declared uncompressed
/// size: that header is written by whoever built the archive, so a "zip bomb" can advertise a
/// few kilobytes and still inflate to gigabytes. `take(limit + 1)` lets exactly one byte past
/// the budget through, which is enough to detect the overrun without buffering it.
fn read_capped<R: Read>(r: &mut R, out: &mut Vec<u8>, remaining: &mut usize) -> Result<(), AppError> {
    let limit = *remaining;
    let n = r.take(limit as u64 + 1).read_to_end(out)?;
    if n > limit {
        return Err(AppError::TooLarge);
    }
    *remaining -= n;
    Ok(())
}

async fn import(user: AuthUser, State(state): State<App>, body: Bytes) -> Result<Json<ImportCounts>, AppError> {
    let budget = state.config.max_import_inflated_bytes();
    let (data, blobs) = tokio::task::spawn_blocking(move || -> Result<(Export, HashMap<String, Vec<u8>>), AppError> {
        let mut z = zip::ZipArchive::new(Cursor::new(body.to_vec()))
            .map_err(|_| AppError::BadRequest("not a zip archive".into()))?;
        let mut remaining = budget;
        let mut json = Vec::new();
        {
            let mut f = z.by_name("data.json").map_err(|_| AppError::BadRequest("data.json missing".into()))?;
            read_capped(&mut f, &mut json, &mut remaining)?;
        }
        let data: Export = serde_json::from_slice(&json).map_err(|e| AppError::BadRequest(format!("invalid data.json: {e}")))?;
        if data.version != 1 { return Err(AppError::BadRequest(format!("unsupported export version {}", data.version))); }
        let mut blobs = HashMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(|e| AppError::BadRequest(e.to_string()))?;
            let name = f.name().to_string();
            if let Some(sha) = name.strip_prefix("files/") {
                let mut b = Vec::new();
                read_capped(&mut f, &mut b, &mut remaining)?;
                if files::sha256_hex(&b) == sha { blobs.insert(sha.to_string(), b); }
            }
        }
        Ok((data, blobs))
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;

    validate_import(&data)?;

    let mut counts = ImportCounts { objects: 0, activities: 0, attachments: 0, reminders: 0 };
    // One instant for the whole import: every row it creates is "set" at the moment the
    // import ran, not at whatever `created_at` the archive says (that field is preserved on
    // the row itself, per rule 1 in `sync::record` -- this is about `changes`/`field_clock`
    // only).
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    for o in data.objects {
        let now = db::now();
        let object_uuid = uuid::Uuid::new_v4().to_string();
        let (ty, description) = resolve_type(&o);
        let (object_id,): (i64,) = sqlx::query_as(
            "INSERT INTO objects (user_id, name, type, counter_unit, fuel_unit, description, purchase_date, purchase_price_cents, \
             archived_at, cover_attachment_id, created_at, updated_at, client_uuid) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, $11, $12) RETURNING id")
            .bind(user.id).bind(o.name.trim()).bind(&ty).bind(&o.counter_unit).bind(&o.fuel_unit).bind(&description)
            .bind(&o.purchase_date).bind(o.purchase_price_cents).bind(&o.archived_at).bind(&o.created_at).bind(&now)
            .bind(&object_uuid)
            .fetch_one(&mut *tx).await?;
        record::record_create(&mut tx, user.id, Entity::Object, &object_uuid, &edited_at).await?;
        counts.objects += 1;

        let mut activity_ids = Vec::new();
        for a in &o.activities {
            let activity_uuid = uuid::Uuid::new_v4().to_string();
            let (aid,): (i64,) = sqlx::query_as(
                "INSERT INTO activities (object_id, date, category, title, notes, counter_value, cost_cents, quantity_milli, created_at, updated_at, client_uuid) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id")
                .bind(object_id).bind(&a.date).bind(&a.category).bind(a.title.trim()).bind(&a.notes)
                .bind(a.counter_value).bind(a.cost_cents).bind(a.quantity_milli).bind(&a.created_at).bind(&now)
                .bind(&activity_uuid)
                .fetch_one(&mut *tx).await?;
            record::record_create(&mut tx, user.id, Entity::Activity, &activity_uuid, &edited_at).await?;
            activity_ids.push(aid);
            counts.activities += 1;
            for x in &a.attachments {
                if import_attachment(&state, &mut tx, user.id, (object_id, Some(aid)), x, &blobs, &edited_at).await?.is_some() { counts.attachments += 1; }
            }
        }
        let mut cover: Option<i64> = None;
        for x in &o.attachments {
            if let Some(att_id) = import_attachment(&state, &mut tx, user.id, (object_id, None), x, &blobs, &edited_at).await? {
                counts.attachments += 1;
                if o.cover_sha256.as_deref() == Some(x.sha256.as_str()) { cover = Some(att_id); }
            }
        }
        if cover.is_none() {
            if let Some(sha) = &o.cover_sha256 {
                let row: Option<(i64,)> = sqlx::query_as(
                    "SELECT a.id FROM attachments a JOIN files f ON f.id = a.file_id \
                     WHERE a.object_id = $1 AND f.sha256 = $2 AND a.kind = 'photo' AND a.deleted_at IS NULL LIMIT 1")
                    .bind(object_id).bind(sha).fetch_optional(&mut *tx).await?;
                cover = row.map(|r| r.0);
            }
        }
        if let Some(c) = cover {
            sqlx::query("UPDATE objects SET cover_attachment_id = $1 WHERE id = $2 AND deleted_at IS NULL").bind(c).bind(object_id).execute(&mut *tx).await?;
            // A real change from the create above's NULL, so it gets its own `set` -- not
            // folded into `record_create`, which only ever describes the row as it was
            // when it was first written.
            record::record_update(
                &mut tx, user.id, Entity::Object, &object_uuid, &[("cover_attachment_id", json!(c))],
                &edited_at,
            )
            .await?;
        }
        for r in &o.reminders {
            let done_activity_id = r.done_activity_index.and_then(|i| activity_ids.get(i).copied());
            let reminder_uuid = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO reminders (object_id, title, notes, due_date, due_counter, repeat_months, repeat_counter, done_at, done_activity_id, created_at, snoozed_until, client_uuid, kind, every_n, every_unit) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)")
                .bind(object_id).bind(r.title.trim()).bind(&r.notes).bind(&r.due_date).bind(r.due_counter)
                .bind(r.repeat_months).bind(r.repeat_counter).bind(&r.done_at).bind(done_activity_id).bind(&r.created_at)
                .bind(&r.snoozed_until)
                .bind(&reminder_uuid)
                .bind(&r.kind).bind(r.every_n).bind(&r.every_unit)
                .execute(&mut *tx).await?;
            record::record_create(&mut tx, user.id, Entity::Reminder, &reminder_uuid, &edited_at).await?;
            counts.reminders += 1;
        }
    }
    tx.commit().await?;
    Ok(Json(counts))
}

/// Validates every object, activity, attachment and reminder in a parsed archive up
/// front, before the import loop writes a single row, so a bad entry anywhere rejects
/// the whole import with a 400 rather than leaving a partial import behind. Reuses the
/// same validators the normal write paths use (`ObjectInput::validate`,
/// `ActivityInput::validate`, `ReminderInput::validate`), so the rules stay identical to
/// what `POST /objects`, `POST .../activities` and `POST .../reminders` already enforce.
fn validate_import(data: &Export) -> Result<(), AppError> {
    for (oi, o) in data.objects.iter().enumerate() {
        // `resolve_type` is pure, so calling it again at insert time (the loop in `import`)
        // reaches the same answer; computed once here so `obj_input` and `object_stub` below
        // -- both stand-ins for the same object -- can't disagree with each other.
        let (ty, description) = resolve_type(o);
        let mut obj_input = ObjectInput {
            name: o.name.clone(),
            type_: ty.clone(),
            counter_unit: o.counter_unit.clone(),
            fuel_unit: o.fuel_unit.clone(),
            description: description.clone(),
            purchase_date: o.purchase_date.clone(),
            purchase_price_cents: o.purchase_price_cents,
            archived: None,
            cover_attachment_id: None,
            parent_id: None,
            client_uuid: None,
            tags: None,
        };
        obj_input.validate().map_err(|e| tag(e, &format!("object {oi} ({})", o.name)))?;

        // `ActivityInput::validate` only reads `object.counter_unit`; the rest of this
        // stand-in row is never inspected, since the real object doesn't exist yet.
        let object_stub = ObjectRow {
            id: 0, user_id: 0, name: o.name.clone(), type_: ty,
            counter_unit: o.counter_unit.clone(), fuel_unit: o.fuel_unit.clone(), description,
            purchase_date: o.purchase_date.clone(), purchase_price_cents: o.purchase_price_cents,
            archived_at: o.archived_at.clone(), cover_attachment_id: None, parent_id: None,
            created_at: o.created_at.clone(), updated_at: o.created_at.clone(),
            client_uuid: None, tags: "[]".into(),
        };

        for (ai, a) in o.activities.iter().enumerate() {
            let mut act_input = ActivityInput {
                date: a.date.clone(), category: a.category.clone(), title: a.title.clone(),
                notes: a.notes.clone(), counter_value: a.counter_value, cost_cents: a.cost_cents,
                quantity_milli: a.quantity_milli, client_op_id: None, edited_at: None, client_uuid: None,
                tags: None,
            };
            act_input.validate(&object_stub)
                .map_err(|e| tag(e, &format!("object {oi} ({}) activity {ai} ({})", o.name, a.title)))?;
            for x in &a.attachments {
                validate_attachment_kind(&x.kind)
                    .map_err(|e| tag(e, &format!("object {oi} ({}) activity {ai} attachment", o.name)))?;
            }
        }
        for x in &o.attachments {
            validate_attachment_kind(&x.kind).map_err(|e| tag(e, &format!("object {oi} ({}) attachment", o.name)))?;
        }
        for (ri, r) in o.reminders.iter().enumerate() {
            let mut rem_input = ReminderInput {
                title: r.title.clone(), notes: r.notes.clone(), due_date: r.due_date.clone(),
                due_counter: r.due_counter, repeat_months: r.repeat_months, repeat_counter: r.repeat_counter,
                kind: r.kind.clone(), every_n: r.every_n, every_unit: r.every_unit.clone(),
                client_uuid: None,
            };
            // `validate` fills in a missing start for a reading reminder, but the insert below
            // writes the archive's own value -- so the archive has to carry one.
            if r.kind == crate::domain::reminder::KIND_READING && r.due_date.is_none() {
                return Err(tag(
                    AppError::BadRequest("a reading reminder needs a due_date (its start)".into()),
                    &format!("object {oi} ({}) reminder {ri} ({})", o.name, r.title),
                ));
            }
            rem_input.validate(o.counter_unit.as_deref())
                .map_err(|e| tag(e, &format!("object {oi} ({}) reminder {ri} ({})", o.name, r.title)))?;
        }
    }
    Ok(())
}

/// Mirrors the `attachments` table's `CHECK (kind IN ('photo', 'document'))`.
fn validate_attachment_kind(kind: &str) -> Result<(), AppError> {
    if kind != "photo" && kind != "document" {
        return Err(AppError::BadRequest(format!("attachment kind must be photo or document, got '{kind}'")));
    }
    Ok(())
}

/// Prefixes a `BadRequest` message with where in the archive it came from; other error
/// variants pass through unchanged.
fn tag(e: AppError, location: &str) -> AppError {
    match e {
        AppError::BadRequest(msg) => AppError::BadRequest(format!("{location}: {msg}")),
        other => other,
    }
}

/// Returns the new attachment id, or None when the blob is missing from the archive.
///
/// `parent` is `(object_id, activity_id)` -- bundled to keep the argument count under
/// clippy's threshold; the two only ever travel together, from the two call sites in `import`.
async fn import_attachment(
    state: &App, tx: &mut sqlx::Transaction<'_, Any>, user_id: i64, parent: (i64, Option<i64>),
    x: &AttachmentExport, blobs: &HashMap<String, Vec<u8>>, edited_at: &str,
) -> Result<Option<i64>, AppError> {
    let (object_id, activity_id) = parent;
    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM files WHERE user_id = $1 AND sha256 = $2")
        .bind(user_id).bind(&x.sha256).fetch_optional(&mut **tx).await?;
    let file_id = match existing {
        Some((id,)) => id,
        None => {
            let Some(bytes) = blobs.get(&x.sha256) else { return Ok(None) };
            let image = if x.mime.starts_with("image/") {
                let b = bytes.clone();
                tokio::task::spawn_blocking(move || files::process_image(&b)).await.map_err(|e| AppError::Internal(e.to_string()))?
            } else { None };
            state.storage.write_blob(&x.sha256, bytes).await?;
            let file_uuid = uuid::Uuid::new_v4().to_string();
            // The insert rides its own savepoint for the same reason `apply.rs`'s `set` arm
            // does: PostgreSQL aborts the whole transaction on any error, so the re-query below
            // -- which runs on this same transaction -- would itself fail with "current
            // transaction is aborted" the moment it followed a bare, unrescued failed INSERT.
            // SQLite rolls back only the failing statement by default, so the savepoint costs
            // it nothing; on PostgreSQL it is what makes the recovery able to recover at all.
            //
            // This path is also reached only if a concurrent import of the same account
            // manages to race this INSERT -- `db::begin_write`'s advisory lock on PostgreSQL
            // (SQLite's single writer, always) serialises every write transaction, `import`
            // included, so today nothing can. It rides the same rule as `apply.rs` anyway,
            // rather than trusting that the lock is never lifted: `tests/concurrency.rs`
            // documents removing it as a mutation test, and an unrescued arm here would have
            // sprung back to life as a real 500 the moment that lock came off, on the one
            // failure path this function could not otherwise exercise.
            sqlx::query("SAVEPOINT logb_import_file").execute(&mut **tx).await?;
            let inserted: Result<(i64,), sqlx::Error> = sqlx::query_as(
                "INSERT INTO files (user_id, sha256, original_name, mime, size, width, height, taken_at, created_at, client_uuid) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id")
                .bind(user_id).bind(&x.sha256).bind(&x.original_name).bind(&x.mime).bind(bytes.len() as i64)
                .bind(image.as_ref().map(|i| i.width as i64)).bind(image.as_ref().map(|i| i.height as i64))
                .bind(x.taken_at.clone().or_else(|| image.as_ref().and_then(|i| i.taken_at.clone()))).bind(db::now())
                .bind(&file_uuid)
                .fetch_one(&mut **tx).await;
            let id = match inserted {
                Ok((id,)) => {
                    sqlx::query("RELEASE SAVEPOINT logb_import_file").execute(&mut **tx).await?;
                    if let Some(img) = &image { state.storage.write_thumb(id, &img.thumb_jpeg).await?; }
                    record::record_create(tx, user_id, Entity::File, &file_uuid, edited_at).await?;
                    id
                }
                // Two concurrent imports (or an import racing a direct upload) of identical
                // bytes for the same user trip UNIQUE(user_id, sha256). That's a cache hit,
                // not an error -- reuse the row the winner just created, which was (or will
                // be) logged by whichever request actually inserted it.
                Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT logb_import_file").execute(&mut **tx).await?;
                    let (id,): (i64,) = sqlx::query_as("SELECT id FROM files WHERE user_id = $1 AND sha256 = $2")
                        .bind(user_id).bind(&x.sha256).fetch_one(&mut **tx).await?;
                    id
                }
                Err(e) => return Err(e.into()),
            };
            id
        }
    };
    let attachment_uuid = uuid::Uuid::new_v4().to_string();
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO attachments (object_id, activity_id, file_id, kind, caption, created_at, client_uuid) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id")
        .bind(object_id).bind(activity_id).bind(file_id).bind(&x.kind).bind(&x.caption).bind(&x.created_at)
        .bind(&attachment_uuid)
        .fetch_one(&mut **tx).await?;
    record::record_create(tx, user_id, Entity::Attachment, &attachment_uuid, edited_at).await?;
    Ok(Some(id))
}
