use image::DynamicImage;
use rand::RngExt;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub const THUMB_MAX: u32 = 400;
pub const MAX_IMAGE_SIDE: u32 = 12_000;
const DOC_EXTENSIONS: [&str; 6] = ["txt", "md", "doc", "docx", "xls", "xlsx"];

#[derive(Clone, Debug)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(data_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(data_dir.join("files"))?;
        std::fs::create_dir_all(data_dir.join("thumbs"))?;
        Ok(Storage { root: data_dir.to_path_buf() })
    }

    pub fn blob_path(&self, sha: &str) -> PathBuf {
        self.root.join("files").join(&sha[..2]).join(sha)
    }

    /// Where the thumbnail of the content hashing to `sha` lives: `thumbs/ab/<sha>.jpg`, sharded
    /// like the blobs.
    ///
    /// Named after the content, not the `files` row, and that is the point. It used to be
    /// `thumbs/<file_id>.jpg`, written inside the transaction that inserted the row -- and SQLite
    /// hands a rolled-back `AUTOINCREMENT` id straight to the next insert, so a failed upload
    /// left a thumbnail on disk that the next, unrelated file with the same id then served as its
    /// own. A hash names exactly one picture, whichever row points at it and whenever it was
    /// written, so a thumbnail written by a request that later failed is at worst an orphan the
    /// sweep collects, never someone else's image. It also means two rows with the same bytes --
    /// two users uploading one photo -- share one thumbnail, as they already share one blob.
    pub fn thumb_path(&self, sha: &str) -> PathBuf {
        self.thumbs_dir().join(&sha[..2]).join(format!("{sha}.jpg"))
    }

    /// The directory holding the blob shards (and scratch files).
    pub fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    /// The directory holding the thumbnail shards -- and, until `files_gc::migrate_legacy_thumbs`
    /// has run once, the old id-named thumbnails directly inside it.
    pub fn thumbs_dir(&self) -> PathBuf {
        self.root.join("thumbs")
    }

    /// Writes the blob, unless an intact one is already there.
    ///
    /// "Intact" is checked, not assumed: an existing file under this name is hashed, and only a
    /// match is kept. The name promises the content, but a write torn by a crash before this
    /// function fsynced, or a disk that rotted a sector, breaks that promise silently -- and
    /// every later upload of the same bytes used to find the file "already there" and leave the
    /// damage in place for good. A mismatch is rewritten from the bytes in hand.
    ///
    /// The write itself is durable before this returns: see `write_durably`.
    pub async fn write_blob(&self, sha: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.blob_path(sha);
        if tokio::fs::try_exists(&path).await? {
            match hash_file(&path).await {
                Ok(on_disk) if on_disk == sha => return Ok(()),
                Ok(on_disk) => tracing::warn!(
                    sha, on_disk = %on_disk, "a stored blob does not match its name; rewriting it"
                ),
                Err(e) => tracing::warn!(sha, error = %e, "a stored blob cannot be read; rewriting it"),
            }
        }
        write_durably(&path, bytes).await
    }

    /// A unique path in the data directory for a temporary working file (a whole export
    /// archive, say). Sits next to the blobs so it lands on the same filesystem, and is the
    /// caller's to delete.
    pub fn scratch_path(&self, prefix: &str) -> PathBuf {
        let mut token = [0u8; 16];
        rand::rng().fill(&mut token);
        self.root.join("files").join(format!(".{prefix}-{}.tmp", hex::encode(token)))
    }

    /// Writes the thumbnail of the content hashing to `sha`. Callers write it before the
    /// transaction that inserts the `files` row opens, so the row never becomes visible ahead of
    /// its thumbnail -- and since the name is the content's, a transaction that then fails leaves
    /// an orphan, not a wrong picture (see `thumb_path`).
    ///
    /// Through a temporary file and a rename, so a reader never sees half a JPEG, and always
    /// written rather than skipped when one exists: the bytes are a pure function of the content,
    /// so replacing an identical file changes nothing, while trusting a torn one would serve it
    /// forever.
    pub async fn write_thumb(&self, sha: &str, jpeg: &[u8]) -> std::io::Result<()> {
        write_durably(&self.thumb_path(sha), jpeg).await
    }

    /// Writes the blob, and the thumbnail if there is one, again if either is missing from disk.
    ///
    /// For a writer that stored both before taking the write lock and now holds it: a
    /// concurrent `discard_blob` may have deleted them in between, having seen no row naming
    /// the hash -- this writer's row was not committed yet. Only a stat each in the common case,
    /// which is what makes it cheap enough to run inside the write transaction.
    pub async fn restore_if_missing(&self, sha: &str, bytes: &[u8], thumb: Option<&[u8]>) -> std::io::Result<()> {
        if !tokio::fs::try_exists(self.blob_path(sha)).await? {
            write_durably(&self.blob_path(sha), bytes).await?;
        }
        if let Some(jpeg) = thumb {
            if !tokio::fs::try_exists(self.thumb_path(sha)).await? {
                self.write_thumb(sha, jpeg).await?;
            }
        }
        Ok(())
    }

    /// Removes the blob and the thumbnail of the content hashing to `sha`. The caller decides
    /// that nothing references them any more -- see `api::attachments::discard_blob`.
    pub async fn remove(&self, sha: &str) {
        let _ = tokio::fs::remove_file(self.blob_path(sha)).await;
        let _ = tokio::fs::remove_file(self.thumb_path(sha)).await;
    }
}

