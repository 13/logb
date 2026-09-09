mod common;

#[tokio::test]
async fn a_run_writes_and_verifies_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 3;
    }).await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    assert!(logby::backup::tick(&app.state, 2).await.unwrap().is_none(), "too early in the day");

    let made = logby::backup::tick(&app.state, 3).await.unwrap().expect("a snapshot was due");
    assert!(made.exists());
    assert_eq!(made.file_name().unwrap(), format!("logby-{}.db", logby::db::today()).as_str());
    logby::backup::verify(&made).await.expect("the snapshot opens and passes integrity_check");

    // The snapshot is the real database, not an empty file that happens to be valid SQLite.
    let pool = logby::db::connect_existing(made.parent().unwrap()).await;
    assert!(pool.is_err(), "connect_existing looks for logby.db, not a dated snapshot");
    let count: i64 = {
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&made).read_only(true);
        let p = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query_scalar("SELECT count(*) FROM objects").fetch_one(&p).await.unwrap()
    };
    assert_eq!(count, 1, "the object is in the snapshot");

    assert!(logby::backup::tick(&app.state, 3).await.unwrap().is_none(), "already done today");
}

#[tokio::test]
async fn backup_is_off_unless_a_directory_is_configured() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert!(logby::backup::tick(&app.state, 23).await.unwrap().is_none());
}

#[tokio::test]
async fn a_corrupt_snapshot_is_rejected_and_the_previous_one_survives() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    let good = backups.join("logby-2020-01-01.db");
    std::fs::write(&good, b"pretend this is yesterday's good snapshot").unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;

    // Today's slot already holds a file that is not a database at all.
    let today = backups.join(format!("logby-{}.db", logby::db::today()));
    std::fs::write(&today, b"not a database").unwrap();

    let made = logby::backup::tick(&app.state, 1).await.unwrap()
        .expect("an unverifiable snapshot must be replaced, not trusted");
    logby::backup::verify(&made).await.expect("the replacement is sound");
    assert!(good.exists(), "an earlier snapshot is never touched by a failure");
}

#[tokio::test]
async fn retention_keeps_the_newest_fourteen() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // Twenty days of history, oldest first.
    for day in 1..=20 {
        std::fs::write(backups.join(format!("logby-2020-01-{day:02}.db")), b"old").unwrap();
    }
    // Something that is not a snapshot must survive untouched.
    std::fs::write(backups.join("notes.txt"), b"keep me").unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    logby::backup::tick(&app.state, 1).await.unwrap().expect("today's snapshot");

    let mut names: Vec<String> = std::fs::read_dir(&backups).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("logby-"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 14, "fourteen kept, the rest pruned: {names:?}");
    assert_eq!(names[0], "logby-2020-01-08.db", "the oldest survivors are the newest of the old");
    assert!(names.last().unwrap().contains(&logby::db::today()), "today's is kept");
    assert!(backups.join("notes.txt").exists(), "unrelated files are not ours to delete");
}
