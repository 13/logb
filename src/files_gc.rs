//! Keeping `files/` and `thumbs/` in step with the `files` table: the one-time move of
//! thumbnails from id names to content names, and the daily sweep of what no row references.

use crate::db;
use crate::error::AppError;
use crate::state::App;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long an unreferenced file is left alone before the sweep removes it.
///
/// Uploads and imports write blobs and thumbnails *before* the transaction that inserts the row
/// naming them, so for a moment every new file is an orphan. A day is far longer than any
/// request, which is what makes "unreferenced and a day old" mean "abandoned".
pub const GRACE: Duration = Duration::from_secs(24 * 3600);

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
///
/// Against a `files` table with no rows at all it does nothing: that is an instance pointed at
/// an empty or wrong database, not one whose thumbnails are all strays, and deleting them would
/// be permanent. The move then happens at the first start against the right database.
pub async fn migrate_legacy_thumbs(state: &App) -> Result<LegacyThumbs, AppError> {
    let mut report = LegacyThumbs::default();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files").fetch_one(&state.db).await?;
    if rows == 0 {
        return Ok(report);
    }
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

/// What one `sweep` removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    /// Blobs no `files` row names.
    pub blobs: usize,
    /// Thumbnails no `files` row names.
    pub thumbs: usize,
    /// Stranded `.part` files (a write interrupted before its rename) and `.tmp` scratch (an
    /// export or import interrupted before it removed its archive).
    pub scratch: usize,
}

/// A blob or thumbnail the sweep may remove, found while listing the directories.
struct Candidate {
    sha: String,
    path: PathBuf,
}

/// Unreferenced blobs and thumbnails at or above this many -- or a tenth of the hashes the
/// `files` table names, whichever is larger -- make a sweep stand down instead of deleting.
///
/// The ordinary leftovers (a rolled-back upload, a lost race) come a few at a time. Hundreds of
/// files no row names at once almost always mean the rows are what is missing: the instance was
/// pointed at another database, an empty one, or an older snapshot restored over the live one.
/// Deleting then would destroy the only copy of every attachment the rows forgot, and with it
/// the way back to the database `--restore` set aside.
pub const MISMATCH_FLOOR: usize = 10;

/// The daily sweep, with what it remembers between runs.
///
/// A blob or thumbnail is removed only once *two* sweeps, at least `grace` apart, have both
/// found no row naming it (and its file is older than `grace` too). The first sighting is only
/// noted. Kept in memory on purpose: a restart forgets every sighting, so an instance that was
/// just restored or repointed deletes nothing for at least `grace` of uptime -- time for an
/// operator who restored the wrong snapshot to notice and go back.
#[derive(Debug, Default)]
pub struct Sweeper {
    unreferenced_since: HashMap<PathBuf, SystemTime>,
}

impl Sweeper {
    pub fn new() -> Self {
        Self::default()
    }

    /// One sweep now. See `sweep_at`.
    pub async fn sweep(&mut self, state: &App, grace: Duration) -> Result<Swept, AppError> {
        self.sweep_at(state, grace, SystemTime::now()).await
    }

