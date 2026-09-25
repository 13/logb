//! Keeping `files/` and `thumbs/` in step with the `files` table: the one-time move of
//! thumbnails from id names to content names, and the daily sweep of what no row references.

use crate::error::AppError;
use crate::state::App;

/// What `migrate_legacy_thumbs` did, for the log line at startup and for tests.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LegacyThumbs {
    /// Moved to (or already present at) their content-named path.
    pub moved: usize,
    /// Named after an id no image row has, so nothing could ever have served them rightly.
    pub deleted: usize,
}

/// Moves every thumbnail still named `thumbs/<file_id>.jpg` -- the layout before thumbnails were
/// named after their content -- to `thumbs/ab/<sha>.jpg`, for the ids a `files` row still has,
/// and deletes the rest. Runs at every start and is idempotent: once nothing id-named is left
/// in `thumbs/`, it reads the directory and returns.
///
/// Only a row that was processed as an image (`width` set) keeps its thumbnail. An id-named
/// JPEG next to a row without one can only be what the old layout's bug left behind: the
/// thumbnail of an upload that rolled back, whose id SQLite then gave to a document. Moving it
/// would carry the wrong picture across; deleting it is what the move is for.
///
/// It cannot tell the same leftover apart when the id went to another *image* -- the old
/// thumbnail was overwritten by the new one in that case, so the file on disk is already the
/// right one. The residual case is an image whose own thumbnail write failed after a stray one
/// landed at its id; that stray is moved, as the old code would have served it anyway.
pub async fn migrate_legacy_thumbs(state: &App) -> Result<LegacyThumbs, AppError> {
    let mut report = LegacyThumbs::default();
    let mut entries = tokio::fs::read_dir(state.storage.thumbs_dir()).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_suffix(".jpg"))
            .and_then(|n| n.parse::<i64>().ok())
        else {
            continue;
        };
        if !entry.file_type().await?.is_file() {
            continue;
        }
        let path = entry.path();
        let row: Option<(String, Option<i64>)> =
            sqlx::query_as("SELECT sha256, width FROM files WHERE id = $1")
                .bind(id)
                .fetch_optional(&state.db)
                .await?;
        match row {
            Some((sha, Some(_))) => {
                let dest = state.storage.thumb_path(&sha);
                if tokio::fs::try_exists(&dest).await? {
                    tokio::fs::remove_file(&path).await?;
                } else {
                    tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                    tokio::fs::rename(&path, &dest).await?;
                }
                report.moved += 1;
            }
            _ => {
                tokio::fs::remove_file(&path).await?;
                report.deleted += 1;
            }
        }
    }
    Ok(report)
}