/// Puts `bytes` at `path` so that, once this returns, a crash cannot leave `path` holding
/// anything but the whole of them.
///
/// Written to a uniquely named `.part` file beside the destination (same directory, so the same
/// filesystem, so the rename is atomic), fsynced, renamed over the destination, and then the
/// directory is fsynced too. The first fsync is what stops a crash from leaving a renamed but
/// empty or partial file -- a rename can reach the disk before the data it points at does. The
/// second makes the rename itself durable, so the file cannot vanish again after the caller has
/// gone on to commit a row that names it. A `.part` file a crash strands is collected by
/// `files_gc::Sweeper`.
async fn write_durably(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let dir = path.parent().expect("a stored file always has a shard directory");
    tokio::fs::create_dir_all(dir).await?;
    let mut token = [0u8; 16];
    rand::rng().fill(&mut token);
    let tmp = path.with_extension(format!("{}.part", hex::encode(token)));
    let written = async {
        let mut f = tokio::fs::File::create(&tmp).await?;
        f.write_all(bytes).await?;
        f.sync_all().await
    }
    .await;
    if let Err(e) = written {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    if let Err(e) = tokio::fs::rename(&tmp, path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        // Another concurrent writer of the same content may have won the race and already
        // produced the destination; its name is derived from the content, so it is identical.
        //
        // Unreachable on Linux, where POSIX rename() replaces an existing destination
        // atomically; this arm is here for platforms whose rename fails when the destination
        // exists.
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            return Err(e);
        }
    }
    sync_dir(dir).await
}

/// Flushes a directory's entries (a rename into it, say) to disk.
#[cfg(unix)]
async fn sync_dir(dir: &Path) -> std::io::Result<()> {
    tokio::fs::File::open(dir).await?.sync_all().await
}

/// Directories cannot be opened for fsync everywhere (Windows refuses); there the rename's own
/// durability is what the platform gives.
#[cfg(not(unix))]
async fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The sha256 of a file's contents, read in chunks rather than into memory whole.
pub async fn hash_file(path: &Path) -> std::io::Result<String> {
    use tokio::io::AsyncReadExt;
    let mut f = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn extension(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Prefer the extension-derived type; fall back to the declared one; then octet-stream.
pub fn resolve_mime(name: &str, declared: Option<&str>) -> String {
    if let Some(m) = mime_guess::from_path(name).first() {
        return m.essence_str().to_string();
    }
    declared.filter(|d| !d.is_empty()).unwrap_or("application/octet-stream").to_string()
}

pub fn is_allowed(mime: &str, name: &str) -> bool {
    mime.starts_with("image/") || mime == "application/pdf" || DOC_EXTENSIONS.contains(&extension(name).as_str())
}

pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub taken_at: Option<String>,
    pub thumb_jpeg: Vec<u8>,
}

/// How many images may be decoding at once, across every upload and import.
const DECODE_SLOTS: usize = 2;
static DECODES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(DECODE_SLOTS);

/// `process_image` on a blocking thread, at most `DECODE_SLOTS` at a time.
///
/// A decode holds the whole bitmap -- up to `MAX_DECODE_ALLOC` -- plus the rotated copy and the
/// thumbnail. Ten photos uploaded at once from a phone used to mean ten of those in memory
/// together, on a server sized for a household; waiting for a slot costs a moment instead.
/// A task that panics while decoding surfaces as an error rather than taking the request down.
pub async fn process_image_queued(
    bytes: impl AsRef<[u8]> + Send + 'static,
) -> Result<Option<ImageInfo>, tokio::task::JoinError> {
    let _slot = DECODES.acquire().await.expect("the decode semaphore is never closed");
    tokio::task::spawn_blocking(move || process_image(bytes.as_ref())).await
}

/// The most a decoder may allocate for one image. Generous for any camera -- a 12 000 x 12 000
/// RGBA bitmap is 550 MiB, so the side limit usually binds first -- but a bound, where the
/// default is double this.
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;

/// Decode, apply EXIF orientation, extract DateTimeOriginal, build a JPEG thumbnail.
/// Returns None when the bytes are not a decodable image -- including one whose header promises
/// more than `MAX_IMAGE_SIDE` pixels a side or needs more than `MAX_DECODE_ALLOC` to decode.
/// A few kilobytes of PNG can declare a 100 000 x 100 000 canvas; without limits the decoder
/// would try to allocate it. Such a file is still stored, as a document without a thumbnail.
pub fn process_image(bytes: &[u8]) -> Option<ImageInfo> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let exif = exif::Reader::new().read_from_container(&mut Cursor::new(bytes)).ok();
    let orientation = exif.as_ref()
        .and_then(|e| e.get_field(exif::Tag::Orientation, exif::In::PRIMARY))
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1);
    let taken_at = exif.as_ref()
        .and_then(|e| e.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY))
        .and_then(|f| exif_datetime_to_iso(&f.display_value().to_string()));
    let img = apply_orientation(img, orientation);
    let thumb = img.thumbnail(THUMB_MAX, THUMB_MAX);
    let mut out = Cursor::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
    enc.encode_image(&thumb.to_rgb8()).ok()?;
    Some(ImageInfo { width: img.width(), height: img.height(), taken_at, thumb_jpeg: out.into_inner() })
}

