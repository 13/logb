use super::activities::load_owned_activity;
use super::objects::load_owned_object;
use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::files;
use crate::state::App;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

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
}

pub async fn for_object(state: &App, object_id: i64) -> Result<Vec<AttachmentOut>, AppError> {
    Ok(sqlx::query_as::<_, AttachmentOut>(
        "SELECT a.id, a.object_id, a.activity_id, a.file_id, a.kind, a.caption, a.created_at, \
         f.original_name, f.mime, f.size, f.width, f.height, f.taken_at, a.client_op_id \
         FROM attachments a JOIN files f ON f.id = a.file_id WHERE a.object_id = ? ORDER BY a.created_at DESC, a.id DESC",
    )
    .bind(object_id).fetch_all(&state.db).await?)
}

async fn load_owned(state: &App, user_id: i64, id: i64) -> Result<AttachmentOut, AppError> {
    sqlx::query_as::<_, AttachmentOut>(
        "SELECT a.id, a.object_id, a.activity_id, a.file_id, a.kind, a.caption, a.created_at, \
         f.original_name, f.mime, f.size, f.width, f.height, f.taken_at, a.client_op_id \
         FROM attachments a JOIN files f ON f.id = a.file_id JOIN objects o ON o.id = a.object_id \
         WHERE a.id = ? AND o.user_id = ?",
    )
    .bind(id).bind(user_id)
    .fetch_optional(&state.db).await?
    .ok_or(AppError::NotFound)
}