    /// Removes, from `files/` and `thumbs/`, what nothing needs: blobs and thumbnails whose hash
    /// no `files` row names, once an earlier sweep at least `grace` before `now` found the same,
    /// and `.part`/`.tmp` scratch older than `grace`. `now` is a parameter so tests can step a
    /// day forward without waiting one.
    ///
    /// These are what the ordinary paths leave behind when they fail between steps: an upload
    /// whose transaction rolled back after its blob and thumbnail were written, a crash between
    /// a `.part` write and its rename, an import killed while its archive sat in scratch. None of
    /// them is ever served -- a thumbnail or blob is only reached through a row naming its hash
    /// -- so this is about disk space, run daily from `tasks`.
    ///
    /// The directories are listed outside any transaction. The decision "no row names this
    /// hash" and the unlink are then made under the write connection, for the same reason as
    /// `api::attachments::discard_blob`: a writer that has already stored the blob and is about
    /// to insert its row either commits first (and this sees the row) or restores the blob after
    /// (see `Storage::restore_if_missing`). The age check is what spares a writer that has not
    /// even reached the lock yet.
    ///
    /// When the unreferenced files look like a database mismatch rather than leftovers (see
    /// `MISMATCH_FLOOR`), nothing but scratch is removed, sightings are forgotten, and the
    /// sweep says so in the log.
    pub async fn sweep_at(&mut self, state: &App, grace: Duration, now: SystemTime) -> Result<Swept, AppError> {
        let cutoff = now - grace;
        let mut swept = Swept::default();
        let mut blobs = Vec::new();
        let mut thumbs = Vec::new();
        let mut scratch = Vec::new();

        for entry in list(&state.storage.files_dir()).await? {
            if entry.is_dir {
                for inner in list(&entry.path).await? {
                    if inner.is_dir || !old(&inner.path, cutoff).await {
                        continue;
                    }
                    if inner.name.ends_with(".part") {
                        scratch.push(inner.path);
                    } else if is_sha(&inner.name) {
                        blobs.push(Candidate { sha: inner.name, path: inner.path });
                    }
                }
            } else if entry.name.ends_with(".tmp") && old(&entry.path, cutoff).await {
                scratch.push(entry.path);
            }
        }
        for entry in list(&state.storage.thumbs_dir()).await? {
            if !entry.is_dir {
                continue;
            }
            for inner in list(&entry.path).await? {
                if inner.is_dir || !old(&inner.path, cutoff).await {
                    continue;
                }
                if inner.name.ends_with(".part") {
                    scratch.push(inner.path);
                } else if let Some(sha) = inner.name.strip_suffix(".jpg").filter(|s| is_sha(s)) {
                    thumbs.push(Candidate { sha: sha.to_string(), path: inner.path.clone() });
                }
            }
        }

        // Scratch is nobody's: no row ever names a `.part` or a `.tmp`.
        for path in scratch {
            if tokio::fs::remove_file(&path).await.is_ok() {
                swept.scratch += 1;
            }
        }
        if blobs.is_empty() && thumbs.is_empty() {
            self.unreferenced_since.clear();
            return Ok(swept);
        }

        let mut tx = db::begin_write(state).await?;
        // One read of every hash rather than a lookup per candidate: every blob a day old is a
        // candidate, and `files.sha256` on its own is not what the unique index leads with.
        let named: HashSet<String> =
            sqlx::query_scalar::<_, String>("SELECT DISTINCT sha256 FROM files")
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .collect();
        let unreferenced: Vec<Candidate> = blobs
            .into_iter()
            .chain(thumbs)
            .filter(|c| !named.contains(&c.sha))
            .collect();
        let orphan_blobs = unreferenced.iter().filter(|c| c.path.extension().is_none()).count();
        if orphan_blobs >= MISMATCH_FLOOR.max(named.len() / 10) {
            tx.rollback().await?;
            self.unreferenced_since.clear();
            tracing::warn!(
                unreferenced_blobs = orphan_blobs,
                named_hashes = named.len(),
                "orphaned file sweep skipped: more files than a failed upload leaves behind have \
                 no row naming them, which looks like the database does not belong to this data \
                 directory; nothing was deleted"
            );
            return Ok(swept);
        }
        let mut seen = HashMap::new();
        for c in unreferenced {
            let first = self.unreferenced_since.get(&c.path).copied().unwrap_or(now);
            if first <= cutoff {
                if tokio::fs::remove_file(&c.path).await.is_ok() {
                    if c.path.extension().is_none() {
                        swept.blobs += 1;
                    } else {
                        swept.thumbs += 1;
                    }
                }
            } else {
                seen.insert(c.path, first);
            }
        }
        // Only what is still unreferenced carries its sighting forward: a file a row now names,
        // or one already gone, starts over if it is ever orphaned again.
        self.unreferenced_since = seen;
        // Nothing was written; the transaction was only this sweep's turn at the lock.
        tx.rollback().await?;
        Ok(swept)
    }
}

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// A directory's entries, or none if it does not exist. A name that is not UTF-8 cannot be
/// anything LogB wrote, and is skipped.
async fn list(dir: &Path) -> Result<Vec<Entry>, AppError> {
    let mut out = Vec::new();
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let is_dir = entry.file_type().await?.is_dir();
        out.push(Entry { name, path: entry.path(), is_dir });
    }
    Ok(out)
}

/// Whether `path` was last modified before `cutoff`. A file that vanished meanwhile, or whose
/// time cannot be read, counts as young: when in doubt, keep.
async fn old(path: &Path, cutoff: SystemTime) -> bool {
    match tokio::fs::metadata(path).await.and_then(|m| m.modified()) {
        Ok(modified) => modified < cutoff,
        Err(_) => false,
    }
}

/// Whether `name` is a lower-case hex sha256, the only name a blob or thumbnail is given.
fn is_sha(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
