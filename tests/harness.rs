//! The harness's own helpers, tested here rather than in `tests/common/mod.rs`.
//!
//! `common` is included by every integration binary, so a `#[cfg(test)] mod tests` inside it
//! would be compiled and run eighteen times over. These live in one binary instead, and are
//! the only tests in the suite whose subject is the harness rather than the app.

mod common;

use common::{pid_of, process_is_alive, replace_database_in_url, unique_suffix};

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

/// The sweep exists to collect databases a killed run left behind, and its whole risk is
/// collecting one a *live* run is between connections on. Reading the pid out of the name is
/// what keeps those apart: `DROP DATABASE` failing while someone is connected does not, because
/// a test that has closed its pool and is still running has nobody connected to it. That window
/// is the `login_logout_cycle` flake -- `3D000 database "logb_test_404882_2" does not exist`.
#[test]
fn a_scratch_name_says_which_process_made_it() {
    assert_eq!(pid_of("logb_test_404882_2"), Some(404_882));
    assert_eq!(pid_of("logb_test_1_0"), Some(1));
    // Anything this harness did not name is not this harness's to drop.
    assert_eq!(pid_of("logb_production"), None);
    assert_eq!(pid_of("logb_test_"), None);
    assert_eq!(pid_of("logb_test_notapid_0"), None);
}

/// The other half: a pid that is still running must read as alive, and one that is not must
/// not. A process this cannot ask about counts as alive, so an unanswerable case leaks a
/// database rather than deleting a running test's.
#[test]
fn only_a_process_that_is_gone_reads_as_gone() {
    assert!(process_is_alive(std::process::id()), "this very process must read as alive");

    // A child, waited for: its pid has been reaped, so nothing is running under it any more.
    let mut child = std::process::Command::new("/bin/sh").arg("-c").arg("exit 0").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert!(!process_is_alive(pid), "a reaped child ({pid}) must not read as alive");
}