/// The distinct `file_id`s an object's attachments point at, collected *before* those
/// attachments are deleted so `purge_orphan_files` knows which files to re-check.
pub async fn files_of_object(state: &App, object_id: i64) -> Result<Vec<i64>, AppError> {
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT DISTINCT file_id FROM attachments WHERE object_id = ?")
        .bind(object_id).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// As `files_of_object`, for the attachments hanging off a single activity.
pub async fn files_of_activity(state: &App, activity_id: i64) -> Result<Vec<i64>, AppError> {
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT DISTINCT file_id FROM attachments WHERE activity_id = ?")
        .bind(activity_id).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Delete the `files` rows (and blobs) among `candidates` that no attachment references any
/// more. Callers pass the ids the deletion could plausibly have orphaned; the previous version
/// re-scanned the whole `files` table on every single delete.
pub async fn purge_orphan_files(state: &App, candidates: &[i64]) -> Result<(), AppError> {
    if candidates.is_empty() {
        return Ok(());
    }
    let mut orphans: Vec<(i64, String)> = Vec::new();
    for &file_id in candidates {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, sha256 FROM files WHERE id = ? AND id NOT IN (SELECT file_id FROM attachments)",
        )
        .bind(file_id).fetch_optional(&state.db).await?;
        if let Some(r) = row {
            orphans.push(r);
        }
    }
    for (id, sha) in orphans {
        sqlx::query("DELETE FROM files WHERE id = ?").bind(id).execute(&state.db).await?;
        discard_blob(state, id, &sha).await?;
    }
    Ok(())
}

/// Drops the on-disk artefacts of a `files` row that has already been deleted: the thumbnail
/// always, and the blob only once no other row still points at that content hash (two users
/// uploading the same photo share one blob, and each has their own `files` row).
pub async fn discard_blob(state: &App, file_id: i64, sha: &str) -> Result<(), AppError> {
    let still_used: Option<(i64,)> = sqlx::query_as("SELECT id FROM files WHERE sha256 = ? LIMIT 1")
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
            "client_op_id" => {
                let t = field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?;
                client_op_id = Some(t.trim().to_string());
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

    // A retried upload must resolve to the attachment the first attempt made, rather than
    // hanging a second row off the same file.
    if let Some(op) = client_op_id.as_deref() {
        let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM attachments WHERE client_op_id = ?")
            .bind(op)
            .fetch_optional(&state.db)
            .await?;
        if let Some((id,)) = existing {
            let out = load_owned(&state, user.id, id).await?;
            if out.object_id != object_id {
                return Err(AppError::Conflict("client_op_id already used for another object".into()));
            }
            return Ok((StatusCode::OK, Json(out)));
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

    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM files WHERE user_id = ? AND sha256 = ?")
        .bind(user.id).bind(&sha).fetch_optional(&state.db).await?;
    let file_id = match existing {
        Some((id,)) => id,
        None => {
            state.storage.write_blob(&sha, &bytes).await?;
            // The thumbnail is named after the file id, so it can only be written once the
            // row exists -- but the row must not become visible before the thumbnail does, or
            // a client that sees the new file can ask for a /thumb that is not on disk yet.
            // Writing both inside one transaction closes that window: other connections see
            // the row only at commit, by which point the JPEG is already written.
            let mut tx = state.db.begin().await?;
            let inserted: Result<(i64,), sqlx::Error> = sqlx::query_as(
                "INSERT INTO files (user_id, sha256, original_name, mime, size, width, height, taken_at, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(user.id).bind(&sha).bind(&name).bind(&mime).bind(bytes.len() as i64)
            .bind(image.as_ref().map(|i| i.width as i64)).bind(image.as_ref().map(|i| i.height as i64))
            .bind(image.as_ref().and_then(|i| i.taken_at.clone())).bind(db::now())
            .fetch_one(&mut *tx).await;
            match inserted {
                Ok((id,)) => {
                    if let Some(img) = &image {
                        state.storage.write_thumb(id, &img.thumb_jpeg).await?;
                    }
                    tx.commit().await?;
                    id
                }
                // Two concurrent uploads of identical bytes for the same user: the loser's
                // INSERT trips the UNIQUE(user_id, sha256) constraint. That's a cache hit,
                // not an error -- reuse the row the winner just created.
                Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
                    tx.rollback().await?;
                    let (id,): (i64,) = sqlx::query_as("SELECT id FROM files WHERE user_id = ? AND sha256 = ?")
                        .bind(user.id).bind(&sha).fetch_one(&state.db).await?;
                    id
                }
                Err(e) => return Err(e.into()),
            }
        }
    };

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO attachments (object_id, activity_id, file_id, kind, caption, client_op_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(object_id).bind(activity_id).bind(file_id).bind(&kind).bind(caption.trim())
    .bind(&client_op_id).bind(db::now())
    .fetch_one(&state.db).await?;
    Ok((StatusCode::CREATED, Json(load_owned(&state, user.id, id).await?)))
}

#[derive(Deserialize)]
pub struct UpdateAttachment {
    pub caption: String,
}

async fn update(user: AuthUser, State(state): State<App>, Path(id): Path<i64>, Json(body): Json<UpdateAttachment>) -> Result<Json<AttachmentOut>, AppError> {
    load_owned(&state, user.id, id).await?;
    sqlx::query("UPDATE attachments SET caption = ? WHERE id = ?").bind(body.caption.trim()).bind(id).execute(&state.db).await?;
    Ok(Json(load_owned(&state, user.id, id).await?))
}

async fn delete(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<StatusCode, AppError> {
    let a = load_owned(&state, user.id, id).await?;
    sqlx::query("UPDATE objects SET cover_attachment_id = NULL WHERE id = ? AND cover_attachment_id = ?")
        .bind(a.object_id).bind(id).execute(&state.db).await?;
    sqlx::query("DELETE FROM attachments WHERE id = ?").bind(id).execute(&state.db).await?;
    purge_orphan_files(&state, &[a.file_id]).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct FileRow {
    id: i64,
    sha256: String,
    original_name: String,
    mime: String,
}

async fn load_owned_file(state: &App, user_id: i64, id: i64) -> Result<FileRow, AppError> {
    sqlx::query_as::<_, FileRow>("SELECT id, sha256, original_name, mime FROM files WHERE id = ? AND user_id = ?")
        .bind(id).bind(user_id)
        .fetch_optional(&state.db).await?
        .ok_or(AppError::NotFound)
}

/// User-supplied bytes are served from the same origin as the app, so anything the browser
/// might execute here runs with access to the session. `nosniff` stops it from ignoring the
/// declared type and guessing something scriptable, and the sandbox CSP strips scripts,
/// plugins and same-origin privileges from whatever does get rendered.
fn file_response(bytes: Vec<u8>, mime: &str, disposition: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream"))),
            (header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition).unwrap_or(HeaderValue::from_static("inline"))),
            (header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable")),
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

async fn serve_original(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Response, AppError> {
    let f = load_owned_file(&state, user.id, id).await?;
    let bytes = tokio::fs::read(state.storage.blob_path(&f.sha256)).await.map_err(|_| AppError::NotFound)?;
    let inline = may_render_inline(&f.mime);
    Ok(file_response(bytes, &f.mime, content_disposition(inline, &f.original_name)))
}

async fn serve_thumb(user: AuthUser, State(state): State<App>, Path(id): Path<i64>) -> Result<Response, AppError> {
    let f = load_owned_file(&state, user.id, id).await?;
    let bytes = tokio::fs::read(state.storage.thumb_path(f.id)).await.map_err(|_| AppError::NotFound)?;
    Ok(file_response(bytes, "image/jpeg", "inline".to_string()))
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
