//! Leveled compaction picker.
//!
//! The picker follows the production leveled-compaction policy used by LevelDB and
//! RocksDB:
//!
//! 1. Compute a compaction score for every level.  L0 uses its file count; deeper
//!    levels use `level_size / target_size`.
//! 2. Pick the level with the highest score above 1.0.
//! 3. L0 compactions select all L0 files and expand the key range to include all
//!    overlapping L1 files.
//! 4. Ln (n >= 1) compactions select the file with the smallest overlap ratio with
//!    the next level, rotating through the key space so compaction coverage is even.

use crate::internal_key::{compare_internal_keys, extract_user_key};
use crate::options::LsmOptions;
use crate::version::{FileMetaData, Version, range_boundaries};
use crate::version_set::VersionSet;
use std::cmp::Ordering;

/// A compaction job description.
#[derive(Debug, Clone)]
pub struct Compaction {
    pub level: usize,
    /// inputs[0] = files from `level`, inputs[1] = files from `level + 1`.
    pub inputs: Vec<Vec<FileMetaData>>,
    /// Largest internal key covered by the job.  Used for key-space rotation and
    /// metrics once the compaction worker is implemented.
    #[allow(dead_code)]
    pub largest: Vec<u8>,
}

impl Compaction {
    /// Total number of input files.
    #[allow(dead_code)]
    pub fn num_input_files(&self) -> usize {
        self.inputs.iter().map(|v| v.len()).sum()
    }

    /// Total byte size of input files.
    #[allow(dead_code)]
    pub fn input_size(&self) -> u64 {
        self.inputs
            .iter()
            .flat_map(|v| v.iter())
            .map(|f| f.file_size)
            .sum()
    }
}

/// Pick the next compaction, if any.
pub fn pick_compaction(
    version: &Version,
    version_set: &VersionSet,
    options: &LsmOptions,
) -> Option<Compaction> {
    let level = version.pick_compaction_level(options)?;
    if level == 0 {
        pick_l0_compaction(version, options)
    } else {
        pick_level_compaction(version, version_set, options, level)
    }
}

/// L0 -> L1 compaction: all L0 files plus all overlapping L1 files.
fn pick_l0_compaction(version: &Version, options: &LsmOptions) -> Option<Compaction> {
    if version.level0_files() < options.level0_file_num_compaction_trigger {
        return None;
    }

    let inputs = version.levels[0].clone();
    let (mut smallest, mut largest) = range_boundaries(&inputs);
    let mut lower: Vec<FileMetaData> = Vec::new();

    // L0 files overlap each other.  Repeatedly expand the key range until all
    // overlapping L1 files are included and the range stabilizes.
    loop {
        let mut extended = false;
        for file in version.overlapping_inputs(1, &smallest, &largest) {
            if lower.iter().any(|f| f.number == file.number) {
                continue;
            }
            if compare_internal_keys(&file.smallest, &smallest) == Ordering::Less {
                smallest = file.smallest.clone();
                extended = true;
            }
            if compare_internal_keys(&file.largest, &largest) == Ordering::Greater {
                largest = file.largest.clone();
                extended = true;
            }
            lower.push(file);
        }
        if !extended {
            break;
        }
    }

    Some(Compaction {
        level: 0,
        inputs: vec![inputs, lower],
        largest,
    })
}

/// Ln -> Ln+1 compaction: pick one file from `level` with minimal overlap ratio.
fn pick_level_compaction(
    version: &Version,
    version_set: &VersionSet,
    _options: &LsmOptions,
    level: usize,
) -> Option<Compaction> {
    debug_assert!(level >= 1);
    debug_assert!(level + 1 < version.levels.len());

    let files = &version.levels[level];
    if files.is_empty() {
        return None;
    }

    let start_hint = version_set.compaction_pointer(level);
    let rotated = rotate_files(files, start_hint.as_deref());

    let mut best_file: Option<&FileMetaData> = None;
    let mut best_ratio = f64::MAX;

    for file in rotated {
        let overlap = version.overlapping_inputs(level + 1, &file.smallest, &file.largest);
        let overlap_size: u64 = overlap.iter().map(|f| f.file_size).sum();
        let ratio = overlap_size as f64 / file.file_size.max(1) as f64;
        if ratio < best_ratio {
            best_ratio = ratio;
            best_file = Some(file);
        }
    }

    let best_file = best_file?;
    let lower = version.overlapping_inputs(level + 1, &best_file.smallest, &best_file.largest);

    Some(Compaction {
        level,
        inputs: vec![vec![best_file.clone()], lower],
        largest: best_file.largest.clone(),
    })
}

