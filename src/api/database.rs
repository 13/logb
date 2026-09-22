//! Choosing the database from Settings: describe the one in use, test another, copy onto it,
//! and restart into it.
//!
//! Four routes, all of them `AdminUser`, and one rule running through every line of them: a
//! PostgreSQL connection URL carries a password, and from the moment this screen exists that
//! password is inside the running process -- typed into a form, copied into a report, written
//! to a file beside the data. It must not come back out. Nothing here formats a URL into a
//! response, a message or a log line without `db::redacted` (for a URL this code holds) or
//! `db::scrub` (for text something else wrote, which may be quoting the URL back). They are the
//! established pair; a third spelling of the same idea is how the fourth one ends up wrong.
//!
//! The order in `switch` is the other load-bearing decision: the copy runs first and the
//! pointer file is written only if it succeeded. Until that file is written nothing has
//! changed -- the instance is still serving its old database, and a failed or abandoned
//! migration leaves an operator exactly where they started, with a half-filled destination they
//! can drop. Writing the pointer first would mean a failed copy had already arranged for the
//! next restart to open the wrong database.

use crate::auth::AdminUser;
use crate::db::{self, BoxError};
use crate::dialect::Backend;
use crate::error::AppError;
use crate::state::App;
use crate::{copy, pointer};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn router() -> Router<App> {
    Router::new()
        .route("/database", get(describe))
        .route("/database/test", post(test))
        .route("/database/switch", post(switch))
        .route("/database/restart", post(restart))
        .route("/database/backup", get(backup_status))
}

/// How long the process waits before exiting, so the 202 is on the wire first. See `restart`.
const RESTART_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// A database, said in a way that can be shown to somebody: no user, no password, no query
/// string. Everything here comes out of `db::redacted`, which is the one function in this
/// codebase that decides what is safe to say about a URL.
#[derive(Serialize)]
struct Location {
    backend: &'static str,
    /// `host:port` for PostgreSQL; `null` for SQLite, which has no host.
    host: Option<String>,
    /// The database name for PostgreSQL, the file for SQLite. `null` for an in-memory one.
    database: Option<String>,
}

impl Location {
    fn of(url: &str) -> Self {
        match Backend::of(url) {
            // A SQLite URL's "host" is its path. `sqlite_file` already knows how to read one,
            // including that `:memory:` is not a file.
            Backend::Sqlite => Self {
                backend: "sqlite",
                host: None,
                database: db::sqlite_file(url).map(|f| f.display().to_string()),
            },
            Backend::Postgres => {
                // Parsed out of the *redacted* URL rather than the real one, so a mistake in
                // this parsing can only ever expose something `redacted` had already passed.
                let safe = db::redacted(url);
                let rest = safe.split_once("://").map_or("", |(_, rest)| rest);
                let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
                let (host, path) = match rest.find('/') {
                    Some(at) => (&rest[..at], rest[at + 1..].to_string()),
                    None => (rest, String::new()),
                };
                Self {
                    backend: "postgres",
                    host: (!host.is_empty()).then(|| host.to_string()),
                    database: (!path.is_empty()).then_some(path),
                }
            },
        }
    }
}

#[derive(Serialize)]
struct Description {
    #[serde(flatten)]
    current: Location,
    /// Whether this instance's database may be chosen from here at all. False when
    /// `LOGB_DATABASE_URL` is set -- the environment always wins, so a pointer file written
    /// here would be ignored at the next start -- and false when the data directory will not
    /// take the file. Either way the screen can say why it is read-only instead of finding out
    /// when somebody presses save.
    pointer_writable: bool,
    /// The database a pointer file names when it is not the one being served: a switch has
    /// happened and the restart has not. `null` the rest of the time.
    pending: Option<Location>,
}

async fn describe(
    AdminUser(_): AdminUser,
    State(state): State<App>,
) -> Result<Json<Description>, AppError> {
    // `state.database_url`, not `config.database_url()`: after a switch those two differ, and
    // the one worth reporting as current is the database the pool is actually talking to.
    let current = Location::of(&state.database_url);
    let pending = state
        .config
        .pointed_database_url()
        .filter(|url| url != &state.database_url)
        .map(|url| Location::of(&url));
    Ok(Json(Description { current, pointer_writable: writable(&state), pending }))
}

/// Whether Settings may write the pointer file: the environment has not taken the decision
/// away, and the data directory takes a file.
fn writable(state: &App) -> bool {
    let environment_decides =
        state.config.database_url.as_deref().is_some_and(|url| !url.trim().is_empty());
    !environment_decides && pointer::writable(&state.config.data_dir)
}

#[derive(Deserialize)]
struct Target {
    url: String,
}

