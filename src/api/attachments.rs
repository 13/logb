use super::activities::load_owned_activity;
use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::files;
use crate::state::App;
use crate::sync::{record, Entity};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn router(max_upload_bytes: usize) -> Router<App> {
    Router::new()
        .route("/objects/{id}/attachments", get(list).post(upload))
        .route("/attachments/{id}", axum::routing::patch(update).delete(delete))
        .route("/files/{id}", get(serve_original))
        .route("/files/{id}/thumb", get(serve_thumb))
        .layer(DefaultBodyLimit::max(max_upload_bytes + 1024 * 1024))
}

#[derive(Serialize, sqlx::FromRow, Clone, Debug)]
pub struct AttachmentOut {
    pub id: i64,
    pub object_id: i64,
    pub activity_id: Option<i64>,
    pub file_id: i64,
    pub kind: String,
    pub caption: String,
    pub created_at: String,
    pub original_name: String,
    pub mime: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub taken_at: Option<String>,
    pub client_op_id: Option<String>,
    /// The row's sync identity, so a client can name it in an op without a bootstrap first.
    pub client_uuid: Option<String>,
    /// The sync identity of the `files` row behind this attachment. Identical bytes dedup onto
    /// one file, so an upload may answer with a file uuid the client has never seen.
    pub file_uuid: Option<String>,
}

/// INVARIANT: filters `a.deleted_at` but, unlike `load_owned`, not `o.deleted_at` -- every
/// caller (`list`, `with_attachments`, `export`) already loads the object through a filtered
/// query first, so a tombstoned object never reaches `object_id` here. Adding the join would
/// just repeat a check every caller has already paid for.
pub async fn for_object(state: &App, object_id: i64) -> Result<Vec<AttachmentOut>, AppError> {
    Ok(sqlx::query_as::<_, AttachmentOut>(
        "SELECT a.id, a.object_id, a.activity_id, a.file_id, a.kind, a.caption, a.created_at, \
         f.original_name, f.mime, f.size, f.width, f.height, f.taken_at, a.client_op_id, \
         a.client_uuid, f.client_uuid AS file_uuid \
         FROM attachments a JOIN files f ON f.id = a.file_id WHERE a.object_id = $1 AND a.deleted_at IS NULL \
         ORDER BY a.created_at DESC, a.id DESC",
    )
    .bind(object_id).fetch_all(&state.db).await?)
}

