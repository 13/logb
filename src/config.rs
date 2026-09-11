use clap::Parser;
use std::path::PathBuf;

/// LogB — complete history of your owned objects.
#[derive(Parser, Clone, Debug)]
#[command(name = "logb", version)]
pub struct Config {
    /// Directory for database, files and thumbnails.
    #[arg(long, env = "LOGB_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,
    #[arg(long, env = "LOGB_BIND", default_value = "0.0.0.0")]
    pub bind: String,
    #[arg(long, env = "LOGB_PORT", default_value_t = 8080)]
    pub port: u16,
    #[arg(long, env = "LOGB_MAX_UPLOAD_MB", default_value_t = 50)]
    pub max_upload_mb: usize,
    /// Largest import archive accepted by `POST /api/import`, in megabytes.
    #[arg(long, env = "LOGB_MAX_IMPORT_MB", default_value_t = 1024)]
    pub max_import_mb: usize,
    /// auto | true | false — auto sets Secure when X-Forwarded-Proto is https.
    #[arg(long, env = "LOGB_SECURE_COOKIE", default_value = "auto")]
    pub secure_cookie: String,
    #[arg(long, env = "LOGB_LOG", default_value = "info")]
    pub log: String,
    /// Where to POST the daily digest of due reminders. Unset disables notifications.
    #[arg(long, env = "LOGB_NOTIFY_URL")]
    pub notify_url: Option<String>,
    /// Hour (0-23), in `LOGB_TIMEZONE`, at which the daily digest goes out.
    #[arg(long, env = "LOGB_NOTIFY_HOUR", default_value_t = 8)]
    pub notify_hour: u32,
    /// `json` posts a structured body; `text` posts the plain digest, which is what
    /// ntfy-style services render.
    #[arg(long, env = "LOGB_NOTIFY_FORMAT", default_value = "json")]
    pub notify_format: String,
    /// IANA timezone name (`Europe/Berlin`, `UTC`, ...). Decides which day a reminder's
    /// due date is compared against, and when the daily digest goes out.
    #[arg(long, env = "LOGB_TIMEZONE", default_value = "UTC")]
    pub timezone: chrono_tz::Tz,
    /// Write a consistent copy of the database to this path and exit, without stopping the
    /// server. Blobs under `files/` are content-addressed and never rewritten, so a plain
    /// copy of that directory pairs with it.
    #[arg(long, value_name = "PATH")]
    pub backup: Option<PathBuf>,
    /// Directory for nightly database snapshots. Unset disables automatic backup entirely, so
    /// an instance that has not opted in behaves exactly as it did before this existed.
    #[arg(long, env = "LOGB_BACKUP_DIR")]
    pub backup_dir: Option<PathBuf>,
    /// Hour (0-23), in `LOGB_TIMEZONE`, at which the nightly snapshot is written.
    #[arg(long, env = "LOGB_BACKUP_HOUR", default_value_t = 3)]
    pub backup_hour: u32,
    /// Replace the database with this snapshot and exit. The server must be stopped. The
    /// database being replaced is kept alongside it, and every synced device is sent back to a
    /// full bootstrap.
    #[arg(long, value_name = "PATH")]
    pub restore: Option<PathBuf>,
    /// Probe a running instance's `/api/health` on the configured port and exit 0 or 1.
    /// This is what the container's HEALTHCHECK runs -- the image has no shell or curl.
    #[arg(long)]
    pub healthcheck: bool,
    /// Trust `X-Forwarded-For` for the client IP. Enable only behind a reverse
    /// proxy that overwrites the header; otherwise clients can spoof it.
    #[arg(long, env = "LOGB_TRUST_PROXY", default_value_t = false)]
    pub trust_proxy: bool,
    /// Failed-or-successful login attempts allowed from one IP per minute, before further
    /// attempts are refused with 429. Every household behind one NAT shares a single address
    /// here, so raise it where many people sign in from the same place.
    #[arg(long, env = "LOGB_LOGIN_MAX_ATTEMPTS", default_value_t = 10)]
    pub login_max_attempts: u32,
    /// Comma-separated origins allowed to call the API from a browser on a DIFFERENT origin,
    /// e.g. a separate web client during development. Empty (the default) sends no CORS
    /// headers at all, which is what the bundled SPA needs, since it is same-origin.
    ///
    /// Credentials are never allowed on a cross-origin request, whatever is listed here: the
    /// session cookie must not ride along on a request some other site made. A cross-origin
    /// client authenticates with a bearer token, which only travels because that client chose
    /// to attach it.
    #[arg(long, env = "LOGB_CORS_ORIGINS", default_value = "")]
    pub cors_origins: String,
}

impl Config {
    /// The configured origins, parsed. An entry that is not a valid origin is dropped rather
    /// than crashing the server: a typo in one entry should not take the instance down.
    pub fn cors_origin_list(&self) -> Vec<axum::http::HeaderValue> {
        self.cors_origins
            .split(',')
            .map(str::trim)
            .filter(|o| !o.is_empty())
            .filter_map(|o| o.parse().ok())
            .collect()
    }

    pub fn max_upload_bytes(&self) -> usize {
        self.max_upload_mb * 1024 * 1024
    }

    pub fn max_import_bytes(&self) -> usize {
        self.max_import_mb * 1024 * 1024
    }

    /// Total number of bytes an import is allowed to decompress to. LogB's own exports
    /// store file blobs uncompressed, so only `data.json` expands meaningfully; twice the
    /// accepted archive size leaves ample room for that while still bounding the memory a
    /// hostile archive can force the process to allocate.
    pub fn max_import_inflated_bytes(&self) -> usize {
        self.max_import_bytes().saturating_mul(2)
    }
}
