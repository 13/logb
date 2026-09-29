//! The backup status's idea of which day a snapshot belongs to, in the instance's own timezone.
//!
//! Moved out of `tests/it/database_api.rs` because it sets the instance timezone to
//! `Asia/Tokyo`, and that setting is process-wide.

use crate::common;

/// The whole point of `last_at` is to say whether this morning's snapshot actually landed -- so
/// it has to name the snapshot's day in the instance's own timezone, not miss it because a
/// same-named file that is not one of `tick`'s sits in the same directory.
///
/// The instance runs on `Asia/Tokyo` and the snapshot's mtime is pinned to 18:00 UTC today --
/// 03:00 tomorrow in Tokyo -- the exact shape of the case finding 1 was verified against
/// (`LOGB_TIMEZONE=Asia/Tokyo`, mtime `2026-09-11T18:00:00Z`, reported as if it were still the
/// 11th). The expected day is computed the same way `today()` computes "today" -- from
/// whichever timezone actually won the process-wide `db::set_timezone` race, so this stays
/// correct even run alongside the other tests in this binary that change it -- but a
/// solitary run of just this test, where `Asia/Tokyo` is the only value ever offered, is what
/// pins finding 1: see the mutation evidence in the report.
#[tokio::test]
async fn last_at_names_the_snapshots_local_day_and_ignores_a_lookalike() {
    let dir = tempfile::tempdir().unwrap();
    let app = common::spawn_with(|c| {
        c.backup_dir = Some(dir.path().to_path_buf());
        c.backup_hour = 3;
        c.timezone = Some(chrono_tz::Tz::Asia__Tokyo);
    })
    .await;
    if common::skipped_on_postgres(
        "last_at_names_the_snapshots_local_day_and_ignores_a_lookalike",
        "automatic backup is a SQLite mechanism",
    ) {
        return;
    }
    app.setup("ben", "correct horse").await;

    let mtime = chrono::Utc::now().date_naive().and_hms_opt(18, 0, 0).unwrap().and_utc();
    let expected = mtime.with_timezone(&logb::db::timezone()).date_naive().to_string();

    let snapshot = dir.path().join(format!("logb-{}.db", expected));
    std::fs::write(&snapshot, b"snapshot").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&snapshot)
        .unwrap()
        .set_modified(mtime.into())
        .unwrap();
    // Not one of tick's: lacks the "logb-" prefix. If this were what last_at reported, its
    // mtime -- not the snapshot's -- would decide the answer.
    std::fs::write(dir.path().join("other-2020-01-01.db"), b"decoy").unwrap();

    let body: serde_json::Value = app.get_json("/database/backup").await;
    let last_at = body["last_at"].as_str().expect("last_at should name the snapshot just written");
    assert_eq!(
        &last_at[..10],
        expected,
        "last_at should carry the snapshot's day in the instance's own timezone, not whatever \
         day its UTC mtime alone would name: {body}"
    );
}
