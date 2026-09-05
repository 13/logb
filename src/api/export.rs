use super::attachments::{self, AttachmentOut};
use super::objects::{load_owned_object, ObjectInput, ObjectRow};
use super::activities::{ActivityInput, ActivityRow};
use super::reminders::{ReminderInput, ReminderRow};
use super::settings;
use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::files;
use crate::state::App;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::Sqlite;
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
}

#[derive(Serialize, Deserialize)]
struct ObjectExport {
    name: String,
    category: String,
    counter_unit: Option<String>,
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

fn att_export(a: &AttachmentOut, sha_by_file: &HashMap<i64, String>) -> AttachmentExport {
    AttachmentExport {
        sha256: sha_by_file[&a.file_id].clone(),
        original_name: a.original_name.clone(),
        mime: a.mime.clone(),
        kind: a.kind.clone(),
        caption: a.caption.clone(),
        taken_at: a.taken_at.clone(),
        created_at: a.created_at.clone(),
    }
}

#[derive(Deserialize)]
pub struct ExportQuery {
    pub object_id: Option<i64>,
}

async fn export(user: AuthUser, State(state): State<App>, Query(q): Query<ExportQuery>) -> Result<Response, AppError> {
    let objects: Vec<ObjectRow> = match q.object_id {
        Some(id) => vec![load_owned_object(&state, user.id, id).await?],
        None => sqlx::query_as::<_, ObjectRow>(
            "SELECT id, user_id, name, category, counter_unit, description, purchase_date, \
             purchase_price_cents, archived_at, cover_attachment_id, created_at, updated_at \
             FROM objects WHERE user_id = ? ORDER BY id")
            .bind(user.id).fetch_all(&state.db).await?,
    };
    let sha_rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, sha256 FROM files WHERE user_id = ?")
        .bind(user.id).fetch_all(&state.db).await?;
    let sha_by_file: HashMap<i64, String> = sha_rows.into_iter().collect();

    let mut out = Vec::new();
    let mut blobs: Vec<String> = Vec::new();
    for o in objects {
        let acts = sqlx::query_as::<_, ActivityRow>(
            "SELECT id, object_id, date, category, title, notes, counter_value, cost_cents, created_at, updated_at \
             FROM activities WHERE object_id = ? ORDER BY date, id")
            .bind(o.id).fetch_all(&state.db).await?;
        let atts = attachments::for_object(&state, o.id).await?;
        let rems = sqlx::query_as::<_, ReminderRow>(
            "SELECT r.id, r.object_id, r.title, r.notes, r.due_date, r.due_counter, r.repeat_months, \
             r.repeat_counter, r.done_at, r.done_activity_id, r.created_at, o.name AS object_name, o.counter_unit, \
             NULL AS current_counter FROM reminders r JOIN objects o ON o.id = r.object_id WHERE r.object_id = ? ORDER BY r.id")
            .bind(o.id).fetch_all(&state.db).await?;
        for a in &atts { blobs.push(sha_by_file[&a.file_id].clone()); }
        let index_of: HashMap<i64, usize> = acts.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        let cover_sha256 = o.cover_attachment_id
            .and_then(|cid| atts.iter().find(|a| a.id == cid))
            .map(|a| sha_by_file[&a.file_id].clone());
        out.push(ObjectExport {
            name: o.name, category: o.category, counter_unit: o.counter_unit, description: o.description,
            purchase_date: o.purchase_date, purchase_price_cents: o.purchase_price_cents,
            archived_at: o.archived_at, created_at: o.created_at, cover_sha256,
            activities: acts.iter().map(|a| ActivityExport {
                date: a.date.clone(), category: a.category.clone(), title: a.title.clone(), notes: a.notes.clone(),
                counter_value: a.counter_value, cost_cents: a.cost_cents, created_at: a.created_at.clone(),
                attachments: atts.iter().filter(|x| x.activity_id == Some(a.id)).map(|x| att_export(x, &sha_by_file)).collect(),
            }).collect(),
            attachments: atts.iter().filter(|x| x.activity_id.is_none()).map(|x| att_export(x, &sha_by_file)).collect(),
            reminders: rems.iter().map(|r| ReminderExport {
                title: r.title.clone(), notes: r.notes.clone(), due_date: r.due_date.clone(), due_counter: r.due_counter,
                repeat_months: r.repeat_months, repeat_counter: r.repeat_counter, done_at: r.done_at.clone(),
                done_activity_index: r.done_activity_id.and_then(|id| index_of.get(&id).copied()),
                created_at: r.created_at.clone(),
            }).collect(),
        });
    }
    let data = Export { version: 1, exported_at: db::now(), currency: settings::currency(&state).await?, objects: out };
    let json = serde_json::to_vec_pretty(&data).map_err(|e| AppError::Internal(e.to_string()))?;

    blobs.sort();
    blobs.dedup();
    let mut blob_bytes = Vec::with_capacity(blobs.len());
    for sha in &blobs {
        if let Ok(b) = tokio::fs::read(state.storage.blob_path(sha)).await {
            blob_bytes.push((sha.clone(), b));
        }
    }
    let zip_bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, AppError> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut cursor);
            let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            w.start_file("data.json", deflate).map_err(|e| AppError::Internal(e.to_string()))?;
            w.write_all(&json)?;
            for (sha, b) in blob_bytes {
                w.start_file(format!("files/{sha}"), stored).map_err(|e| AppError::Internal(e.to_string()))?;
                w.write_all(&b)?;
            }
            w.finish().map_err(|e| AppError::Internal(e.to_string()))?;
        }
        Ok(cursor.into_inner())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;

    let name = format!("attachment; filename=\"memto-export-{}.zip\"", db::today());
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("application/zip")),
            (header::CONTENT_DISPOSITION, HeaderValue::from_str(&name).unwrap()),
        ],
        zip_bytes,
    ).into_response())
}

