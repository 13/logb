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

    assert!(logb::backup::tick(&app.state, 2).await.unwrap().is_none(), "too early in the day");

    let made = logb::backup::tick(&app.state, 3).await.unwrap().expect("a snapshot was due");
    assert!(made.exists());
    assert_eq!(made.file_name().unwrap(), format!("logb-{}.db", logb::db::today()).as_str());
    logb::backup::verify(&made).await.expect("the snapshot opens and passes integrity_check");

    // The snapshot is the real database, not an empty file that happens to be valid SQLite.
    let pool = logb::db::connect_existing(&logb::db::sqlite_url(made.parent().unwrap()).unwrap()).await;
    assert!(pool.is_err(), "connect_existing looks for logb.db, not a dated snapshot");
    let count: i64 = {
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&made).read_only(true);
        let p = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query_scalar("SELECT count(*) FROM objects").fetch_one(&p).await.unwrap()
    };
    assert_eq!(count, 1, "the object is in the snapshot");

    assert!(logb::backup::tick(&app.state, 3).await.unwrap().is_none(), "already done today");
}

#[tokio::test]
async fn backup_is_off_unless_a_directory_is_configured() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    assert!(logb::backup::tick(&app.state, 23).await.unwrap().is_none());
}

#[tokio::test]
async fn a_corrupt_snapshot_is_rejected_and_the_previous_one_survives() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    let good = backups.join("logb-2020-01-01.db");
    std::fs::write(&good, b"pretend this is yesterday's good snapshot").unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;

    // Today's slot already holds a file that is not a database at all.
    let today = backups.join(format!("logb-{}.db", logb::db::today()));
    std::fs::write(&today, b"not a database").unwrap();

    let made = logb::backup::tick(&app.state, 1).await.unwrap()
        .expect("an unverifiable snapshot must be replaced, not trusted");
    logb::backup::verify(&made).await.expect("the replacement is sound");
    assert!(good.exists(), "an earlier snapshot is never touched by a failure");
}

#[tokio::test]
async fn a_zero_length_file_in_todays_slot_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;

    // A zero-length file lands in today's slot -- a stray `touch`, an interrupted copy, or a
    // `backup_to` that failed partway and (before this fix) left its partial file behind.
    // SQLite calls a zero-length file a valid, schema-less database, so a naive verify would
    // wave it through and the day would silently go without a backup.
    let today = backups.join(format!("logb-{}.db", logb::db::today()));
    std::fs::write(&today, b"").unwrap();

    let made = logb::backup::tick(&app.state, 1).await.unwrap()
        .expect("an empty file must be replaced, not accepted as an already-done backup");
    logb::backup::verify(&made).await.expect("the replacement is sound");
    assert!(std::fs::metadata(&made).unwrap().len() > 0, "the slot no longer holds an empty file");
}

#[tokio::test]
async fn a_prune_failure_does_not_mask_a_successful_backup() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // Enough dated snapshots to force pruning, with one slot occupied by a directory instead
    // of a file -- `remove_file` on it fails with "Is a directory".
    for day in 1..=20 {
        if day == 5 {
            std::fs::create_dir(backups.join(format!("logb-2020-01-{day:02}.db"))).unwrap();
        } else {
            std::fs::write(backups.join(format!("logb-2020-01-{day:02}.db")), b"old").unwrap();
        }
    }

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;

    let made = logb::backup::tick(&app.state, 1).await.unwrap()
        .expect("a prune problem must not be reported as a backup failure");
    logb::backup::verify(&made).await.expect("the new snapshot itself is sound");

    assert!(
        !backups.join("logb-2020-01-01.db").exists(),
        "the oldest real snapshot is still pruned: one bad entry does not stop the others"
    );
    assert!(
        backups.join("logb-2020-01-05.db").is_dir(),
        "the undeletable directory is left alone, not silently vanished, and does not panic the run"
    );
}

