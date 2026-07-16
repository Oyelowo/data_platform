//! Leveled compaction picker.

use crate::options::LsmOptions;
use crate::version::{FileMetaData, Version};

/// A compaction job description.
#[derive(Debug, Clone)]
pub struct Compaction {
    pub level: usize,
    pub inputs: Vec<Vec<FileMetaData>>,
}

/// Pick the next compaction, if any.
pub fn pick_compaction(version: &Version, options: &LsmOptions) -> Option<Compaction> {
    let mut best_level = 0usize;
    let mut best_score = 1.0f64;

    let l0_files = version.level0_files();
    if l0_files >= options.level0_file_num_compaction_trigger {
        best_level = 0;
        best_score = l0_files as f64 / options.level0_file_num_compaction_trigger as f64;
    }

    let mut level_size = options.max_bytes_for_level_base;
    for level in 1..options.num_levels {
        let size = version.level_size(level);
        let score = size as f64 / level_size as f64;
        if score > best_score {
            best_level = level;
            best_score = score;
        }
        level_size = level_size.saturating_mul(options.max_bytes_for_level_multiplier);
    }

    if best_score <= 1.0 {
        return None;
    }

    if best_level == 0 {
        let inputs = version.levels[0].clone();
        Some(Compaction {
            level: 0,
            inputs: vec![inputs],
        })
    } else {
        let files = &version.levels[best_level];
        if files.is_empty() {
            return None;
        }
        let mut best_file = &files[0];
        let mut best_ratio = f64::MAX;
        for file in files {
            let overlap = version.overlapping_inputs(best_level + 1, &file.smallest, &file.largest);
            let overlap_size: u64 = overlap.iter().map(|f| f.file_size).sum();
            let ratio = overlap_size as f64 / file.file_size.max(1) as f64;
            if ratio < best_ratio {
                best_ratio = ratio;
                best_file = file;
            }
        }
        let lower = version.overlapping_inputs(best_level + 1, &best_file.smallest, &best_file.largest);
        Some(Compaction {
            level: best_level,
            inputs: vec![vec![best_file.clone()], lower],
        })
    }
}
