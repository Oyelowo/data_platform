//! Deferred deletion of obsolete SSTable files.
//!
//! Compaction input files must not be physically deleted until:
//!
//! 1. The `VersionEdit` that removes them is logged and synced to the MANIFEST.
//! 2. The `VersionSet` has applied the edit so the current `Version` no longer
//!    references them.
//! 3. No in-flight reader still holds an older `Arc<Version>` that references
//!    them.
//!
//! This module tracks file numbers awaiting deletion and provides a cleanup
//! routine that deletes files whose numbers are not referenced by any live
//! `Version`.

use std::collections::HashSet;
use std::path::Path;

use crate::immutable::sstable_path;
use crate::version_set::VersionSet;
use crate::{FileNumber, Result};

/// Set of SSTable file numbers waiting to be deleted.
#[derive(Debug, Default)]
pub struct ObsoleteFiles {
    pending: HashSet<FileNumber>,
}

impl ObsoleteFiles {
    /// Create an empty obsolete-file tracker.
    pub fn new() -> Self {
        Self {
            pending: HashSet::new(),
        }
    }

    /// Mark a file number as obsolete.
    ///
    /// The file will be deleted once it is no longer referenced by any live
    /// `Version`.
    #[allow(dead_code)]
    pub fn mark_obsolete(&mut self, number: FileNumber) {
        self.pending.insert(number);
    }

    /// Mark many file numbers as obsolete.
    pub fn mark_obsolete_many(&mut self, numbers: impl IntoIterator<Item = FileNumber>) {
        for number in numbers {
            self.pending.insert(number);
        }
    }

    /// Delete pending files that are not referenced by any live `Version`.
    ///
    /// Files that are still referenced remain in the pending set and will be
    /// re-checked on the next call.
    pub fn delete_unreferenced(&mut self, db_path: &Path, version_set: &VersionSet) -> Result<()> {
        let live = version_set.live_file_numbers();
        let to_delete: Vec<FileNumber> = self
            .pending
            .iter()
            .copied()
            .filter(|n| !live.contains(n))
            .collect();

        for number in to_delete {
            let path = sstable_path(db_path, number);
            // Failure to remove a file is not fatal: it will be re-checked and
            // cleaned up on the next pass (or on recovery).
            let _ = std::fs::remove_file(&path);
            self.pending.remove(&number);
        }

        Ok(())
    }

    /// Scan the database directory and delete every SSTable not referenced by a
    /// live `Version`.
    ///
    /// This is used during recovery to remove files left behind by a crashed
    /// compaction or an interrupted flush.
    pub fn delete_unreferenced_files_on_disk(
        &mut self,
        db_path: &Path,
        version_set: &VersionSet,
    ) -> Result<()> {
        self.delete_unreferenced_files_on_disk_with_live(db_path, version_set.live_file_numbers())
    }

    /// Like `delete_unreferenced_files_on_disk` but with an explicit live set.
    ///
    /// This is used during recovery when live files must be computed across all
    /// column families rather than a single `VersionSet`.
    pub fn delete_unreferenced_files_on_disk_with_live(
        &mut self,
        db_path: &Path,
        live: HashSet<FileNumber>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(db_path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.ends_with(".sst") {
                continue;
            }
            let stem = name.strip_suffix(".sst").unwrap();
            if let Ok(number) = stem.parse::<FileNumber>()
                && !live.contains(&number)
            {
                let _ = std::fs::remove_file(entry.path());
                self.pending.remove(&number);
            }
        }

        // Keep only pending numbers that are still unreferenced; anything that
        // is live again (e.g. reused after recovery) should not be deleted.
        self.pending.retain(|n| !live.contains(n));

        Ok(())
    }

