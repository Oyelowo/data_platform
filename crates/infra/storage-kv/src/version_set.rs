//! VersionSet: owns current Version and assigns file numbers.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, Weak};

use crate::file_number::FileNumberAllocator;
use crate::internal_key::compare_internal_keys;
use crate::version::{FileMetaData, Version};
use crate::{FileNumber, Result};

/// Shared ownership of the current Version and monotonic file numbers.
pub struct VersionSet {
    inner: Mutex<Inner>,
}

struct Inner {
    current: Arc<Version>,
    file_numbers: FileNumberAllocator,
    last_sequence: u64,
    /// For each level >= 1, the largest user key of the last compaction, used
    /// to rotate compactions through the key space.
    compaction_pointers: Vec<Option<Vec<u8>>>,
    /// Weak references to versions that were previously current but may still
    /// be held by in-flight readers.  These are used by the obsolete-file
    /// cleaner to avoid deleting SSTables that a reader still references.
    retired_versions: Vec<Weak<Version>>,
}

impl VersionSet {
    pub fn new(num_levels: usize) -> Self {
        Self::with_allocator(num_levels, FileNumberAllocator::default())
    }

    /// Create a `VersionSet` that draws file numbers from a shared allocator.
    pub fn with_allocator(num_levels: usize, file_numbers: FileNumberAllocator) -> Self {
        Self {
            inner: Mutex::new(Inner {
                current: Arc::new(Version::new(num_levels)),
                file_numbers,
                last_sequence: u64::MAX,
                compaction_pointers: vec![None; num_levels],
                retired_versions: Vec::new(),
            }),
        }
    }

    pub fn current(&self) -> Arc<Version> {
        self.inner.lock().unwrap().current.clone()
    }

    pub fn new_file_number(&self) -> FileNumber {
        self.inner.lock().unwrap().file_numbers.next()
    }

    /// Return the next file number that will be assigned (without consuming it).
    pub fn next_file_number(&self) -> FileNumber {
        self.inner.lock().unwrap().file_numbers.current()
    }

    /// Set the next file number. Used during recovery to ensure reused opens
    /// do not collide with existing SSTable files.
    pub fn set_next_file_number(&self, n: FileNumber) {
        self.inner.lock().unwrap().file_numbers.ensure_at_least(n);
    }