async fn load_owned(state: &App, user_id: i64, id: i64) -> Result<AttachmentOut, AppError> {
    sqlx::query_as::<_, AttachmentOut>(
        "SELECT a.id, a.object_id, a.activity_id, a.file_id, a.kind, a.caption, a.created_at, \
         f.original_name, f.mime, f.size, f.width, f.height, f.taken_at, a.client_op_id, \
         a.client_uuid, f.client_uuid AS file_uuid \
         FROM attachments a JOIN files f ON f.id = a.file_id JOIN objects o ON o.id = a.object_id \
         WHERE a.id = $1 AND o.user_id = $2 AND a.deleted_at IS NULL AND o.deleted_at IS NULL",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

/// Delete the `files` rows (and blobs) among `candidates` that no attachment references any
/// more. Callers pass the ids the deletion could plausibly have orphaned; the previous version
/// re-scanned the whole `files` table on every single delete.
///
/// The reference check counts *every* attachment row, tombstoned ones included, because that
/// is what `attachments.file_id ON DELETE RESTRICT` counts: a tombstoned attachment still
/// pins its file, and deleting the row out from under it would abort with a foreign key
/// error. So this only ever fires for a file no attachment has ever pointed at -- the orphan a
/// lost upload race leaves behind. Files whose attachments were merely tombstoned are freed
/// when the retention purge finally removes those rows.
pub async fn purge_orphan_files(state: &App, candidates: &[i64]) -> Result<(), AppError> {
    if candidates.is_empty() {
        return Ok(());
    }
    let mut orphans: Vec<(i64, String)> = Vec::new();
    for &file_id in candidates {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, sha256 FROM files WHERE id = $1 AND id NOT IN (SELECT file_id FROM attachments)",
        )
        .bind(file_id).fetch_optional(&state.db).await?;
        if let Some(r) = row {
            orphans.push(r);
        }
    }
    for (id, sha) in orphans {
        sqlx::query("DELETE FROM files WHERE id = $1").bind(id).execute(&state.db).await?;
        discard_blob(state, id, &sha).await?;
    }
    Ok(())
}

/// Drops the on-disk artefacts of a `files` row that has already been deleted: the thumbnail
/// always, and the blob only once no other row still points at that content hash (two users
/// uploading the same photo share one blob, and each has their own `files` row).
pub async fn discard_blob(state: &App, file_id: i64, sha: &str) -> Result<(), AppError> {
    let still_used: Option<(i64,)> = sqlx::query_as("SELECT id FROM files WHERE sha256 = $1 LIMIT 1")
        .bind(sha).fetch_optional(&state.db).await?;
    if still_used.is_none() {
        state.storage.remove(sha, file_id).await;
    } else {
        let _ = tokio::fs::remove_file(state.storage.thumb_path(file_id)).await;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub activity_id: Option<i64>,
}

async fn list(user: AuthUser, State(state): State<App>, Path(object_id): Path<i64>, Query(q): Query<ListQuery>) -> Result<Json<Vec<AttachmentOut>>, AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let all = for_object(&state, object_id).await?;
    Ok(Json(match q.activity_id {
        Some(aid) => all.into_iter().filter(|a| a.activity_id == Some(aid)).collect(),
        None => all,
    }))
}

async fn upload(
    user: AuthUser,
    State(state): State<App>,
    Path(object_id): Path<i64>,
    mut mp: Multipart,
) -> Result<(StatusCode, Json<AttachmentOut>), AppError> {
    load_owned_object(&state, user.id, object_id).await?;
    let max = state.config.max_upload_bytes();
    let mut file: Option<(String, Option<String>, Vec<u8>)> = None;
    let mut activity_id: Option<i64> = None;
    let mut kind: Option<String> = None;
    let mut caption = String::new();
    let mut client_op_id: Option<String> = None;
    let mut client_uuid: Option<String> = None;

    loop {
        let field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => return Err(AppError::TooLarge),
            Err(e) => return Err(AppError::BadRequest(e.body_text())),
        };
        match field.name().unwrap_or("") {
            "file" => {
                let name = field.file_name().unwrap_or("upload").to_string();
                let declared = field.content_type().map(|s| s.to_string());
                let bytes = field.bytes().await.map_err(|e| {
                    if e.status() == StatusCode::PAYLOAD_TOO_LARGE { AppError::TooLarge } else { AppError::BadRequest(e.body_text()) }
                })?;
                if bytes.len() > max { return Err(AppError::TooLarge); }
                file = Some((name, declared, bytes.to_vec()));
            }
            "activity_id" => {
                let t = field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?;
                activity_id = Some(t.trim().parse().map_err(|_| AppError::BadRequest("activity_id must be a number".into()))?);
            }
            "kind" => kind = Some(field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?),
            "caption" => caption = field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?,
            "client_uuid" => {
                let t = field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?;
                client_uuid = super::normalize_client_uuid(Some(t))?;
            }
            "client_op_id" => {
                let t = field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?;
                // As in the JSON create path: a blank op id means no idempotency was
                // requested, not a literal id that would collide every naive client's
                // blanks together (see `super::normalize_op_id`).
                client_op_id = super::normalize_op_id(Some(t));
            }
            _ => {}
        }
    }

    let (name, declared, bytes) = file.ok_or_else(|| AppError::BadRequest("file field is required".into()))?;
    if bytes.is_empty() { return Err(AppError::BadRequest("file is empty".into())); }
    let mime = files::resolve_mime(&name, declared.as_deref());
    if !files::is_allowed(&mime, &name) {
        return Err(AppError::BadRequest(format!("file type not allowed: {mime}")));
    }
    if let Some(aid) = activity_id {
        let a = load_owned_activity(&state, user.id, aid).await?;
        if a.object_id != object_id { return Err(AppError::NotFound); }
    }

    // The client's own identity for the attachment, checked the way `objects::create` checks
    // it: a replay of the caller's own live row answers with that row, anything else is a
    // conflict. Like the op-id replay below, this runs after the body has been drained and
    // before the bytes are hashed or written.
    if let Some(uuid) = client_uuid.as_deref() {
        let existing: Option<(i64, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT t.id, t.object_id, o.user_id, t.deleted_at FROM attachments t \
             JOIN objects o ON o.id = t.object_id WHERE t.client_uuid = $1")
            .bind(uuid).fetch_optional(&state.db).await?;
        match existing {
            Some((id, oid, owner, None)) if owner == user.id && oid == object_id => {
                return Ok((StatusCode::OK, Json(load_owned(&state, user.id, id).await?)));
            }
            Some(_) => return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into())),
            None => {}
        }
    }

    // A retried upload must resolve to the attachment the first attempt made, rather than
    // hanging a second row off the same file. This check runs after the multipart loop above
    // has already drained the whole request body -- file bytes included -- so a replay does
    // not skip the upload itself; what it skips is the hash computation and the blob and
    // thumbnail writes below.
    if let Some(op) = client_op_id.as_deref() {
        let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE client_op_id = $1 AND deleted_at IS NULL")
            .bind(op)
            .fetch_optional(&state.db)
            .await?;
        if let Some((id,)) = existing {
            return op_id_attachment_response(&state, user.id, id, object_id).await;
        }
    }

    let sha = files::sha256_hex(&bytes);
    let image = if mime.starts_with("image/") {
        let b = bytes.clone();
        tokio::task::spawn_blocking(move || files::process_image(&b)).await.map_err(|e| AppError::Internal(e.to_string()))?
    } else { None };
    let kind = match kind.as_deref() {
        Some("photo") | Some("document") => kind.unwrap(),
        Some(other) => return Err(AppError::BadRequest(format!("kind must be photo or document, got {other}"))),
        None => if image.is_some() { "photo".to_string() } else { "document".to_string() },
    };

    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM files WHERE user_id = $1 AND sha256 = $2")
        .bind(user.id).bind(&sha).fetch_optional(&state.db).await?;
    let file_id = match existing {
        Some((id,)) => id,
        None => {
            state.storage.write_blob(&sha, &bytes).await?;
            let file_uuid = uuid::Uuid::new_v4().to_string();
            let edited_at = record::edited_at_now();
            // The thumbnail is named after the file id, so it can only be written once the
            // row exists -- but the row must not become visible before the thumbnail does, or
            // a client that sees the new file can ask for a /thumb that is not on disk yet.
            // Writing both inside one transaction closes that window: other connections see
            // the row only at commit, by which point the JPEG is already written.
            let mut tx = db::begin_write(&state.db, state.backend).await?;
            let inserted: Result<(i64,), sqlx::Error> = sqlx::query_as(
                "INSERT INTO files (user_id, sha256, original_name, mime, size, width, height, taken_at, created_at, client_uuid) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
            )
            .bind(user.id).bind(&sha).bind(&name).bind(&mime).bind(bytes.len() as i64)
            .bind(image.as_ref().map(|i| i.width as i64)).bind(image.as_ref().map(|i| i.height as i64))
            .bind(image.as_ref().and_then(|i| i.taken_at.clone())).bind(db::now())
            .bind(&file_uuid)
            .fetch_one(&mut *tx).await;
            match inserted {
                Ok((id,)) => {
                    if let Some(img) = &image {
                        state.storage.write_thumb(id, &img.thumb_jpeg).await?;
                    }
                    record::record_create(&mut tx, user.id, Entity::File, &file_uuid, &edited_at).await?;
                    tx.commit().await?;
                    id
                }
                // Two concurrent uploads of identical bytes for the same user: the loser's
                // INSERT trips the UNIQUE(user_id, sha256) constraint. That's a cache hit,
                // not an error -- reuse the row the winner just created.
                Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
                    tx.rollback().await?;
                    let (id,): (i64,) = sqlx::query_as("SELECT id FROM files WHERE user_id = $1 AND sha256 = $2")
                        .bind(user.id).bind(&sha).fetch_one(&state.db).await?;
                    id
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    let attachment_uuid = client_uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let inserted: Result<(i64,), sqlx::Error> = sqlx::query_as(
        "INSERT INTO attachments (object_id, activity_id, file_id, kind, caption, client_op_id, created_at, client_uuid) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(object_id).bind(activity_id).bind(file_id).bind(&kind).bind(caption.trim())
    .bind(&client_op_id).bind(db::now())
    .bind(&attachment_uuid)
    .fetch_one(&mut *tx).await;
    let id = match inserted {
        Ok((id,)) => {
            record::record_create(&mut tx, user.id, Entity::Attachment, &attachment_uuid, &edited_at).await?;
            tx.commit().await?;
            id
        }
        // Two concurrent uploads carrying the same client_op_id (several tabs sharing one
        // offline outbox, flushing on reconnect): the loser's INSERT trips the partial unique
        // index on attachments -- the same shape of race as the files-table dedup above.
        // Adopt the winner's row instead of failing the request. The winner may belong to a
        // different object than this upload targeted, so apply the same object check the
        // pre-check above does, rather than handing back another object's attachment.
        Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
            tx.rollback().await?;
            // Without an op id the only unique index this insert can trip is `client_uuid`:
            // a replay raced past the pre-check above. This request's bytes are then
            // referenced by nothing, as in the op-id case below.
            let Some(op) = client_op_id.as_deref() else {
                purge_orphan_files(&state, &[file_id]).await?;
                return Err(AppError::Conflict(super::CLIENT_UUID_TAKEN.into()));
            };
            let winner: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE client_op_id = $1 AND deleted_at IS NULL")
                .bind(op)
                .fetch_optional(&state.db).await?;
            // As on the activity path: the row holding this op id may be a tombstone, which
            // the filter hides. The id is still taken, so that is a conflict, not a 500.
            let Some((winner_id,)) = winner else {
                purge_orphan_files(&state, &[file_id]).await?;
                return Err(super::op_id_conflict());
            };
            // This request's own bytes are now referenced by nothing: the winner's attachment
            // points at the winner's file. Identical bytes dedup onto that same row, so the
            // orphan only exists when one op id was reused with DIFFERENT bytes -- a violation
            // of the op-id contract, but one that otherwise leaks a `files` row and a blob per
            // loser, permanently and invisibly. `purge_orphan_files` re-checks the reference
            // before deleting, so the dedup case (file_id shared with the winner) is a no-op.
            purge_orphan_files(&state, &[file_id]).await?;
            return op_id_attachment_response(&state, user.id, winner_id, object_id).await;
        }
        Err(e) => return Err(e.into()),
    };
    Ok((StatusCode::CREATED, Json(load_owned(&state, user.id, id).await?)))
}

/// The attachment a client_op_id lookup found -- whether from the pre-check or after losing
/// an insert race -- may belong to a different object than the one being uploaded to; that's
/// a 409, not this object's attachment.
async fn op_id_attachment_response(
    state: &App,
    user_id: i64,
    id: i64,
    object_id: i64,
) -> Result<(StatusCode, Json<AttachmentOut>), AppError> {
    let out = load_owned(state, user_id, id).await?;
    if out.object_id != object_id {
        return Err(super::op_id_conflict());
    }
    Ok((StatusCode::OK, Json(out)))
}

#[derive(Deserialize)]
pub struct UpdateAttachment {
    pub caption: String,
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(body): Json<UpdateAttachment>) -> Result<Json<AttachmentOut>, AppError> {
    let existing = load_owned(&state, user.id, id).await?;
    let caption = body.caption.trim();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    sqlx::query("UPDATE attachments SET caption = $1 WHERE id = $2 AND deleted_at IS NULL").bind(caption).bind(id).execute(&mut *tx).await?;
    if existing.caption != caption {
        let uuid = record::uuid_of(&mut tx, Entity::Attachment, id).await?;
        record::record_update(
            &mut tx, user.id, Entity::Attachment, &uuid, &[("caption", json!(caption))],
            &record::edited_at_now(),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load_owned(&state, user.id, id).await?))
}

/// Tombstoned rather than removed, so an offline client learns the attachment is gone. The
/// `files` row and its blob stay: `attachments.file_id` is `ON DELETE RESTRICT` and this row
/// still references it, so the content is only reclaimed once the retention purge drops the
/// tombstone. `load_owned_file` is what stops the file being served in the meantime.
async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    load_owned(&state, user.id, id).await?;
    let now = db::now();
    let edited_at = record::edited_at_now();
    let mut tx = db::begin_write(&state.db, state.backend).await?;
    let affected = sqlx::query("UPDATE attachments SET deleted_at = $1 WHERE id = $2 AND deleted_at IS NULL")
        .bind(&now).bind(id).execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    // `record::clear_cover_of` is the single copy of this statement, shared with
    // `sync::apply::apply_op`'s `delete` handling.
    let attachment_uuid = record::uuid_of(&mut tx, Entity::Attachment, id).await?;
    record::clear_cover_of(&mut tx, user.id, &attachment_uuid, &edited_at).await?;
    record::record_delete(&mut tx, user.id, Entity::Attachment, &attachment_uuid, &edited_at).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct FileRow {
    id: i64,
    sha256: String,
    original_name: String,
    mime: String,
}

/// A `files` row is reachable only through an attachment, so it stops being readable the
/// moment every attachment pointing at it is tombstoned -- which is how deleting the last
/// attachment of a photo, or the activity or object it hung off, still makes `/files/{id}`
/// read as absent even though the row and blob survive for the sync window.
async fn load_owned_file(state: &App, user_id: i64, id: i64) -> Result<FileRow, AppError> {
    sqlx::query_as::<_, FileRow>(
        "SELECT f.id, f.sha256, f.original_name, f.mime FROM files f \
         WHERE f.id = $1 AND f.user_id = $2 AND EXISTS ( \
           SELECT 1 FROM attachments a JOIN objects o ON o.id = a.object_id \
           WHERE a.file_id = f.id AND a.deleted_at IS NULL AND o.deleted_at IS NULL)",
    )
        .bind(id).bind(user_id)
        .fetch_optional(&state.db).await?
        .ok_or(AppError::NotFound)
}

/// User-supplied bytes are served from the same origin as the app, so anything the browser
/// might execute here runs with access to the session. `nosniff` stops it from ignoring the
/// declared type and guessing something scriptable, and the sandbox CSP strips scripts,
/// plugins and same-origin privileges from whatever does get rendered.
///
/// `private, no-cache` rather than a long `immutable` lifetime: file ids are sequential, and a
/// response the browser may reuse without asking would hand the next person signed in on the
/// same browser the previous person's bytes for `/api/files/N` without `load_owned_file` ever
/// running. With `no-cache` every reuse is revalidated, and the 304 that makes that cheap is
/// only ever answered after the ownership check (see `not_modified`).
fn file_response(bytes: Vec<u8>, mime: &str, disposition: String, etag: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream"))),
            (header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition).unwrap_or(HeaderValue::from_static("inline"))),
            (header::CACHE_CONTROL, HeaderValue::from_static(FILE_CACHE_CONTROL)),
            (header::ETAG, HeaderValue::from_str(etag).unwrap_or(HeaderValue::from_static("\"\""))),
            (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
            (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'; sandbox")),
        ],
        Body::from(bytes),
    ).into_response()
}

/// Types a browser may render in place. Everything else downloads.
///
/// An allow-list, not `image/*`: SVG is an image by MIME type and a scriptable document in
/// practice, so serving one inline from this origin -- which the documents tab links straight
/// to -- would let an uploaded file run script against the uploader's own session. PDFs stay
/// inline because browsers render them in a sandboxed viewer, and the CSP above holds anyway.
fn may_render_inline(mime: &str) -> bool {
    matches!(mime, "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/avif" | "image/bmp" | "application/pdf")
}

/// Percent-encodes `name` for the `filename*` parameter of RFC 6266 / RFC 5987. Everything
/// outside that grammar's `attr-char` set is escaped, so the result is always plain ASCII.
fn encode_ext_value(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A `Content-Disposition` value that is always a valid header: the plain `filename` is
/// reduced to printable ASCII for old clients, and the real name -- accents, CJK, emoji --
/// rides along UTF-8-encoded in `filename*`, which every current browser prefers.
///
/// Building it this way matters because a header value that fails to parse used to fall back
/// to a bare `inline`, silently turning an intended download into an in-page render.
fn content_disposition(inline: bool, original_name: &str) -> String {
    let kind = if inline { "inline" } else { "attachment" };
    let ascii: String = original_name
        .chars()
        .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '_' })
        .map(|c| if matches!(c, '"' | '\\') { '_' } else { c })
        .collect();
    let ascii = if ascii.trim().is_empty() { "download".to_string() } else { ascii };
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8\'\'{}", encode_ext_value(original_name))
}