#[derive(Serialize)]
pub struct ImportCounts {
    pub objects: usize,
    pub activities: usize,
    pub attachments: usize,
    pub reminders: usize,
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
    let mut tx = state.db.begin().await?;
    for o in data.objects {
        let now = db::now();
        let (object_id,): (i64,) = sqlx::query_as(
            "INSERT INTO objects (user_id, name, category, counter_unit, description, purchase_date, purchase_price_cents, \
             archived_at, cover_attachment_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?) RETURNING id")
            .bind(user.id).bind(o.name.trim()).bind(o.category.trim()).bind(&o.counter_unit).bind(&o.description)
            .bind(&o.purchase_date).bind(o.purchase_price_cents).bind(&o.archived_at).bind(&o.created_at).bind(&now)
            .fetch_one(&mut *tx).await?;
        counts.objects += 1;

        let mut activity_ids = Vec::new();
        for a in &o.activities {
            let (aid,): (i64,) = sqlx::query_as(
                "INSERT INTO activities (object_id, date, category, title, notes, counter_value, cost_cents, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id")
                .bind(object_id).bind(&a.date).bind(&a.category).bind(a.title.trim()).bind(&a.notes)
                .bind(a.counter_value).bind(a.cost_cents).bind(&a.created_at).bind(&now)
                .fetch_one(&mut *tx).await?;
            activity_ids.push(aid);
            counts.activities += 1;
            for x in &a.attachments {
                if import_attachment(&state, &mut tx, user.id, object_id, Some(aid), x, &blobs).await?.is_some() { counts.attachments += 1; }
            }
        }
        let mut cover: Option<i64> = None;
        for x in &o.attachments {
            if let Some(att_id) = import_attachment(&state, &mut tx, user.id, object_id, None, x, &blobs).await? {
                counts.attachments += 1;
                if o.cover_sha256.as_deref() == Some(x.sha256.as_str()) { cover = Some(att_id); }
            }
        }
        if cover.is_none() {
            if let Some(sha) = &o.cover_sha256 {
                let row: Option<(i64,)> = sqlx::query_as(
                    "SELECT a.id FROM attachments a JOIN files f ON f.id = a.file_id WHERE a.object_id = ? AND f.sha256 = ? AND a.kind = 'photo' LIMIT 1")
                    .bind(object_id).bind(sha).fetch_optional(&mut *tx).await?;
                cover = row.map(|r| r.0);
            }
        }
        if let Some(c) = cover {
            sqlx::query("UPDATE objects SET cover_attachment_id = ? WHERE id = ?").bind(c).bind(object_id).execute(&mut *tx).await?;
        }
        for r in &o.reminders {
            let done_activity_id = r.done_activity_index.and_then(|i| activity_ids.get(i).copied());
            sqlx::query(
                "INSERT INTO reminders (object_id, title, notes, due_date, due_counter, repeat_months, repeat_counter, done_at, done_activity_id, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(object_id).bind(r.title.trim()).bind(&r.notes).bind(&r.due_date).bind(r.due_counter)
                .bind(r.repeat_months).bind(r.repeat_counter).bind(&r.done_at).bind(done_activity_id).bind(&r.created_at)
                .execute(&mut *tx).await?;
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
        let mut obj_input = ObjectInput {
            name: o.name.clone(),
            category: o.category.clone(),
            counter_unit: o.counter_unit.clone(),
            description: o.description.clone(),
            purchase_date: o.purchase_date.clone(),
            purchase_price_cents: o.purchase_price_cents,
            archived: None,
            cover_attachment_id: None,
        };
        obj_input.validate().map_err(|e| tag(e, &format!("object {oi} ({})", o.name)))?;

        // `ActivityInput::validate` only reads `object.counter_unit`; the rest of this
        // stand-in row is never inspected, since the real object doesn't exist yet.
        let object_stub = ObjectRow {
            id: 0, user_id: 0, name: o.name.clone(), category: o.category.clone(),
            counter_unit: o.counter_unit.clone(), description: o.description.clone(),
            purchase_date: o.purchase_date.clone(), purchase_price_cents: o.purchase_price_cents,
            archived_at: o.archived_at.clone(), cover_attachment_id: None,
            created_at: o.created_at.clone(), updated_at: o.created_at.clone(),
        };

        for (ai, a) in o.activities.iter().enumerate() {
            let mut act_input = ActivityInput {
                date: a.date.clone(), category: a.category.clone(), title: a.title.clone(),
                notes: a.notes.clone(), counter_value: a.counter_value, cost_cents: a.cost_cents,
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
            };
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
async fn import_attachment(
    state: &App, tx: &mut sqlx::Transaction<'_, Sqlite>, user_id: i64, object_id: i64, activity_id: Option<i64>,
    x: &AttachmentExport, blobs: &HashMap<String, Vec<u8>>,
) -> Result<Option<i64>, AppError> {
    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM files WHERE user_id = ? AND sha256 = ?")
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
            let inserted: Result<(i64,), sqlx::Error> = sqlx::query_as(
                "INSERT INTO files (user_id, sha256, original_name, mime, size, width, height, taken_at, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id")
                .bind(user_id).bind(&x.sha256).bind(&x.original_name).bind(&x.mime).bind(bytes.len() as i64)
                .bind(image.as_ref().map(|i| i.width as i64)).bind(image.as_ref().map(|i| i.height as i64))
                .bind(x.taken_at.clone().or_else(|| image.as_ref().and_then(|i| i.taken_at.clone()))).bind(db::now())
                .fetch_one(&mut **tx).await;
            let id = match inserted {
                Ok((id,)) => {
                    if let Some(img) = &image { state.storage.write_thumb(id, &img.thumb_jpeg).await?; }
                    id
                }
                // Two concurrent imports (or an import racing a direct upload) of identical
                // bytes for the same user trip UNIQUE(user_id, sha256). That's a cache hit,
                // not an error -- reuse the row the winner just created.
                Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
                    let (id,): (i64,) = sqlx::query_as("SELECT id FROM files WHERE user_id = ? AND sha256 = ?")
                        .bind(user_id).bind(&x.sha256).fetch_one(&mut **tx).await?;
                    id
                }
                Err(e) => return Err(e.into()),
            };
            id
        }
    };
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO attachments (object_id, activity_id, file_id, kind, caption, created_at) VALUES (?, ?, ?, ?, ?, ?) RETURNING id")
        .bind(object_id).bind(activity_id).bind(file_id).bind(&x.kind).bind(&x.caption).bind(&x.created_at)
        .fetch_one(&mut **tx).await?;
    Ok(Some(id))
}
