//! Compaction planner and background worker for the columnar engine.

use crate::Result;
use crate::manifest::{FileMeta, Manifest};
use crate::options::ColumnarOptions;

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
        if let Some(p) = partition
            && part != p
        {
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

/// Messages sent to the background compaction worker.
#[derive(Debug)]
pub enum CompactionMsg {
    /// Trigger a compaction run.
    Trigger,
    /// Stop the worker.
    Shutdown,
}

/// Handle to the background compaction worker.
#[derive(Debug)]
pub struct CompactionWorker {
    tx: std::sync::mpsc::Sender<CompactionMsg>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl CompactionWorker {
    /// Spawn a background thread that receives compaction triggers and runs
    /// compaction using the provided closure.
    pub fn spawn<F>(compact: F) -> Self
    where
        F: Fn() -> Result<usize> + Send + Sync + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel::<CompactionMsg>();
        let handle = std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                match msg {
                    CompactionMsg::Trigger => {
                        let _ = compact();
                    }
                    CompactionMsg::Shutdown => break,
                }
            }
        });
        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Request a compaction run without blocking.
    pub fn trigger(&self) -> Result<()> {
        self.tx
            .send(CompactionMsg::Trigger)
            .map_err(|_| crate::Error::Batch("background compaction channel closed".into()))?;
        Ok(())
    }

    /// Signal the worker to shut down and wait for it to finish.
    pub fn shutdown(&mut self) -> Result<()> {
        let _ = self.tx.send(CompactionMsg::Shutdown);
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| crate::Error::Batch("background compaction thread panicked".into()))?;
        }
        Ok(())
    }
}

/// Compute the number of output files to produce for a compaction job given
/// the total input byte size and the configured target file size.
///
/// A `target_file_size` of 0 disables splitting and always returns 1.
pub fn output_file_count(total_input_bytes: u64, target_file_size: u64) -> usize {
    if target_file_size == 0 || total_input_bytes <= target_file_size {
        return 1;
    }
    let count = total_input_bytes.div_ceil(target_file_size);
    count.max(1) as usize
}