#[tokio::test]
async fn prune_matches_by_name_only_not_by_being_a_real_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // Fourteen real dated snapshots -- already at capacity before anything else is added.
    for day in 1..=14 {
        std::fs::write(backups.join(format!("logb-2020-01-{day:02}.db")), b"old").unwrap();
    }
    // Not ours: the filter requires the ".db" suffix, and this file does not have it.
    let not_ours_suffix = backups.join("logb-2020-01-01.db.bak");
    std::fs::write(&not_ours_suffix, b"decoy").unwrap();
    // Not ours: ends in ".db" but lacks the "logb-" prefix -- filter must reject this.
    let not_ours_prefix = backups.join("other-2020-01-01.db");
    std::fs::write(&not_ours_prefix, b"decoy").unwrap();
    // Ours by name alone, though it was never a dated snapshot -- the filter has no
    // provenance tracking, only a name pattern, so it takes a retention slot like any other.
    let impostor = backups.join("logb-x.db");
    std::fs::write(&impostor, b"decoy").unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    logb::backup::tick(&app.state, 1).await.unwrap().expect("today's snapshot");

    assert!(not_ours_suffix.exists(), "a name the filter does not match (wrong suffix) is never ours to delete");
    assert!(not_ours_prefix.exists(), "a name the filter does not match (missing prefix) is never ours to delete");

    let mut names: Vec<String> = std::fs::read_dir(&backups).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("logb-") && n.ends_with(".db"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 14, "the filter still caps at fourteen: {names:?}");
    assert!(
        names.contains(&"logb-x.db".to_string()),
        "a non-dated name matching the pattern is treated as ours and takes a retention slot: {names:?}"
    );
    assert!(
        !names.contains(&"logb-2020-01-01.db".to_string()),
        "the impostor's late sort position displaced the oldest real snapshot: {names:?}"
    );
}

#[tokio::test]
async fn retention_keeps_the_newest_fourteen() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // Twenty days of history, oldest first.
    for day in 1..=20 {
        std::fs::write(backups.join(format!("logb-2020-01-{day:02}.db")), b"old").unwrap();
    }
    // Something that is not a snapshot must survive untouched.
    std::fs::write(backups.join("notes.txt"), b"keep me").unwrap();

    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    logb::backup::tick(&app.state, 1).await.unwrap().expect("today's snapshot");

    let mut names: Vec<String> = std::fs::read_dir(&backups).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("logb-"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 14, "fourteen kept, the rest pruned: {names:?}");
    assert_eq!(names[0], "logb-2020-01-08.db", "the oldest survivors are the newest of the old");
    assert!(names.last().unwrap().contains(&logb::db::today()), "today's is kept");
    assert!(backups.join("notes.txt").exists(), "unrelated files are not ours to delete");
}

#[tokio::test]
async fn restore_brings_back_the_snapshot_and_changes_the_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;

    let snapshot = logb::backup::tick(&app.state, 1).await.unwrap().expect("a snapshot");
    let epoch_before = logb::sync::epoch::current(&app.state.db).await.unwrap();

    // The mistake we are recovering from.
    app.create_object(&app.client, "Regrettable", None).await;
    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM objects")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(live, 2);

    // The running instance holds the database open, so restore has to happen against a stopped
    // one. Point it at a data directory of its own, seeded from this instance's snapshot.
    let target = tempfile::tempdir().unwrap();
    std::fs::copy(&snapshot, target.path().join("logb.db")).unwrap();
    let report = logb::restore::run(target.path(), &snapshot).await.unwrap();

    assert!(report.replaced_to.is_some(), "the database it replaced is kept, not deleted");
    assert!(report.replaced_to.as_ref().unwrap().exists());
    assert_ne!(report.epoch, epoch_before, "a restored database is a different database");

    let pool = logb::db::connect_existing(&logb::db::sqlite_url(target.path()).unwrap()).await.unwrap();
    let restored: i64 = sqlx::query_scalar("SELECT count(*) FROM objects").fetch_one(&pool).await.unwrap();
    assert_eq!(restored, 1, "the regrettable object is not in the restored database");
    let name: String = sqlx::query_scalar("SELECT name FROM objects").fetch_one(&pool).await.unwrap();
    assert_eq!(name, "Golf");
}

