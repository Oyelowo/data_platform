//! VersionSet: owns current Version and assigns file numbers.

use std::sync::{Arc, Mutex};

use crate::internal_key::compare_internal_keys;
use crate::version::{FileMetaData, Version};
use crate::{FileNumber, Result};

/// Shared ownership of the current Version and monotonic file numbers.
pub struct VersionSet {
    inner: Mutex<Inner>,
}

struct Inner {
    current: Arc<Version>,
    next_file_number: FileNumber,
    last_sequence: u64,
}

impl VersionSet {
    pub fn new(num_levels: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                current: Arc::new(Version::new(num_levels)),
                next_file_number: 1,
                last_sequence: u64::MAX,
            }),
        }
    }

    pub fn current(&self) -> Arc<Version> {
        self.inner.lock().unwrap().current.clone()
    }

    pub fn new_file_number(&self) -> FileNumber {
        let mut inner = self.inner.lock().unwrap();
        let n = inner.next_file_number;
        inner.next_file_number += 1;
        n
    }

    /// Return the next file number that will be assigned (without consuming it).
    pub fn next_file_number(&self) -> FileNumber {
        self.inner.lock().unwrap().next_file_number
    }

    /// Set the next file number. Used during recovery to ensure reused opens
    /// do not collide with existing SSTable files.
    pub fn set_next_file_number(&self, n: FileNumber) {
        let mut inner = self.inner.lock().unwrap();
        inner.next_file_number = inner.next_file_number.max(n);
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
    pub fn apply(&self, edit: VersionEdit) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let mut new_version = (*inner.current).clone();
        for (level, number) in edit.deleted_files {
            new_version.levels[level].retain(|f| f.number != number);
        }
        for (level, meta) in edit.new_files {
            new_version.levels[level].push(meta);
        }
        for level in 0..new_version.levels.len() {
            new_version.levels[level].sort_by(|a, b| {
                compare_internal_keys(&a.smallest, &b.smallest)
            });
        }
        inner.current = Arc::new(new_version);
        inner.next_file_number = inner.next_file_number.max(edit.next_file_number);
        inner.last_sequence = inner.last_sequence.min(edit.last_sequence);
        Ok(())
    }
}

/// A delta describing a change to the VersionSet.
#[derive(Debug, Default, Clone)]
pub struct VersionEdit {
    pub deleted_files: Vec<(usize, FileNumber)>,
    pub new_files: Vec<(usize, FileMetaData)>,
    pub next_file_number: FileNumber,
    pub last_sequence: u64,
}
