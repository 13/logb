//! The harness's own two helpers, tested here rather than in `tests/common/mod.rs`.
//!
//! `common` is included by every integration binary, so a `#[cfg(test)] mod tests` inside it
//! would be compiled and run eighteen times over. These live in one binary instead, and are
//! the only tests in the suite whose subject is the harness rather than the app.

mod common;

use common::{replace_database_in_url, unique_suffix};

/// The whole job: keep user, password, host and port, replace the database name.
#[test]
fn the_database_name_is_swapped_and_nothing_else_is() {
    assert_eq!(
        replace_database_in_url("postgres://user:pw@host:5432/postgres", "logb_test_1234_7"),
        "postgres://user:pw@host:5432/logb_test_1234_7"
    );
    // A server URL naming no database at all is the other shape an operator might hand over.
    assert_eq!(
        replace_database_in_url("postgres://user:pw@host:5432", "logb_test_1_0"),
        "postgres://user:pw@host:5432/logb_test_1_0"
    );
    // Connection options live in the query string and have to survive: dropping `sslmode`
    // would change how the test connects, not just where.
    assert_eq!(
        replace_database_in_url("postgres://host/postgres?sslmode=require", "logb_test_1_0"),
        "postgres://host/logb_test_1_0?sslmode=require"
    );
}

/// Uniqueness within a run is the only property that matters -- two tests in one binary must
/// never be handed the same database, and the process id keeps two binaries apart.
#[test]
fn every_suffix_in_a_process_is_different_and_carries_the_process_id() {
    let pid = std::process::id().to_string();
    let names: std::collections::BTreeSet<String> = (0..100).map(|_| unique_suffix()).collect();
    assert_eq!(names.len(), 100, "a suffix was handed out twice: {names:?}");
    assert!(names.iter().all(|n| n.starts_with(&format!("{pid}_"))), "{names:?}");
}