/// The URL out of a request body, trimmed, refusing a blank one.
///
/// A form submitted early sends `""`, and every layer below treats that as a URL it cannot
/// parse -- an error about a driver, for a field somebody simply had not filled in yet.
fn requested(target: &Target) -> Result<String, AppError> {
    let url = target.url.trim();
    if url.is_empty() {
        return Err(AppError::BadRequest("a database URL is required".into()));
    }
    Ok(url.to_string())
}

#[derive(Serialize)]
struct Probe {
    reachable: bool,
    /// What the server says it is: `PostgreSQL 16.4 ...`, or a SQLite version. `null` when
    /// there is nothing to ask -- an unreachable database, or a SQLite file not created yet.
    version: Option<String>,
    /// `empty`, `holds_logb_data` or `unreachable`.
    state: &'static str,
    /// Why, when that is worth saying. Always scrubbed against the URL it is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

const EMPTY: &str = "empty";
const HOLDS_DATA: &str = "holds_logb_data";
const UNREACHABLE: &str = "unreachable";

/// Looks at a database without changing it.
///
/// Deliberately not `db::connect`: that builds a pool, runs the migrations and seeds two
/// settings rows, which would turn "can I reach this?" into "this is now a LogB database" --
/// asked of the wrong URL, that is a schema created in somebody else's database. One
/// connection, two reads, closed.
async fn test(AdminUser(_): AdminUser, Json(body): Json<Target>) -> Result<Json<Probe>, AppError> {
    let url = requested(&body)?;
    Ok(Json(probe(&url).await))
}

async fn probe(url: &str) -> Probe {
    // A SQLite file that does not exist yet is not a failure and must not be created to find
    // that out: switching onto it is what creates it, and it will be empty when it does.
    if let Some(file) = db::sqlite_file(url) {
        if !file.exists() {
            return Probe {
                reachable: true,
                version: None,
                state: EMPTY,
                message: Some("no file there yet -- switching to it creates one".into()),
            };
        }
    }
    match inspect(url).await {
        Ok(probe) => probe,
        Err(e) => Probe {
            reachable: false,
            version: None,
            state: UNREACHABLE,
            // The driver's own text, which may be quoting the URL it was handed.
            message: Some(db::scrub(&e.to_string(), url)),
        },
    }
}

async fn inspect(url: &str) -> Result<Probe, BoxError> {
    use sqlx::Connection;
    sqlx::any::install_default_drivers();
    // `mode=rwc` would have SQLite create the database this is only being asked about.
    let url = match db::sqlite_file(url) {
        Some(_) => url.replace("mode=rwc", "mode=rw"),
        None => url.to_string(),
    };
    let mut conn = sqlx::AnyConnection::connect(&url).await?;
    let version_sql = match Backend::of(&url) {
        Backend::Sqlite => "SELECT sqlite_version()",
        Backend::Postgres => "SELECT version()",
    };
    let version: Option<String> = sqlx::query_scalar(version_sql).fetch_one(&mut conn).await.ok();
    // A database with no `users` table is not a LogB database at all, which for this question
    // is the same answer as one whose `users` table is empty: nothing here to lose.
    let users: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users").fetch_one(&mut conn).await.unwrap_or(0);
    let _ = conn.close().await;
    Ok(Probe {
        reachable: true,
        version,
        state: if users == 0 { EMPTY } else { HOLDS_DATA },
        message: None,
    })
}

#[derive(Serialize)]
struct Copied {
    table: String,
    rows: i64,
}

#[derive(Serialize)]
struct Switched {
    /// Every table and how many rows went into it, in the order they were copied.
    tables: Vec<Copied>,
    /// The identity the destination now advertises; every synced device re-bootstraps against
    /// it. Worth showing, because it is the visible consequence of the move.
    epoch: String,
    /// Where the choice was remembered.
    pointer: String,
    /// Nothing has moved yet: this instance is still serving the database it started on.
    restart_required: bool,
    /// The destination, said safely.
    database: Location,
}

/// Copies this instance's database into another one and remembers the choice.
///
/// **This request blocks for the whole copy, and that is the intended behaviour.**
/// `copy::run_live` reads inside `db::begin_write`, the transaction every write in this
/// application goes through, so for as long as it runs no write can land on the source and the
/// eleven tables are read from one point in time. Backgrounding it and answering 202 would not
/// make the instance usable during the copy -- writes would stall against that same lock either
/// way -- it would only hide from the operator that they are stalled, and leave them with no
/// answer to the one question that matters afterwards: did it verify? A migration is a
/// deliberate one-off act performed by somebody watching the screen, so the honest shape is a
/// request that takes as long as the copy takes and answers with the report. Reads are
/// unaffected throughout.
async fn switch(
    AdminUser(_): AdminUser,
    State(state): State<App>,
    Json(body): Json<Target>,
) -> Result<Json<Switched>, AppError> {
    let url = requested(&body)?;
    if state.config.database_url.as_deref().is_some_and(|url| !url.trim().is_empty()) {
        return Err(AppError::BadRequest(
            "LOGB_DATABASE_URL is set, so this instance's database is decided by its \
             environment: a database chosen here would be ignored at the next start. Unset it \
             to choose from Settings."
                .into(),
        ));
    }
    // `copy::run_live` is handed a pool rather than a source URL, so it cannot make this
    // comparison itself the way `copy::run` does. Without it, copying onto the database already
    // in use is refused several seconds later as "the destination already holds data" -- true,
    // and a thoroughly confusing answer to somebody who simply pasted the wrong URL.
    if url == state.database_url {
        return Err(AppError::BadRequest(
            "that is the database LogB is already running on".into(),
        ));
    }
    // Asked before the copy rather than after it: finding out that the pointer cannot be
    // written is worth a refusal, and worthless once a migration has already run.
    if !pointer::writable(&state.config.data_dir) {
        return Err(AppError::BadRequest(format!(
            "{} cannot be written, so the choice could not be remembered and the next start \
             would open the old database anyway. Make the data directory writable first.",
            pointer::path(&state.config.data_dir).display(),
        )));
    }

    let report = copy::run_live(&state, &url).await.map_err(|e| {
        let reason = db::scrub(&e.to_string(), &url);
        tracing::warn!(destination = %db::redacted(&url), error = %reason, "the copy to a new database failed");
        // Almost always a fact about the URL that was typed -- unreachable, not empty, wrong
        // credentials -- so it is shown rather than swallowed behind "internal error". It is
        // scrubbed, and the copy left nothing behind: `run_live` rolls the destination back.
        AppError::BadRequest(reason)
    })?;

    // Only now. Before this line nothing has changed and the operator can walk away.
    pointer::write(&state.config.data_dir, &url).map_err(|e| {
        tracing::error!(error = %db::scrub(&e.to_string(), &url), "the copy finished but the pointer could not be written");
        AppError::Unavailable(format!(
            "the copy finished, but {} could not be written, so the next start would open the \
             old database: {}. The copy is complete and can be pointed at by hand.",
            pointer::path(&state.config.data_dir).display(),
            db::scrub(&e.to_string(), &url),
        ))
    })?;
    tracing::info!(
        destination = %db::redacted(&url),
        tables = report.tables.len(),
        epoch = %report.epoch,
        "copied this instance's database and remembered the destination; a restart moves onto it",
    );

    Ok(Json(Switched {
        tables: report.tables.into_iter().map(|(table, rows)| Copied { table, rows }).collect(),
        epoch: report.epoch,
        pointer: pointer::path(&state.config.data_dir).display().to_string(),
        restart_required: true,
        database: Location::of(&url),
    }))
}

/// Exits the process, so the next start opens whatever the pointer file names.
///
/// There is no way to swap the pool underneath a running instance: half the application holds
/// `state.db` and a backend it decided once from the URL, background tasks included. A restart
/// is the whole mechanism, and it is honest about needing something to restart *into* --
/// systemd, a container restart policy, whatever supervises this process. Nothing here can
/// check that something does, so the answer says so plainly rather than implying the process
/// brings itself back.
///
/// 202 and not 200: the work is accepted, not done. The exit waits `RESTART_GRACE` so the
/// response is written and flushed first -- exiting from inside the handler would drop the
/// connection, and the client would see a transport error where it should see an
/// acknowledgement.
async fn restart(AdminUser(_): AdminUser) -> (StatusCode, Json<serde_json::Value>) {
    tokio::spawn(async move {
        tokio::time::sleep(RESTART_GRACE).await;
        tracing::info!("exiting on request from Settings; whatever supervises LogB starts it again");
        std::process::exit(0);
    });
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "restarting": true,
            "message": "LogB is exiting now. Whatever supervises it -- systemd, a container \
                        restart policy -- starts it again, on the database the pointer file \
                        names. If nothing supervises it, start it yourself.",
        })),
    )
}