    /// Return the compaction rotation pointer for `level`, if any.
    pub fn compaction_pointer(&self, level: usize) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().compaction_pointers.get(level).cloned().flatten()
    }

    /// Store the largest user key of the most recent compaction at `level`.
    #[allow(dead_code)]
    pub fn set_compaction_pointer(&self, level: usize, key: Vec<u8>) {
        if let Some(ptr) = self.inner.lock().unwrap().compaction_pointers.get_mut(level) {
            *ptr = Some(key);
        }
    }

    #[allow(dead_code)]
    pub fn last_sequence(&self) -> u64 {
        self.inner.lock().unwrap().last_sequence
    }

    #[allow(dead_code)]
    pub fn set_last_sequence(&self, seq: u64) {
        self.inner.lock().unwrap().last_sequence = seq;
    }

    /// Apply a version edit atomically.
    ///
    /// The previous current `Version` is retired to a weak-reference list so
    /// that obsolete-file cleanup can wait for in-flight readers to drop it.
    pub fn apply(&self, edit: VersionEdit) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let num_levels = inner.current.levels.len();
        Self::validate_edit(num_levels, &edit)?;

        let mut new_version = (*inner.current).clone();
        for (level, number) in edit.deleted_files {
            new_version.levels[level].retain(|f| f.number != number);
        }
        for (level, meta) in edit.new_files {
            new_version.levels[level].push(meta);
        }
        // L0 files overlap and must remain in creation order (newest last) so
        // that reads search the most recent flush first.  File numbers are
        // reserved at freeze time, so number order *is* creation order; sort
        // explicitly because the background worker and the synchronous
        // backpressure flush can apply their edits out of order.  Levels 1+
        // are non-overlapping and sorted by smallest internal key.
        new_version.levels[0].sort_by_key(|f| f.number);
        for level in 1..new_version.levels.len() {
            new_version.levels[level]
                .sort_by(|a, b| compare_internal_keys(&a.smallest, &b.smallest));
        }

        // Retire the old current version before replacing it.  Holding the lock
        // here guarantees no reader can obtain a reference to the old version
        // between retirement and replacement.
        let old_current = std::mem::replace(&mut inner.current, Arc::new(new_version));
        inner.retired_versions.push(Arc::downgrade(&old_current));

        inner.file_numbers.ensure_at_least(edit.next_file_number);
        inner.last_sequence = inner.last_sequence.min(edit.last_sequence);
        Ok(())
    }

    /// Return the set of file numbers referenced by the current `Version`.
    pub fn current_file_numbers(&self) -> HashSet<FileNumber> {
        let inner = self.inner.lock().unwrap();
        inner
            .current
            .levels
            .iter()
            .flat_map(|level| level.iter().map(|f| f.number))
            .collect()
    }

    /// Return the set of file numbers referenced by any live `Version`.
    ///
    /// This includes the current version and any retired versions still held by
    /// in-flight readers.  Dead weak references are pruned during the scan.
    pub fn live_file_numbers(&self) -> HashSet<FileNumber> {
        let mut inner = self.inner.lock().unwrap();
        let mut live = HashSet::new();

        for level in &inner.current.levels {
            for file in level {
                live.insert(file.number);
            }
        }

        inner.retired_versions.retain(|weak| {
            if let Some(version) = weak.upgrade() {
                for level in &version.levels {
                    for file in level {
                        live.insert(file.number);
                    }
                }
                true
            } else {
                false
            }
        });

        live
    }

    fn validate_edit(num_levels: usize, edit: &VersionEdit) -> Result<()> {
        for (level, number) in &edit.deleted_files {
            if *level >= num_levels {
                return Err(crate::Error::Manifest(format!(
                    "deleted file references invalid level {level}"
                )));
            }
            if *number == 0 {
                return Err(crate::Error::Manifest(
                    "deleted file has invalid number 0".into(),
                ));
            }
        }
        for (level, meta) in &edit.new_files {
            if *level >= num_levels {
                return Err(crate::Error::Manifest(format!(
                    "new file references invalid level {level}"
                )));
            }
            if meta.number == 0 {
                return Err(crate::Error::Manifest(
                    "new file has invalid number 0".into(),
                ));
            }
            if meta.file_size == 0 {
                return Err(crate::Error::Manifest(format!(
                    "file {} has zero size",
                    meta.number
                )));
            }
            if meta.smallest.is_empty() || meta.largest.is_empty() {
                return Err(crate::Error::Manifest(format!(
                    "file {} has empty bounds",
                    meta.number
                )));
            }
            if compare_internal_keys(&meta.smallest, &meta.largest) == std::cmp::Ordering::Greater {
                return Err(crate::Error::Manifest(format!(
                    "file {} has smallest > largest",
                    meta.number
                )));
            }
        }
        Ok(())
    }
}

/// A delta describing a change to the VersionSet.
#[derive(Debug, Default, Clone)]
pub struct VersionEdit {
    pub cf_id: crate::column_family::ColumnFamilyId,
    pub deleted_files: Vec<(usize, FileNumber)>,
    pub new_files: Vec<(usize, FileMetaData)>,
    pub next_file_number: FileNumber,
    pub last_sequence: u64,
    /// Column families created by this edit (`(id, name)`).
    pub created_cfs: Vec<(crate::column_family::ColumnFamilyId, String)>,
    /// Column families dropped by this edit.
    pub dropped_cfs: Vec<crate::column_family::ColumnFamilyId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_key::{ValueType, build_internal_key};

    fn meta(number: FileNumber) -> FileMetaData {
        FileMetaData {
            number,
            file_size: 100,
            smallest: build_internal_key(b"a", 1, ValueType::Value),
            largest: build_internal_key(b"z", 1, ValueType::Value),
        }
    }

    /// L0 must stay sorted by file number even when flushers apply their
    /// edits out of order (background worker vs synchronous backpressure).
    #[test]
    fn l0_files_stay_sorted_by_number() {
        let vs = VersionSet::new(7);
        vs.apply(VersionEdit {
            new_files: vec![(0, meta(12))],
            next_file_number: 13,
            ..Default::default()
        })
        .unwrap();
        vs.apply(VersionEdit {
            new_files: vec![(0, meta(10)), (0, meta(11))],
            next_file_number: 13,
            ..Default::default()
        })
        .unwrap();

        let numbers: Vec<FileNumber> = vs
            .current()
            .levels[0]
            .iter()
            .map(|f| f.number)
            .collect();
        assert_eq!(numbers, vec![10, 11, 12]);
    }
}