/// Iterate over `files` starting with the first file whose user-key range begins
/// after `start_hint`, then wrap around to the beginning.  This rotates
/// compactions through the key space.
fn rotate_files<'a>(
    files: &'a [FileMetaData],
    start_hint: Option<&[u8]>,
) -> impl Iterator<Item = &'a FileMetaData> {
    let start_idx = match start_hint {
        Some(hint) => {
            files.partition_point(|f| extract_user_key(&f.smallest).cmp(hint) != Ordering::Greater)
        }
        None => 0,
    };

    files[start_idx..]
        .iter()
        .chain(files[..start_idx.min(files.len())].iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_key::{ValueType, build_internal_key};

    /// Build an internal key from a user key for use in test metadata.
    fn ikey(user: &[u8]) -> Vec<u8> {
        build_internal_key(user, 1, ValueType::Value)
    }

    fn file(number: u64, smallest: Vec<u8>, largest: Vec<u8>, file_size: u64) -> FileMetaData {
        FileMetaData {
            number,
            smallest,
            largest,
            file_size,
        }
    }

    fn opts() -> LsmOptions {
        LsmOptions {
            write_buffer_size: 256,
            max_write_buffer_number: 3,
            level0_file_num_compaction_trigger: 4,
            level0_slowdown_writes_trigger: 12,
            level0_stop_writes_trigger: 20,
            max_bytes_for_level_base: 256,
            max_bytes_for_level_multiplier: 10,
            target_file_size_base: 64,
            target_file_size_multiplier: 1,
            num_levels: 7,
            compaction_max_overlap_files: 10,
            block_size: 4 * 1024,
            block_restart_interval: 16,
            bloom_bits_per_key: 10,
            wal_segment_size: 64 * 1024 * 1024,
            block_cache_size: 8 * 1024 * 1024,
            compressed_block_cache_size: 0,
            compression: crate::sstable::format::CompressionType::Lz4,
            bottommost_compression: crate::sstable::format::CompressionType::Zstd,
            logger: None,
            min_blob_value_size: 0,
            blob_file_size: 64 * 1024 * 1024,
            blob_gc_ratio: 0.0,
            blob_gc_interval_ms: 0,
            blob_gc_threads: 1,
            blob_gc_force_threshold: 0.0,
        }
    }

    #[test]
    fn no_compaction_when_all_scores_low() {
        let version = Version::new(7);
        let options = opts();
        assert!(version.pick_compaction_level(&options).is_none());
        let vs = VersionSet::new(7);
        assert!(pick_compaction(&version, &vs, &options).is_none());
    }

    #[test]
    fn l0_file_count_trigger() {
        let mut version = Version::new(7);
        let options = opts();
        for i in 0..options.level0_file_num_compaction_trigger {
            version.levels[0].push(file(i as u64, ikey(&[i as u8]), ikey(&[i as u8, 0xff]), 1));
        }
        assert_eq!(version.compaction_score(0, &options), 1.0);
        version.levels[0].push(file(100, ikey(&[9]), ikey(&[9, 0xff]), 1));
        assert!(version.compaction_score(0, &options) > 1.0);

        let vs = VersionSet::new(7);
        let job = pick_compaction(&version, &vs, &options).unwrap();
        assert_eq!(job.level, 0);
        assert_eq!(
            job.inputs[0].len(),
            options.level0_file_num_compaction_trigger + 1
        );
    }

    #[test]
    fn l0_expands_to_overlapping_l1() {
        let mut version = Version::new(7);
        let options = opts();
        // L0 files cover keys 0, 1, 2, 3.
        for i in 0..4 {
            version.levels[0].push(file(i as u64, ikey(&[i * 2]), ikey(&[i * 2 + 1]), 1));
        }
        // L1 has one file that overlaps [0,7] and one that does not.
        version.levels[1].push(file(10, ikey(&[0]), ikey(&[1]), 1));
        version.levels[1].push(file(11, ikey(&[100]), ikey(&[101]), 1));

        let vs = VersionSet::new(7);
        let job = pick_compaction(&version, &vs, &options).unwrap();
        assert_eq!(job.level, 0);
        assert_eq!(job.inputs[0].len(), 4);
        assert_eq!(job.inputs[1].len(), 1);
        assert_eq!(job.inputs[1][0].number, 10);
    }

    #[test]
    fn size_trigger_selects_overfull_level() {
        let mut version = Version::new(7);
        let mut options = opts();
        options.max_bytes_for_level_base = 100;
        options.max_bytes_for_level_multiplier = 10;

        // L1 is exactly at target.
        version.levels[1].push(file(1, ikey(&[0]), ikey(&[9]), 100));
        // L2 is well over target.
        version.levels[2].push(file(2, ikey(&[0]), ikey(&[9]), 1100));

        assert_eq!(version.compaction_score(1, &options), 1.0);
        assert!(version.compaction_score(2, &options) > 1.0);

        let vs = VersionSet::new(7);
        let job = pick_compaction(&version, &vs, &options).unwrap();
        assert_eq!(job.level, 2);
    }

    #[test]
    fn level_picker_rotates_through_key_space() {
        let mut version = Version::new(7);
        let mut options = opts();
        // Make L1 exceed its target so the level is selected.
        options.max_bytes_for_level_base = 50;

        // Two disjoint L1 files, each overlapping exactly one L2 file.
        version.levels[1].push(file(1, ikey(&[0]), ikey(&[9]), 100));
        version.levels[1].push(file(2, ikey(&[10]), ikey(&[19]), 100));
        version.levels[2].push(file(3, ikey(&[0]), ikey(&[9]), 50));
        version.levels[2].push(file(4, ikey(&[10]), ikey(&[19]), 50));

        let vs = VersionSet::new(7);
        vs.set_compaction_pointer(1, vec![0]);

        // With pointer at user key 0, rotation should start with file 2 (keys 10..19).
        let job = pick_compaction(&version, &vs, &options).unwrap();
        assert_eq!(job.level, 1);
        assert_eq!(job.inputs[0][0].number, 2);
    }

    #[test]
    fn last_level_is_not_selected() {
        let mut version = Version::new(3);
        let options = opts();
        // Fill the last level far beyond target.
        version.levels[2].push(file(1, ikey(&[0]), ikey(&[9]), u64::MAX / 2));
        // No level above 2 exists, so it must not be picked.
        assert!(version.pick_compaction_level(&options).is_none());
    }
}
