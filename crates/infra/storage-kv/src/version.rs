//! Level metadata and Version snapshots.

use crate::internal_key::compare_internal_keys;
use crate::FileNumber;

/// Metadata for a single SSTable file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetaData {
    pub number: FileNumber,
    pub file_size: u64,
    pub smallest: Vec<u8>,
    pub largest: Vec<u8>,
}

impl FileMetaData {
    pub fn overlaps(&self, smallest: &[u8], largest: &[u8]) -> bool {
        compare_internal_keys(&self.smallest, largest) != std::cmp::Ordering::Greater
            && compare_internal_keys(&self.largest, smallest) != std::cmp::Ordering::Less
    }
}

/// Immutable snapshot of the LSM-tree levels.
#[derive(Debug, Clone, Default)]
pub struct Version {
    pub levels: Vec<Vec<FileMetaData>>,
}

impl Version {
    pub fn new(num_levels: usize) -> Self {
        Self {
            levels: vec![Vec::new(); num_levels],
        }
    }

    /// Total size of a level.
    pub fn level_size(&self, level: usize) -> u64 {
        self.levels[level].iter().map(|f| f.file_size).sum()
    }

    /// Number of files in L0.
    pub fn level0_files(&self) -> usize {
        self.levels[0].len()
    }

    /// Find files in `level` that overlap [smallest, largest].
    pub fn overlapping_inputs(&self, level: usize, smallest: &[u8], largest: &[u8]) -> Vec<FileMetaData> {
        self.levels[level]
            .iter()
            .filter(|f| f.overlaps(smallest, largest))
            .cloned()
            .collect()
    }

    /// For level > 0, find the single file that may contain `key`.
    pub fn pick_level_file(&self, level: usize, key: &[u8]) -> Option<&FileMetaData> {
        self.levels[level]
            .binary_search_by(|f| compare_internal_keys(&f.largest, key))
            .ok()
            .and_then(|idx| self.levels[level].get(idx))
    }
}
