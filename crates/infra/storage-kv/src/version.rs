//! Level metadata and Version snapshots.

use crate::FileNumber;
use crate::internal_key::{compare_internal_keys, extract_user_key};
use crate::options::LsmOptions;

/// Metadata for a single SSTable file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetaData {
    pub number: FileNumber,
    pub file_size: u64,
    pub smallest: Vec<u8>,
    pub largest: Vec<u8>,
}

impl FileMetaData {
    /// True if this file's user-key range overlaps `[smallest, largest]`.
    ///
    /// `smallest` and `largest` are internal keys; only their user-key prefix is
    /// used for the comparison.  Comparing by full internal key (which embeds
    /// the sequence number) is incorrect: a file containing user key `k` at
    /// sequence 1 overlaps a query for the same user key `k` at sequence 100.
    pub fn overlaps(&self, smallest: &[u8], largest: &[u8]) -> bool {
        let self_smallest_user = extract_user_key(&self.smallest);
        let self_largest_user = extract_user_key(&self.largest);
        let query_smallest_user = extract_user_key(smallest);
        let query_largest_user = extract_user_key(largest);
        self_smallest_user <= query_largest_user && self_largest_user >= query_smallest_user
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
    pub fn overlapping_inputs(
        &self,
        level: usize,
        smallest: &[u8],
        largest: &[u8],
    ) -> Vec<FileMetaData> {
        self.levels[level]
            .iter()
            .filter(|f| f.overlaps(smallest, largest))
            .cloned()
            .collect()
    }

    /// For level > 0, find the single file that may contain `key`.
    ///
    /// `key` is a user key (without the internal-key trailer). Levels above L0
    /// are non-overlapping and sorted by user key, so we binary-search by the
    /// user-key portion of each file's largest internal key.
    #[allow(dead_code)]
    pub fn pick_level_file(&self, level: usize, key: &[u8]) -> Option<&FileMetaData> {
        let idx = self.levels[level]
            .binary_search_by(|f| extract_user_key(&f.largest).cmp(key))
            .unwrap_or_else(|idx| idx);
        self.levels[level].get(idx)
    }

    /// Target byte size for a level.
    ///
    /// L0 has no byte target; levels >= 1 follow the RocksDB static leveled
    /// formula: `max_bytes_for_level_base * multiplier^(level - 1)`.
    pub fn level_target_bytes(level: usize, options: &LsmOptions) -> u64 {
        assert!(
            level >= 1,
            "level_target_bytes is only defined for level >= 1"
        );
        let mut target = options.max_bytes_for_level_base;
        for _ in 1..level {
            target = target.saturating_mul(options.max_bytes_for_level_multiplier);
        }
        target
    }

    /// Compaction score for a level.  A score > 1.0 means the level should be
    /// compacted.
    pub fn compaction_score(&self, level: usize, options: &LsmOptions) -> f64 {
        match level {
            0 => {
                let files = self.level0_files();
                files as f64 / options.level0_file_num_compaction_trigger as f64
            }
            _ => {
                let target = Self::level_target_bytes(level, options);
                if target == 0 {
                    return f64::MAX;
                }
                self.level_size(level) as f64 / target as f64
            }
        }
    }

    /// Return the level with the highest compaction score, or `None` if no
    /// level needs compaction.
    ///
    /// The last level is never selected because there is no deeper level to
    /// compact into.  A score of exactly 1.0 is treated as needing compaction,
    /// matching the LevelDB/RocksDB trigger semantics.
    pub fn pick_compaction_level(&self, options: &LsmOptions) -> Option<usize> {
        let max_level = self.levels.len().saturating_sub(1);
        if max_level == 0 {
            return None;
        }

        let mut best_level = None;
        let mut best_score = 1.0f64;
        for level in 0..max_level {
            let score = self.compaction_score(level, options);
            if score > best_score {
                best_score = score;
                best_level = Some(level);
            }
        }

        // If no score is strictly above 1.0, pick the first level whose score
        // is exactly 1.0 (e.g. L0 hit its file-count trigger precisely).
        if best_level.is_none() {
            for level in 0..max_level {
                if self.compaction_score(level, options) >= 1.0 {
                    return Some(level);
                }
            }
        }

        best_level
    }
}

/// Return the smallest and largest internal keys across a set of files.
pub fn range_boundaries(files: &[FileMetaData]) -> (Vec<u8>, Vec<u8>) {
    assert!(!files.is_empty(), "range_boundaries called with no files");
    let mut smallest = files[0].smallest.clone();
    let mut largest = files[0].largest.clone();
    for file in &files[1..] {
        if compare_internal_keys(&file.smallest, &smallest) == std::cmp::Ordering::Less {
            smallest = file.smallest.clone();
        }
        if compare_internal_keys(&file.largest, &largest) == std::cmp::Ordering::Greater {
            largest = file.largest.clone();
        }
    }
    (smallest, largest)
}
