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
    /// Copy this database into another one and exit. The destination must be empty. Blobs are
    /// not moved: they are content-addressed files under the data directory, and a copy of that
    /// directory pairs with any database.
    #[arg(long, value_name = "URL")]
    pub copy_to: Option<String>,
    /// Copy into a destination that already holds data. Two histories in one database.
    #[arg(long, requires = "copy_to")]
    pub force: bool,
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
    /// Full database connection URL. Unset (the default) uses the SQLite file inside
    /// `LOGB_DATA_DIR`, which is where LogB has always kept it. Set this to point at
    /// PostgreSQL instead; `LOGB_DATA_DIR` still decides where blobs live either way.
    #[arg(long, env = "LOGB_DATABASE_URL")]
    pub database_url: Option<String>,
    /// Maximum connections in the database pool. Unset (the default) keeps today's behaviour:
    /// 4 for SQLite, 16 for everything else. Exists so the test harness -- which opens one pool
    /// per test and runs many tests in parallel -- can ask for a small pool instead of exhausting
    /// a stock PostgreSQL server's `max_connections`.
    #[arg(long, env = "LOGB_DB_POOL_SIZE")]
    pub db_pool_size: Option<u32>,
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

    /// The database to open.
    ///
    /// The `database_url` field (`LOGB_DATABASE_URL`) when it is set and non-blank, otherwise
    /// the SQLite file inside the data directory -- which is where LogB has always kept it.
    /// `LOGB_DATA_DIR` is unchanged either way: it still decides where blobs live, and it is
    /// still the default database location.
    pub fn database_url(&self) -> Result<String, crate::db::BoxError> {
        match &self.database_url {
            Some(url) if !url.trim().is_empty() => Ok(url.clone()),
            _ => crate::db::sqlite_url(&self.data_dir),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(data_dir: PathBuf) -> Config {
        Config {
            data_dir,
            bind: "127.0.0.1".into(),
            port: 0,
            max_upload_mb: 2,
            max_import_mb: 4,
            notify_url: None,
            notify_hour: 8,
            notify_format: "json".into(),
            timezone: chrono_tz::Tz::UTC,
            backup: None,
            backup_dir: None,
            backup_hour: 3,
            restore: None,
            copy_to: None,
            force: false,
            healthcheck: false,
            secure_cookie: "false".into(),
            log: "warn".into(),
            trust_proxy: false,
            login_max_attempts: 10,
            cors_origins: String::new(),
            database_url: None,
            db_pool_size: None,
        }
    }

    /// `LOGB_DATABASE_URL` used to be read straight from the process environment, bypassing
    /// the `Config` value entirely -- a `Config` a test built around its own temporary
    /// directory was overridden by whatever happened to be exported in the shell. `database_url`
    /// is now a clap field: a `Config` with `database_url: None` must fall back to its own
    /// `data_dir` regardless of what `LOGB_DATABASE_URL` says in the environment, because
    /// nothing in this path reads the environment directly any more.
    #[test]
    fn a_config_with_no_database_url_field_uses_its_own_data_dir_even_if_the_environment_has_one() {
        // SAFETY: no other test in this process reads or writes LOGB_DATABASE_URL, and this
        // test restores whatever was there before it returns.
        let previous = std::env::var("LOGB_DATABASE_URL").ok();
        unsafe {
            std::env::set_var("LOGB_DATABASE_URL", "sqlite://somewhere/else/logb.db?mode=rwc");
        }

        let config = base_config(PathBuf::from("/this/tests/own/data/dir"));
        let url = config.database_url();

        match previous {
            Some(v) => unsafe { std::env::set_var("LOGB_DATABASE_URL", v) },
            None => unsafe { std::env::remove_var("LOGB_DATABASE_URL") },
        }

        assert_eq!(url.unwrap(), "sqlite:///this/tests/own/data/dir/logb.db?mode=rwc");
    }

    /// The field, not a bare environment read, is what wins when it is set.
    #[test]
    fn a_config_with_a_database_url_field_uses_it_regardless_of_data_dir() {
        let mut config = base_config(PathBuf::from("/unused"));
        config.database_url = Some("postgres://localhost/logb".into());
        assert_eq!(config.database_url().unwrap(), "postgres://localhost/logb");
    }
}