    /// Iterate over file numbers currently awaiting deletion.
    pub fn pending(&self) -> impl Iterator<Item = FileNumber> + '_ {
        self.pending.iter().copied()
    }

    /// Remove a file number from the pending set, e.g. after deleting it.
    pub fn remove(&mut self, number: FileNumber) {
        self.pending.remove(&number);
    }

    /// Number of files currently awaiting deletion.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_key::{ValueType, build_internal_key};
    use crate::version::FileMetaData;
    use tempfile::TempDir;

    fn ikey(key: &[u8], seq: u64) -> Vec<u8> {
        build_internal_key(key, seq, ValueType::Value)
    }

    fn sample_meta(number: u64) -> FileMetaData {
        FileMetaData {
            number,
            file_size: 1,
            smallest: ikey(&[0], 1),
            largest: ikey(&[9], 1),
        }
    }

    #[test]
    fn deletes_only_unreferenced_files() {
        let dir = TempDir::new().unwrap();
        let version_set = VersionSet::new(2);

        // File 1 is live, file 2 is obsolete.
        let mut edit = crate::version_set::VersionEdit::default();
        edit.new_files.push((0, sample_meta(1)));
        version_set.apply(edit).unwrap();

        std::fs::write(sstable_path(dir.path(), 1), b"live").unwrap();
        std::fs::write(sstable_path(dir.path(), 2), b"obsolete").unwrap();

        let mut obsolete = ObsoleteFiles::new();
        obsolete.mark_obsolete(2);
        assert_eq!(obsolete.pending_count(), 1);

        obsolete.delete_unreferenced(dir.path(), &version_set).unwrap();
        assert!(sstable_path(dir.path(), 1).exists());
        assert!(!sstable_path(dir.path(), 2).exists());
        assert_eq!(obsolete.pending_count(), 0);
    }

    #[test]
    fn keeps_file_while_retired_version_references_it() {
        let dir = TempDir::new().unwrap();
        let version_set = VersionSet::new(2);

        // File 1 starts live, then a reader grabs the version before we
        // compact it away.
        let mut edit = crate::version_set::VersionEdit::default();
        edit.new_files.push((0, sample_meta(1)));
        version_set.apply(edit).unwrap();
        let _held_version = version_set.current();

        std::fs::write(sstable_path(dir.path(), 1), b"data").unwrap();

        // A second edit removes file 1 and retires the version the reader
        // still holds.
        let mut edit2 = crate::version_set::VersionEdit::default();
        edit2.deleted_files.push((0, 1));
        version_set.apply(edit2).unwrap();

        let mut obsolete = ObsoleteFiles::new();
        obsolete.mark_obsolete(1);

        // The held version still references file 1, so it must not be deleted.
        obsolete.delete_unreferenced(dir.path(), &version_set).unwrap();
        assert!(sstable_path(dir.path(), 1).exists());
        assert_eq!(obsolete.pending_count(), 1);

        // After dropping the held version the next cleanup pass deletes it.
        drop(_held_version);
        obsolete.delete_unreferenced(dir.path(), &version_set).unwrap();
        assert!(!sstable_path(dir.path(), 1).exists());
        assert_eq!(obsolete.pending_count(), 0);
    }

    #[test]
    fn recovery_deletes_unreferenced_disk_files() {
        let dir = TempDir::new().unwrap();
        let version_set = VersionSet::new(2);

        let mut edit = crate::version_set::VersionEdit::default();
        edit.new_files.push((0, sample_meta(1)));
        version_set.apply(edit).unwrap();

        std::fs::write(sstable_path(dir.path(), 1), b"live").unwrap();
        std::fs::write(sstable_path(dir.path(), 2), b"orphan").unwrap();
        // Non-SSTable files must not be touched.
        std::fs::write(dir.path().join("wal"), b"wal").unwrap();

        let mut obsolete = ObsoleteFiles::new();
        obsolete
            .delete_unreferenced_files_on_disk(dir.path(), &version_set)
            .unwrap();

        assert!(sstable_path(dir.path(), 1).exists());
        assert!(!sstable_path(dir.path(), 2).exists());
        assert!(dir.path().join("wal").exists());
    }
}
