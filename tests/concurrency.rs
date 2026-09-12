//! The races that SQLite's single writer hid.
//!
//! Every test here must be able to fail: each one is paired with a note saying what to remove
//! to see it red, and the task that added it proved it. A concurrency test that has never been
//! observed failing is decoration.

mod common;

use std::time::Duration;

/// Two write transactions must not overlap. On SQLite `BEGIN IMMEDIATE` already guarantees it;
/// on PostgreSQL the advisory lock does. Remove `Backend::write_lock`'s PostgreSQL arm and this
/// fails on PostgreSQL while still passing on SQLite -- which is the whole point of it.
#[tokio::test]
async fn two_write_transactions_do_not_overlap() {
    let app = common::spawn().await;
    let a = logb::db::begin_write(&app.state.db, app.state.backend).await.unwrap();

    // The second one must not be able to start while the first is open.
    let blocked = tokio::time::timeout(
        Duration::from_millis(750),
        logb::db::begin_write(&app.state.db, app.state.backend),
    )
    .await;
    assert!(blocked.is_err(), "a second write transaction began while the first was still open");

    drop(a);
    // And it must proceed once the first is done, rather than deadlocking forever.
    let after = tokio::time::timeout(
        Duration::from_secs(5),
        logb::db::begin_write(&app.state.db, app.state.backend),
    )
    .await;
    assert!(after.is_ok(), "the lock was not released when the transaction ended");
}

/// The cursor's promise, from `0007_sync.sql`: "monotonic, gapless per database, and ordered".
/// A device pulls with `seq > cursor` and remembers the highest it saw, so a number that
/// appears after the device has moved past it is not late -- it is gone, for that device,
/// permanently and with nothing reporting an error.
///
/// What turns this red on PostgreSQL is the advisory lock, not the `MAX(seq) + 1` assignment:
/// point these writers at `state.db.begin()` instead of `db::begin_write`, or drop
/// `Backend::write_lock`'s PostgreSQL arm, and the writers commit out of order while the puller
/// walks past numbers that have not landed -- observed at 148, 154 and 158 of 161 changes
/// reaching the puller. Removing `MAX(seq) + 1` on its own leaves it green; that assignment
/// buys gaplessness across a rollback, which is not what this test measures.
///
/// It stays green on SQLite throughout, which is what says the test is about the difference
/// between the two databases rather than about itself.
///
/// Sixteen writers against a pool of sixteen, on a multi-threaded runtime, with the puller
/// polling every millisecond: eight writers against the harness's default pool of two never
/// once failed, because two concurrent transactions leave a window too narrow to read into.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_puller_sees_every_change_exactly_once_under_concurrent_writers() {
    let app = std::sync::Arc::new(common::spawn_with(|c| c.db_pool_size = Some(16)).await);
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writers: Vec<_> = (0..16)
        .map(|i| {
            let app = app.clone();
            let object = object["id"].clone();
            tokio::spawn(async move {
                for n in 0..10 {
                    app.create_activity(&object, &format!("entry {i}-{n}")).await;
                }
            })
        })
        .collect();

    let puller = {
        let app = app.clone();
        let done = done.clone();
        tokio::spawn(async move {
            let mut cursor = 0i64;
            let mut seen = Vec::new();
            let mut drain = 0;
            loop {
                let page = app.pull(cursor).await;
                for change in page["changes"].as_array().unwrap() {
                    let seq = change["seq"].as_i64().unwrap();
                    seen.push(seq);
                    cursor = cursor.max(seq);
                }
                if done.load(std::sync::atomic::Ordering::SeqCst) {
                    drain += 1;
                    if drain > 3 {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            seen
        })
    };

    for w in writers {
        w.await.unwrap();
    }
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let seen = puller.await.unwrap();

    let total: i64 = app.count_changes().await;
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(seen, sorted, "the puller saw a seq out of order or twice: {seen:?}");
    assert_eq!(
        sorted.len() as i64,
        total,
        "the puller saw {} of {total} changes -- one was skipped permanently",
        sorted.len(),
    );
}