/// A restore that died between its two renames -- the live file moved aside, its -wal/-shm not
/// yet moved -- leaves exactly this on disk: no `logb.db`, but sidecars sitting where the next
/// restore's copy will land. They must not survive to sit beside a database they do not belong
/// to; SQLite would read them as that database's journal.
#[tokio::test]
async fn restore_moves_aside_an_orphaned_wal_with_no_live_database() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;
    let snapshot = logb::backup::tick(&app.state, 1).await.unwrap().expect("a snapshot");

    let target = tempfile::tempdir().unwrap();
    let orphan_wal: &[u8] = b"wal belonging to some other database";
    let orphan_shm: &[u8] = b"shm belonging to some other database";
    std::fs::write(target.path().join("logb.db-wal"), orphan_wal).unwrap();
    std::fs::write(target.path().join("logb.db-shm"), orphan_shm).unwrap();

    logb::restore::run(target.path(), &snapshot).await.unwrap();

    // The exact live path must not hold the orphan. This alone is not proof of a fix: SQLite's
    // own close-time checkpoint clears a `-wal` beside a database it just opened regardless of
    // this module's code, so an unfixed restore can leave this path clean too while the orphan
    // was silently destroyed rather than preserved. The real assertion is the one below.
    assert!(
        !target.path().join("logb.db-wal").exists(),
        "an orphaned -wal must not end up beside the freshly restored database"
    );
    assert!(
        !target.path().join("logb.db-shm").exists(),
        "an orphaned -shm must not end up beside the freshly restored database"
    );

    // The orphan must have been moved aside -- kept, recoverable, unmodified -- before anything
    // touched `logb.db`, not silently clobbered by SQLite opening the new database.
    let moved: Vec<_> = std::fs::read_dir(target.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with("-wal") && n != "logb.db-wal")
        .collect();
    assert_eq!(moved.len(), 1, "the orphaned -wal should be moved aside, not lost: {moved:?}");
    assert_eq!(
        std::fs::read(target.path().join(&moved[0])).unwrap(),
        orphan_wal,
        "the moved-aside file must be the original orphan, byte for byte"
    );
    let shm_name = moved[0].replace("-wal", "-shm");
    assert_eq!(
        std::fs::read(target.path().join(&shm_name)).unwrap(),
        orphan_shm,
        "the -shm must travel with its -wal, moved aside as the same set"
    );
}

/// A snapshot carrying a migration version this binary does not recognise -- an older binary
/// restoring a newer backup -- makes `sqlx::migrate!` refuse with `VersionMissing`, *after* the
/// live database has already been swapped for the snapshot. `api::mod::health` already treats
/// exactly this situation (an ahead schema) as healthy, on the grounds that migrations are
/// additive, so `--restore` must reach the same conclusion and still rotate the epoch --
/// leaving the swap done but the epoch unrotated is the one failure mode the epoch exists to
/// prevent.
#[tokio::test]
async fn restore_of_an_ahead_schema_snapshot_still_succeeds_and_rotates_the_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(backups.clone());
        c.backup_hour = 0;
    }).await;
    app.setup("ben", "correct horse").await;
    app.create_object(&app.client, "Golf", Some("km")).await;
    let snapshot = logb::backup::tick(&app.state, 1).await.unwrap().expect("a snapshot");

    // Graft on a migration version no binary in this build ships -- standing in for a newer
    // release's backup being restored by an older one.
    {
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&snapshot).create_if_missing(false);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations \
             (version, description, installed_on, success, checksum, execution_time) \
             VALUES (?, 'from-the-future', CURRENT_TIMESTAMP, 1, ?, 0)")
            .bind(99_999_999_i64)
            .bind(vec![0u8; 32])
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
    let epoch_before: String = {
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&snapshot).read_only(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        let v = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'")
            .fetch_one(&pool).await.unwrap();
        pool.close().await;
        v
    };

    let target = tempfile::tempdir().unwrap();
    std::fs::copy(&snapshot, target.path().join("logb.db")).unwrap();

    let report = logb::restore::run(target.path(), &snapshot).await
        .expect("an ahead schema is additive and safe to use as-is, per the health check's own rule");
    assert_ne!(report.epoch, epoch_before, "the epoch must be rotated even on this path");

    let epoch_on_disk: String = {
        let pool = logb::db::connect_existing(&logb::db::sqlite_url(target.path()).unwrap()).await.unwrap();
        let v = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'sync_epoch'")
            .fetch_one(&pool).await.unwrap();
        pool.close().await;
        v
    };
    assert_eq!(epoch_on_disk, report.epoch, "the rotated epoch is actually on disk, not just reported");
}

#[tokio::test]
async fn restore_refuses_a_file_that_is_not_a_database() {
    let target = tempfile::tempdir().unwrap();
    let junk = target.path().join("not-a-snapshot.db");
    std::fs::write(&junk, b"absolutely not a database").unwrap();
    std::fs::write(target.path().join("logb.db"), b"the live one").unwrap();

    let err = logb::restore::run(target.path(), &junk).await.unwrap_err();
    assert!(err.to_string().contains("not a usable snapshot"), "got: {err}");
    assert_eq!(
        std::fs::read(target.path().join("logb.db")).unwrap(),
        b"the live one",
        "a refused restore must not have touched the live database"
    );
}
