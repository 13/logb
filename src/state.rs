use crate::config::Config;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct AppState {
    pub db: SqlitePool,
    pub config: Config,
    /// login attempts per IP: (count, window start)
    pub login_attempts: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

pub type App = Arc<AppState>;
