use clap::Parser;
use std::path::PathBuf;

/// memto — complete history of your owned objects.
#[derive(Parser, Clone, Debug)]
#[command(name = "memto", version)]
pub struct Config {
    /// Directory for database, files and thumbnails.
    #[arg(long, env = "MEMTO_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,
    #[arg(long, env = "MEMTO_BIND", default_value = "0.0.0.0")]
    pub bind: String,
    #[arg(long, env = "MEMTO_PORT", default_value_t = 8080)]
    pub port: u16,
    #[arg(long, env = "MEMTO_MAX_UPLOAD_MB", default_value_t = 50)]
    pub max_upload_mb: usize,
    /// Largest import archive accepted by `POST /api/import`, in megabytes.
    #[arg(long, env = "MEMTO_MAX_IMPORT_MB", default_value_t = 1024)]
    pub max_import_mb: usize,
    /// auto | true | false — auto sets Secure when X-Forwarded-Proto is https.
    #[arg(long, env = "MEMTO_SECURE_COOKIE", default_value = "auto")]
    pub secure_cookie: String,
    #[arg(long, env = "MEMTO_LOG", default_value = "info")]
    pub log: String,
    /// Trust `X-Forwarded-For` for the client IP. Enable only behind a reverse
    /// proxy that overwrites the header; otherwise clients can spoof it.
    #[arg(long, env = "MEMTO_TRUST_PROXY", default_value_t = false)]
    pub trust_proxy: bool,
}

impl Config {
    pub fn max_upload_bytes(&self) -> usize {
        self.max_upload_mb * 1024 * 1024
    }

    pub fn max_import_bytes(&self) -> usize {
        self.max_import_mb * 1024 * 1024
    }

    /// Total number of bytes an import is allowed to decompress to. memto's own exports
    /// store file blobs uncompressed, so only `data.json` expands meaningfully; twice the
    /// accepted archive size leaves ample room for that while still bounding the memory a
    /// hostile archive can force the process to allocate.
    pub fn max_import_inflated_bytes(&self) -> usize {
        self.max_import_bytes().saturating_mul(2)
    }
}
