use crate::config::Config;
use sqlx::AnyPool;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct AppState {
    pub db: AnyPool,
    /// The pool write transactions come from: on SQLite one connection, so writers queue for
    /// their turn instead of racing the file lock; on PostgreSQL the same pool as `db`. See
    /// `db::connect_writer`.
    pub write_db: AnyPool,
    /// The URL `db` was opened from -- what this instance is *actually* serving, which is not
    /// always what `config.database_url()` would answer now: writing the pointer file from
    /// Settings changes that answer immediately, while the pool goes on serving the database it
    /// opened until the process restarts. Anything describing or comparing against the live
    /// database has to read it here, or it will describe a database nobody is connected to.
    ///
    /// It holds a password on PostgreSQL. `db::redacted` is how it reaches a human.
    pub database_url: String,
    /// Which database `db` is, for the handful of statements the two spell differently.
    /// Decided once from the connection URL rather than re-derived per request.
    pub backend: crate::dialect::Backend,
    pub storage: crate::files::Storage,
    pub config: Config,
    /// login attempts per IP: (count, window start)
    pub login_attempts: Mutex<HashMap<IpAddr, (u32, Instant)>>,
    /// login attempts per lower-cased username, same shape and window as `login_attempts`.
    /// See `auth::check_username_rate`.
    pub login_attempts_by_user: Mutex<HashMap<String, (u32, Instant)>>,
    /// The snapshot `backup::tick` last wrote-and-verified, or found and verified, in this
    /// process. Today's file, once it is in here, is not opened again until tomorrow's name
    /// replaces it -- see `backup::tick`. In memory on purpose: after a restart nothing is
    /// remembered, so a file this process never checked is checked once.
    pub backup_verified: Mutex<Option<std::path::PathBuf>>,
    /// Cancelled once, when the process should stop: SIGTERM or ctrl-c (see `main`), or
    /// Settings -> Restart (`api::database::restart`). The server stops accepting connections
    /// and drains, and the background loops (`tasks`, `telegram`) end at their next wait.
    /// One token for every reason to stop, so a restart is exactly as orderly as a
    /// `docker stop`.
    pub shutdown: tokio_util::sync::CancellationToken,
    /// `telegram.key`, once it has been read or written. It never changes while the instance
    /// runs, and the Telegram loop used to read it from disk on every poll.
    pub telegram_key: std::sync::OnceLock<[u8; 32]>,
    /// Request and write-lock counters behind `/metrics`. Kept whether or not the endpoint is
    /// enabled: counting costs a few atomics, and it keeps one code path.
    pub metrics: crate::metrics::Metrics,
}

pub type App = Arc<AppState>;
