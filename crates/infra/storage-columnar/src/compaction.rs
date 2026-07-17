//! Compaction planner for the columnar engine.

use crate::manifest::{FileMeta, Manifest};
use crate::options::ColumnarOptions;
use crate::Result;

/// Files selected for a single compaction job.
#[derive(Debug, Clone)]
pub struct CompactionInput {
    /// Partition directory being compacted.
    pub partition: String,
    /// Files to replace.
    pub files: Vec<FileMeta>,
}

/// Plan compaction jobs for the current manifest.
///
/// For each partition, if the number of files exceeds `max_small_files` or the
/// total byte size of the smallest files exceeds `compaction_threshold_bytes`,
/// select those files for compaction.
///
/// If `partition` is `Some`, only that partition is considered.
pub fn plan(
    manifest: &Manifest,
    options: &ColumnarOptions,
    partition: Option<&str>,
) -> Result<Vec<CompactionInput>> {
    let mut jobs = Vec::new();

    // Group files by partition.
    let mut by_partition: std::collections::HashMap<String, Vec<&FileMeta>> =
        std::collections::HashMap::new();
    for file in &manifest.files {
        by_partition
            .entry(file.partition.clone())
            .or_default()
            .push(file);
    }

    for (part, files) in by_partition {
        if let Some(p) = partition && part != p {
            continue;
        }

        // Sort by creation time so older files are compacted first.
        let mut files = files;
        files.sort_by_key(|f| f.created_at);

        if files.len() >= options.max_small_files {
            let selected: Vec<FileMeta> = files.iter().map(|&f| f.clone()).collect();
            jobs.push(CompactionInput {
                partition: part,
                files: selected,
            });
            continue;
        }

        let total_bytes: u64 = files
            .iter()
            .map(|f| std::fs::metadata(&f.path).map(|m| m.len()).unwrap_or(0))
            .sum();
        if total_bytes >= options.compaction_threshold_bytes {
            let selected: Vec<FileMeta> = files.iter().map(|&f| f.clone()).collect();
            jobs.push(CompactionInput {
                partition: part,
                files: selected,
            });
        }
    }

    Ok(jobs)
}
