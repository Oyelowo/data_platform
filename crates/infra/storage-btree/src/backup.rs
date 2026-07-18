//! Online backup for the B+ tree engine.
//!
//! A backup is a crash-consistent point-in-time copy of all on-disk files.
//! The implementation pauses background I/O threads, issues a final sync, and
//! copies files using plain `std::fs::copy`.  The resulting directory is a
//! valid, independently openable database.

use std::path::Path;

use crate::error::{Error, Result};
use crate::io::{RealBackend, StorageBackend};

/// List of file/directory names that must be copied for a complete backup.
const BACKUP_ENTRIES: &[&str] = &["pages.dat", "values.log", "META", "META.bak", "wal"];

/// Copy all database files from `src_dir` to `dst_dir`, creating `dst_dir` if
/// necessary.
///
/// The caller is responsible for ensuring that no background writer is active
/// during the copy; `BtreeEngine::backup` pauses the checkpoint and cleaner
/// threads around this call.
pub fn copy_database_files(src_dir: &Path, dst_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dst_dir)?;

    for entry in BACKUP_ENTRIES {
        let src = src_dir.join(entry);
        let dst = dst_dir.join(entry);
        if !RealBackend.exists(&src) {
            continue;
        }
        let meta = std::fs::metadata(&src)?;
        if meta.is_dir() {
            copy_dir_recursively(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }

    Ok(())
}

fn copy_dir_recursively(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_dir_recursively(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Validate that `path` looks like a complete database directory by checking
/// for the required files.
pub fn validate_backup(path: &Path) -> Result<()> {
    let required = ["pages.dat", "values.log", "META"];
    for name in &required {
        if !path.join(name).exists() {
            return Err(Error::Corruption(format!(
                "backup at {} is missing {}",
                path.display(),
                name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_validate_roundtrip() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        std::fs::write(src.path().join("pages.dat"), b"pages").unwrap();
        std::fs::write(src.path().join("values.log"), b"values").unwrap();
        std::fs::write(src.path().join("META"), b"meta").unwrap();

        copy_database_files(src.path(), dst.path()).unwrap();
        validate_backup(dst.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst.path().join("pages.dat")).unwrap(),
            "pages"
        );
    }

    #[test]
    fn validate_rejects_incomplete_backup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pages.dat"), b"x").unwrap();
        assert!(validate_backup(dir.path()).is_err());
    }
}