/// Whether LogB is taking automatic backups of the database it is serving, and where.
#[derive(Serialize)]
struct BackupStatus {
    /// `scheduled` on SQLite with a recent directory snapshot, `stale` when the configured
    /// backup has not produced a snapshot recently, `off` on SQLite without one,
    /// `not_ours` on PostgreSQL -- where `crate::backup` never runs at all (see `backup::tick`
    /// and `NOT_ON_POSTGRES`), regardless of whether `LOGB_BACKUP_DIR` happens to be set.
    state: &'static str,
    /// The configured directory, or `null` for `off` and `not_ours`.
    directory: Option<String>,
    /// The newest snapshot's timestamp, or `null` when the directory has none yet -- including
    /// a directory that does not exist yet, which is what a fresh `scheduled` instance looks
    /// like before its first run.
    last_at: Option<String>,
    /// The configured hour, or `null` for `off` and `not_ours`.
    hour: Option<u32>,
}

const NOT_OURS: &str = "not_ours";
const OFF: &str = "off";
const SCHEDULED: &str = "scheduled";
const STALE: &str = "stale";

/// The modified time of the newest file `crate::backup::tick` would have written
/// (`logb-*.db`), converted to the instance's configured timezone -- the same one `tick` used
/// to name the file and to decide whether to write it, so this stays "today" for as long as the
/// filename and `db::local_hour()` beside it agree it is. `None` for a directory with no
/// snapshot yet, or one that does not exist at all -- neither is an error here, since `tick`
/// creates the directory itself on its first run.
fn newest_snapshot(dir: &std::path::Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let newest = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_str().is_some_and(|n| n.starts_with("logb-") && n.ends_with(".db"))
        })
        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
        .max()?;
    let secs = newest.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let at = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0)?;
    Some(at.with_timezone(&crate::db::timezone()).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Reports who is responsible for backing up this database up: LogB itself (SQLite, with the
/// schedule it runs on), nobody through LogB (SQLite with no directory configured), or somebody
/// else entirely (PostgreSQL). The backend decides `not_ours` before anything else is looked at
/// -- `LOGB_BACKUP_DIR` may well be set on an instance that was migrated to PostgreSQL, left
/// over from the compose file it started life on, and it means nothing there.
async fn backup_status(AdminUser(_): AdminUser, State(state): State<App>) -> Json<BackupStatus> {
    if state.backend != Backend::Sqlite {
        return Json(BackupStatus { state: NOT_OURS, directory: None, last_at: None, hour: None });
    }
    let Some(dir) = state.config.backup_dir.clone() else {
        return Json(BackupStatus { state: OFF, directory: None, last_at: None, hour: None });
    };
    let last_at = newest_snapshot(&dir);
    let stale = last_at.as_deref().and_then(|stamp| chrono::DateTime::parse_from_rfc3339(stamp).ok())
        .map(|at| chrono::Utc::now().signed_duration_since(at.with_timezone(&chrono::Utc)) > chrono::Duration::hours(36))
        .unwrap_or(false);
    Json(BackupStatus {
        state: if stale { STALE } else { SCHEDULED },
        last_at,
        directory: Some(dir.display().to_string()),
        hour: Some(state.config.backup_hour),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parsing in `Location` is the one place this module takes a URL apart itself, and it
    /// does it to already-redacted text. These pin that nothing of the userinfo survives.
    #[test]
    fn a_postgres_location_keeps_the_host_and_the_database_and_nothing_else() {
        let at = Location::of("postgres://ben:hunter2@db.example:5432/logb?sslmode=require");
        assert_eq!(at.backend, "postgres");
        assert_eq!(at.host.as_deref(), Some("db.example:5432"));
        assert_eq!(at.database.as_deref(), Some("logb"));
    }

    #[test]
    fn a_postgres_location_without_credentials_still_reads() {
        let at = Location::of("postgres://db.example/logb");
        assert_eq!(at.host.as_deref(), Some("db.example"));
        assert_eq!(at.database.as_deref(), Some("logb"));
    }

    #[test]
    fn a_sqlite_location_is_its_file() {
        let at = Location::of("sqlite:///var/lib/logb/logb.db?mode=rwc");
        assert_eq!(at.backend, "sqlite");
        assert_eq!(at.host, None);
        assert_eq!(at.database.as_deref(), Some("/var/lib/logb/logb.db"));
    }

    #[test]
    fn an_in_memory_sqlite_location_names_no_file() {
        let at = Location::of("sqlite::memory:");
        assert_eq!(at.backend, "sqlite");
        assert_eq!(at.database, None);
    }

    /// The assertion this whole module exists for, made directly against the serialized bytes.
    #[test]
    fn no_part_of_a_url_reaches_the_serialized_description() {
        let described = serde_json::to_string(&Description {
            current: Location::of("postgres://ben:hunter2@db.example:5432/logb?sslmode=require"),
            pointer_writable: true,
            pending: Some(Location::of("postgres://root:swordfish@other.example/moved")),
        })
        .unwrap();
        for secret in ["hunter2", "swordfish", "ben", "root", "sslmode"] {
            assert!(!described.contains(secret), "{secret} survived into {described}");
        }
    }

    #[test]
    fn a_blank_url_is_refused_before_anything_tries_to_connect() {
        let refused = requested(&Target { url: "   ".into() });
        assert!(matches!(refused, Err(AppError::BadRequest(_))));
    }
}
