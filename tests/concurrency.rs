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

/// The cursor's promise, from `0007_sync.sql`: monotonic and in commit order. A device pulls
/// with `seq > cursor` and remembers the highest it saw, so a number that appears after the
/// device has moved past it is not late -- it is gone, for that device, permanently and with
/// nothing reporting an error.
///
/// What turns this red on PostgreSQL is the advisory lock in `db::begin_write`: point these
/// writers at `state.db.begin()` instead, or drop `Backend::write_lock`'s PostgreSQL arm, and
/// the writers commit out of order while the puller walks past numbers that have not landed --
/// observed at 148, 154 and 158 of 161 changes reaching the puller. `seq` is not gapless on
/// either backend -- a rolled-back insert burns its number here the same as on SQLite -- and
/// this test does not measure that; it only measures that every number that does land, lands
/// in order.
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

/// A client that retries a push it never saw the answer to sends the same op ids again. Two
/// such attempts can arrive at once -- a flaky connection retrying while the first is still in
/// flight. Both must be answered `accepted`, and the write must happen once.
///
/// Before the fix this raced: idempotency was SELECT-then-INSERT, so both attempts read "not
/// seen", both inserted, and the second hit `idx_changes_user_op` and turned the whole batch
/// into a 500 -- a client that retries forever, forever.
///
/// What to remove to see it red, because this one takes two removals rather than one: the
/// advisory lock already serialises the two pushes, so with `Backend::write_lock`'s PostgreSQL
/// arm in place the second attempt always finds the first's row however the lookup is spelled,
/// and this passes on either version of `api::sync::push`. Drop that arm and the two pushes
/// genuinely overlap -- then the old SELECT-then-INSERT answers the second attempt 500
/// (observed, on the first run, with two attempts; no need to add more), and the `ON CONFLICT
/// (user_id, client_op_id) DO NOTHING` claim that replaced it answers 200 twice with one row
/// logged. That pairing is the point: the lock and the index protect this independently, and
/// idempotency should not be resting on a lock taken for a different reason.
///
/// The count is scoped to this op id rather than the whole log (`count_changes`) because the
/// REST create above logs a change of its own; the question here is how many times the pushed
/// op landed.
#[tokio::test]
async fn a_push_retried_concurrently_is_applied_once_and_accepted_twice() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    let ops = app.one_set_op(&object, "Golf VII", "op-retry-1").await;

    let (a, b) = tokio::join!(app.push_raw(&ops), app.push_raw(&ops));
    for (which, res) in [("first", a), ("second", b)] {
        assert_eq!(res.status(), 200, "the {which} attempt did not answer 200");
    }
    assert_eq!(app.count_changes_of("op-retry-1").await, 1, "the op was recorded more than once");
}

/// The purge deletes aged-out tombstones under guards that say "only if this parent has no
/// children left". Each guard rides in the same `DELETE` as the delete it guards, but the four
/// deletes were four separate autocommit statements against the pool, taking no transaction and
/// no lock -- and on PostgreSQL a `NOT EXISTS` subquery answers from the snapshot its statement
/// began with. A child that commits while that statement is parked on the parent's row lock is
/// therefore invisible to the guard and still taken by `ON DELETE CASCADE`: hard-deleted, with
/// no tombstone and no `changes` row, so no device ever learns it existed. That is the exact
/// loss `sync::feed::purge`'s own comment says the guards exist to prevent.
///
/// The writer is the insert `api::activities::create` runs, in the `db::begin_write`
/// transaction it runs it in, rather than a REST call: the interesting create is one that
/// passed `load_owned_object` while the object was still live -- that check reads the pool
/// before the transaction opens -- and only commits after the delete has landed. Holding that
/// transaction open across the purge turns a window measured in microseconds into one a test
/// can stand in, instead of one it has to hope to land in.
///
/// Two removals turn it red on PostgreSQL, and which one it is matters, because unlike the
/// three tasks before it this race was NOT already prevented by Task 1's advisory lock. The
/// lock was in place throughout and the test still failed: the purge ran its deletes in
/// autocommit, so it took no lock at all and `Backend::write_lock` had nothing to say about
/// them. Remove the `db::begin_write` around the guard loop in `sync::feed::purge` and it goes
/// red again for that reason. Keep the transaction and remove `Backend::write_lock`'s
/// PostgreSQL arm instead, and it ALSO goes red -- a READ COMMITTED transaction is no defence
/// on its own, since every statement in it takes a fresh snapshot and none of them blocks the
/// writer. So the two are one mechanism here rather than two independent ones: the transaction
/// is what makes the purge take the lock, and the lock is what the transaction protects it
/// with. Both were observed, in that order.
///
/// Observed before the fix, on PostgreSQL: the activity row gone, no tombstone, nothing in
/// `changes`. On SQLite it passed throughout -- the writer's `BEGIN IMMEDIATE` holds the one
/// write lock the database has, so the purge's first statement waits for the commit and every
/// guard afterwards sees the child. The fix therefore changes nothing SQLite was relying on; it
/// gives PostgreSQL the serialisation SQLite had all along.
#[tokio::test]
async fn a_child_created_during_a_purge_is_not_cascade_deleted() {
    let app = common::spawn().await;
    app.setup("ben", "correct horse").await;
    let object = app.create_object(&app.client, "Golf", Some("km")).await;
    let object_id = object["id"].as_i64().unwrap();
    app.delete_object(&object).await;
    app.age_out_tombstones().await;

    // A device that was offline creates an activity against the object. Open and insert, but
    // do not commit yet: this is the create that got past the liveness check and is still in
    // flight when the purge starts.
    const UUID: &str = "late-arrival-uuid";
    let mut writer = logb::db::begin_write(&app.state.db, app.state.backend).await.unwrap();
    sqlx::query(
        "INSERT INTO activities \
           (object_id, date, category, title, notes, created_at, updated_at, client_uuid) \
         VALUES ($1, '2024-03-01', 'maintenance', 'late arrival', '', $2, $3, $4)",
    )
    .bind(object_id)
    .bind(logb::db::now())
    .bind(logb::db::now())
    .bind(UUID)
    .execute(&mut *writer)
    .await
    .unwrap();

    let purge = {
        let state = app.state.clone();
        tokio::spawn(async move { logb::sync::feed::purge(&state, 90).await })
    };
    // Long enough for the purge to have reached the delete that matters and be waiting on the
    // row this transaction holds; the commit is what lets it through.
    tokio::time::sleep(Duration::from_millis(500)).await;
    writer.commit().await.unwrap();
    purge.await.unwrap().expect("the purge itself must not fail");

    // Either the object survived with its child, or the child is a tombstone, or a device can
    // still learn of its delete from the log. What must never happen is a row vanishing with
    // none of the three -- which is what a cascade over a live child leaves behind.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM activities WHERE client_uuid = $1")
        .bind(UUID)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM changes WHERE entity_uuid = $1 AND op = 'delete'")
            .bind(UUID)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(
        rows == 1 || logged >= 1,
        "the activity is gone with no tombstone and no delete in the log: no device can ever \
         learn it existed"
    );
    // The other shape the same cascade could take, and the weaker of the two: a child that
    // outlived its object rather than dying with it.
    assert_eq!(app.count_orphan_activities().await, 0, "an activity outlived its object");
}
