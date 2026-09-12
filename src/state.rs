use crate::config::Config;
use sqlx::AnyPool;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct AppState {
    pub db: AnyPool,
    /// Which database `db` is, for the handful of statements the two spell differently.
    /// Decided once from the connection URL rather than re-derived per request.
    pub backend: crate::dialect::Backend,
    pub storage: crate::files::Storage,
    pub config: Config,
    /// login attempts per IP: (count, window start)
    pub login_attempts: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

pub type App = Arc<AppState>;
