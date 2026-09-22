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
}

pub type App = Arc<AppState>;
