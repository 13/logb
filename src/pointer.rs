//! Where LogB remembers which database to open, when that choice was made from Settings
//! rather than the environment. It cannot live in the database, because it points away from
//! it: the file has to exist before there is any database to read it from.
//!
//! The file holds a connection URL in plaintext -- including a password, for PostgreSQL --
//! so it is created readable only by its owner (mode 0600 on Unix). That is the real cost of
//! moving this choice into a web form instead of leaving it to the environment.

use crate::db::BoxError;
use rand::RngExt;
use std::path::{Path, PathBuf};

/// The pointer file inside a data directory: the URL and nothing else.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("database.url")
}

/// The stored URL, or `None` if there is no pointer file, it is empty, or it cannot be read.
pub fn read(data_dir: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path(data_dir)).ok()?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Write the pointer file atomically: a temporary file beside it, permissioned 0600, then
/// renamed into place. A half-written pointer is a database that will not open, and that
/// failure would otherwise arrive at the worst possible moment -- the next restart.
pub fn write(data_dir: &Path, url: &str) -> Result<(), BoxError> {
    std::fs::create_dir_all(data_dir)?;
    let dest = path(data_dir);
    let mut token = [0u8; 16];
    rand::rng().fill(&mut token);
    let tmp = data_dir.join(format!(".database.url.{}.tmp", hex::encode(token)));

    std::fs::write(&tmp, url)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }

    match std::fs::rename(&tmp, &dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}

/// Whether a pointer file could be written here, asked by writing one and taking it away again.
///
/// The question is asked before a copy rather than after it, so an operator finds out that the
/// data directory is read-only *before* a migration runs rather than while looking at a
/// finished copy the instance will never open. A probe is the only honest way to ask it: the
/// mode bits on a directory say nothing about a read-only mount, a full disk, or the container
/// that dropped this process's write access to a volume.
pub fn writable(data_dir: &Path) -> bool {
    if std::fs::create_dir_all(data_dir).is_err() {
        return false;
    }
    let mut token = [0u8; 16];
    rand::rng().fill(&mut token);
    // Same shape as the temporary file `write` uses, so a probe left behind by a process killed
    // between these two lines is recognisable as this module's and is not the pointer itself.
    let probe = data_dir.join(format!(".database.url.{}.probe", hex::encode(token)));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_that_takes_a_file_is_writable_and_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(writable(dir.path()));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "the probe was left behind");
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_written_says_so() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).unwrap();
        let answer = writable(&data);
        // Restored so the temporary directory can be cleaned up either way.
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Root ignores the mode bits, so this can only be asserted where the test is not root.
        if effective_uid() != 0 {
            assert!(!answer);
        }
    }

    /// The effective uid, to know whether the mode bits above apply to this test at all.
    ///
    /// `std` exposes no `geteuid` and a dependency for one number is not worth it: on Linux it
    /// is the second field of `/proc/self/status`' `Uid:` line. Anything that cannot be read
    /// counts as "not root", which only makes the test stricter.
    #[cfg(unix)]
    fn effective_uid() -> u32 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("Uid:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|uid| uid.parse().ok())
            })
            .unwrap_or(1)
    }

    #[test]
    fn no_file_means_no_pointer() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn a_blank_file_means_no_pointer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), "   \n").unwrap();
        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "postgres://user:pass@host/db").unwrap();
        assert_eq!(read(dir.path()).as_deref(), Some("postgres://user:pass@host/db"));
    }

    #[test]
    fn writing_twice_leaves_only_the_new_value() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "postgres://first/db").unwrap();
        write(dir.path(), "postgres://second/db").unwrap();
        assert_eq!(read(dir.path()).as_deref(), Some("postgres://second/db"));
        // No leftover temporary files.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary file left behind: {leftovers:?}");
    }
}