fn apply_orientation(img: DynamicImage, o: u32) -> DynamicImage {
    match o {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// EXIF `2024:05:01 12:00:00` (or `2024-05-01 12:00:00`) -> `2024-05-01T12:00:00`.
pub fn exif_datetime_to_iso(s: &str) -> Option<String> {
    let s = s.trim();
    for fmt in ["%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.format("%Y-%m-%dT%H:%M:%S").to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_and_allow_list() {
        assert_eq!(resolve_mime("a.PNG", None), "image/png");
        assert_eq!(resolve_mime("a.pdf", Some("text/plain")), "application/pdf");
        assert_eq!(resolve_mime("noext", Some("image/jpeg")), "image/jpeg");
        assert!(is_allowed("image/heic", "x.heic"));
        assert!(is_allowed("application/octet-stream", "manual.DOCX"));
        assert!(!is_allowed("application/octet-stream", "virus.exe"));
    }

    #[test]
    fn exif_dates() {
        assert_eq!(exif_datetime_to_iso("2024:05:01 12:00:00").as_deref(), Some("2024-05-01T12:00:00"));
        assert_eq!(exif_datetime_to_iso("2024-05-01 12:00:00").as_deref(), Some("2024-05-01T12:00:00"));
        assert_eq!(exif_datetime_to_iso("garbage"), None);
    }

    #[test]
    fn thumbnail_keeps_aspect() {
        let img = DynamicImage::new_rgb8(1000, 500);
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let info = process_image(buf.get_ref()).unwrap();
        assert_eq!((info.width, info.height), (1000, 500));
        let t = image::load_from_memory(&info.thumb_jpeg).unwrap();
        assert_eq!((t.width(), t.height()), (400, 200));
        assert!(process_image(b"not an image").is_none());
    }

    /// A PNG header is a few bytes; the pixels it promises are what gets allocated. An image
    /// past the side limit is not decoded, so it gets no thumbnail and is stored as a document.
    #[test]
    fn images_past_the_side_limit_are_not_decoded() {
        let png = |w: u32, h: u32| {
            let mut buf = Cursor::new(Vec::new());
            DynamicImage::new_luma8(w, h).write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        };
        assert!(process_image(&png(MAX_IMAGE_SIDE, 1)).is_some());
        assert!(process_image(&png(MAX_IMAGE_SIDE + 1, 1)).is_none());
        assert!(process_image(&png(1, MAX_IMAGE_SIDE + 1)).is_none());
    }

    #[test]
    fn blob_paths_are_sharded() {
        let s = Storage { root: PathBuf::from("/data") };
        assert_eq!(s.blob_path("abcdef"), PathBuf::from("/data/files/ab/abcdef"));
        assert_eq!(s.thumb_path("abcdef"), PathBuf::from("/data/thumbs/ab/abcdef.jpg"));
        assert_eq!(sha256_hex(b"").len(), 64);
    }

    /// A blob that exists under its hash is not taken on trust: a torn write from before a
    /// crash, or bit rot, would otherwise be served forever, since every later upload of the
    /// same bytes would find the file "already there" and skip it.
    #[tokio::test]
    async fn write_blob_replaces_a_damaged_blob_under_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).unwrap();
        let bytes = b"the real content".to_vec();
        let sha = sha256_hex(&bytes);
        let path = storage.blob_path(&sha);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"the real con").unwrap();

        storage.write_blob(&sha, &bytes).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        // And an intact one is left exactly as it is.
        storage.write_blob(&sha, &bytes).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[tokio::test]
    async fn concurrent_write_blob_for_same_content_never_errors_or_leaves_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).unwrap();
        let bytes = b"identical bytes uploaded concurrently".to_vec();
        let sha = sha256_hex(&bytes);

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let storage = storage.clone();
            let sha = sha.clone();
            let bytes = bytes.clone();
            tasks.push(tokio::spawn(async move { storage.write_blob(&sha, &bytes).await }));
        }
        for t in tasks {
            assert!(t.await.unwrap().is_ok());
        }

        let path = storage.blob_path(&sha);
        assert_eq!(tokio::fs::read(&path).await.unwrap(), bytes);

        let shard_dir = path.parent().unwrap();
        let mut entries = tokio::fs::read_dir(shard_dir).await.unwrap();
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        assert_eq!(names, vec![sha], "no stray temporary files left behind: {names:?}");
    }
}
