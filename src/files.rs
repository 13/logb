use image::DynamicImage;
use rand::RngExt;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub const THUMB_MAX: u32 = 400;
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

    pub fn thumb_path(&self, file_id: i64) -> PathBuf {
        self.root.join("thumbs").join(format!("{file_id}.jpg"))
    }

    /// Write the blob unless an identical one already exists.
    pub async fn write_blob(&self, sha: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.blob_path(sha);
        if tokio::fs::try_exists(&path).await? {
            return Ok(());
        }
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        let mut token = [0u8; 16];
        rand::rng().fill(&mut token);
        let tmp = path.with_extension(format!("{}.part", hex::encode(token)));
        tokio::fs::write(&tmp, bytes).await?;
        match tokio::fs::rename(&tmp, &path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Another concurrent writer for the same content hash may have won the
                // race and already produced the destination file. Since the path is
                // derived from the content hash, that file has identical bytes to ours.
                //
                // Unreachable on Linux, where POSIX rename() replaces an existing
                // destination atomically; this arm is here for platforms whose rename
                // fails when the destination exists.
                if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    Ok(())
                } else {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    Err(e)
                }
            }
        }
    }

    pub async fn write_thumb(&self, file_id: i64, jpeg: &[u8]) -> std::io::Result<()> {
        tokio::fs::write(self.thumb_path(file_id), jpeg).await
    }

    pub async fn remove(&self, sha: &str, file_id: i64) {
        let _ = tokio::fs::remove_file(self.blob_path(sha)).await;
        let _ = tokio::fs::remove_file(self.thumb_path(file_id)).await;
    }
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

/// Decode, apply EXIF orientation, extract DateTimeOriginal, build a JPEG thumbnail.
/// Returns None when the bytes are not a decodable image.
pub fn process_image(bytes: &[u8]) -> Option<ImageInfo> {
    let img = image::load_from_memory(bytes).ok()?;
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

    #[test]
    fn blob_paths_are_sharded() {
        let s = Storage { root: PathBuf::from("/data") };
        assert_eq!(s.blob_path("abcdef"), PathBuf::from("/data/files/ab/abcdef"));
        assert_eq!(s.thumb_path(7), PathBuf::from("/data/thumbs/7.jpg"));
        assert_eq!(sha256_hex(b"").len(), 64);
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
