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
