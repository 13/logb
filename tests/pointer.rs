//! Where LogB remembers which database to open. It cannot live in the database, because it
//! points away from it.

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
