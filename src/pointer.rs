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

#[cfg(test)]
mod tests {
    use super::*;

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