const FILE_CACHE_CONTROL: &str = "private, no-cache";

/// A strong validator for one stored file: its content hash, which already names the blob. The
/// thumbnail is different bytes under the same hash, so it carries its own suffix.
fn file_etag(sha256: &str, thumb: bool) -> String {
    if thumb { format!("\"{sha256}-thumb\"") } else { format!("\"{sha256}\"") }
}

/// Whether `If-None-Match` names `etag` (or is `*`). Weak comparison, as RFC 9110 prescribes for
/// this header, so a `W/` prefix a proxy added still matches.
fn if_none_match_hits(headers: &HeaderMap, etag: &str) -> bool {
    headers.get_all(header::IF_NONE_MATCH).iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag)
}

/// The 304 for a revalidation. Only ever built after `load_owned_file` has passed: a 304 tells
/// the browser to reuse what it holds, so answering one before the ownership check would let
/// another user's cached bytes through exactly as the old `immutable` lifetime did.
fn not_modified(etag: &str) -> Response {
    (
        StatusCode::NOT_MODIFIED,
        [
            (header::CACHE_CONTROL, HeaderValue::from_static(FILE_CACHE_CONTROL)),
            (header::ETAG, HeaderValue::from_str(etag).unwrap_or(HeaderValue::from_static("\"\""))),
        ],
    ).into_response()
}

