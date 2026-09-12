//! Where LogB remembers which database to open. It cannot live in the database, because it
//! points away from it.

mod common;

use clap::Parser;

#[test]
fn the_pointer_is_written_readable_only_by_its_owner() {
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), "postgres://user:secret@db/logb").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(logb::pointer::path(dir.path())).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the file holds a password: {mode:o}");
    }
    assert_eq!(logb::pointer::read(dir.path()).as_deref(), Some("postgres://user:secret@db/logb"));
}

#[test]
fn the_environment_wins_over_the_file() {
    // An operator who sets the environment cannot have it changed from a browser.
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), "postgres://from-the-file/logb").unwrap();
    let config = logb::config::Config::parse_from(["logb", "--data-dir", dir.path().to_str().unwrap()]);
    let config = logb::config::Config {
        database_url: Some("postgres://from-the-environment/logb".into()),
        ..config
    };
    assert_eq!(config.database_url().unwrap(), "postgres://from-the-environment/logb");
}

#[test]
fn no_pointer_and_no_environment_means_the_sqlite_file_beside_the_data() {
    let dir = tempfile::tempdir().unwrap();
    let config = logb::config::Config::parse_from(["logb", "--data-dir", dir.path().to_str().unwrap()]);
    assert_eq!(config.database_url().unwrap(), logb::db::sqlite_url(dir.path()).unwrap());
}

/// A pointer naming a database that cannot be reached must stop the server, not start it on
/// something else. Starting on the SQLite default instead would silently serve the old data
/// after a migration the operator believes succeeded.
#[tokio::test]
async fn an_unreachable_pointer_refuses_to_start() {
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), "postgres://nobody:nothing@127.0.0.1:1/logb").unwrap();
    let config = logb::config::Config::parse_from(["logb", "--data-dir", dir.path().to_str().unwrap()]);
    let err = logb::build(config).await.unwrap_err().to_string();
    assert!(err.contains("database.url"), "the error must name the pointer: {err}");
    assert!(!err.contains("nothing"), "the password must not appear in the error: {err}");
}

/// A pointer naming an empty database is the same failure wearing a friendlier face: the schema
/// would be created and the instance would come up with no data, looking healthy.
#[tokio::test]
async fn a_pointer_to_an_empty_database_refuses_to_start() {
    let Ok(server) = std::env::var("LOGB_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED: needs LOGB_TEST_DATABASE_URL");
        return;
    };
    let (_empty, empty_url) = common::scratch_database_on(&server).await;
    let dir = tempfile::tempdir().unwrap();
    logb::pointer::write(dir.path(), &empty_url).unwrap();
    let config = logb::config::Config::parse_from(["logb", "--data-dir", dir.path().to_str().unwrap()]);
    let err = logb::build(config).await.unwrap_err().to_string();
    assert!(err.contains("no users"), "the error must say what is wrong: {err}");
}