async fn serve_original(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, headers: HeaderMap) -> Result<Response, AppError> {
    let f = load_owned_file(&state, user.id, id).await?;
    let etag = file_etag(&f.sha256, false);
    if if_none_match_hits(&headers, &etag) { return Ok(not_modified(&etag)); }
    let bytes = tokio::fs::read(state.storage.blob_path(&f.sha256)).await.map_err(|_| AppError::NotFound)?;
    let inline = may_render_inline(&f.mime);
    Ok(file_response(bytes, &f.mime, content_disposition(inline, &f.original_name), &etag))
}

async fn serve_thumb(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, headers: HeaderMap) -> Result<Response, AppError> {
    let f = load_owned_file(&state, user.id, id).await?;
    let etag = file_etag(&f.sha256, true);
    // A thumbnail only exists for images; the 304 must not claim one that was never made.
    let path = state.storage.thumb_path(f.id);
    if if_none_match_hits(&headers, &etag) && tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(not_modified(&etag));
    }
    let bytes = tokio::fs::read(path).await.map_err(|_| AppError::NotFound)?;
    Ok(file_response(bytes, "image/jpeg", "inline".to_string(), &etag))
}

#[cfg(test)]
mod tests {
    use super::content_disposition;

    #[test]
    fn ascii_names_pass_through_in_both_parameters() {
        let cd = content_disposition(false, "invoice.pdf");
        assert_eq!(cd, "attachment; filename=\"invoice.pdf\"; filename*=UTF-8''invoice.pdf");
    }

    #[test]
    fn non_ascii_names_survive_in_the_extended_parameter() {
        let cd = content_disposition(true, "Anhängerkupplung.jpg");
        assert!(cd.starts_with("inline; filename=\"Anh_ngerkupplung.jpg\""), "{cd}");
        assert!(cd.ends_with("filename*=UTF-8''Anh%C3%A4ngerkupplung.jpg"), "{cd}");
        assert!(cd.is_ascii(), "the header value must be sendable as-is: {cd}");
    }

    #[test]
    fn quotes_and_control_characters_cannot_break_out_of_the_quoted_string() {
        let cd = content_disposition(false, "a\"; rm -rf /\r\n.txt");
        assert!(!cd["attachment; filename=\"".len()..].starts_with('"'));
        assert_eq!(cd.matches('"').count(), 2, "{cd}");
        assert!(!cd.contains('\r') && !cd.contains('\n'), "{cd}");
    }

    #[test]
    fn a_name_with_nothing_printable_still_yields_a_filename() {
        let cd = content_disposition(false, "  ");
        assert!(cd.contains("filename=\"download\""), "{cd}");
    }
}
