//! Retention enforcement and simple chunk compaction.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::chunk::builder::ChunkBuilder;
use crate::chunk::reader::ChunkReader;
use crate::format::{Sample, Timestamp, CHUNKS_DIR};
use crate::options::{CompressionKind, RetentionPolicy};

/// Information about a chunk file on disk.
#[derive(Debug, Clone)]
pub struct ChunkFile {
    /// Full file path.
    pub path: PathBuf,
    /// Series key.
    pub series_key: Vec<u8>,
    /// Minimum timestamp.
    pub min_ts: Timestamp,
    /// Maximum timestamp.
    pub max_ts: Timestamp,
    /// File size in bytes.
    pub size: u64,
}

/// List all chunk files under `dir`.
pub fn list_chunk_files(dir: &Path) -> crate::Result<Vec<ChunkFile>> {
    let chunks_dir = dir.join(CHUNKS_DIR);
    if !chunks_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&chunks_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            for sub in std::fs::read_dir(&path)? {
                let sub = sub?;
                let p = sub.path();
                if p.extension().map(|e| e == "chunk").unwrap_or(false) {
                    let md = sub.metadata()?;
                    let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    // Expected: <series_hash>_<min_ts>_<max_ts>
                    let parts: Vec<_> = name.split('_').collect();
                    if parts.len() >= 3 {
                        let min_ts = parts[parts.len() - 2].parse().unwrap_or(0);
                        let max_ts = parts[parts.len() - 1].parse().unwrap_or(0);
                        let data = std::fs::read(&p)?;
                        let reader = ChunkReader::new(&data)?;
                        files.push(ChunkFile {
                            path: p,
                            series_key: reader.header().series_key.clone(),
                            min_ts,
                            max_ts,
                            size: md.len(),
                        });
                    }
                }
            }
        }
    }
    Ok(files)
}

/// Remove chunk files whose `max_ts` is older than the retention duration.
pub fn apply_retention(
    dir: &Path,
    retention: Option<RetentionPolicy>,
    now: Timestamp,
) -> crate::Result<()> {
    let Some(retention) = retention else { return Ok(()) };
    match retention {
        RetentionPolicy::Duration(d) => {
            let cutoff = now.saturating_sub(d.as_nanos() as u64);
            for file in list_chunk_files(dir)? {
                if file.max_ts < cutoff {
                    let _ = std::fs::remove_file(&file.path);
                }
            }
        }
        RetentionPolicy::MaxSamples(limit) => {
            apply_max_samples_retention(dir, limit)?;
        }
    }
    Ok(())
}

fn apply_max_samples_retention(dir: &Path, limit: usize) -> crate::Result<()> {
    let files = list_chunk_files(dir)?;
    let mut by_series: BTreeMap<Vec<u8>, Vec<ChunkFile>> = BTreeMap::new();
    for file in files {
        by_series.entry(file.series_key.clone()).or_default().push(file);
    }
    for (_series_key, mut series_files) in by_series {
        series_files.sort_by_key(|f| f.max_ts);
        let mut total = 0usize;
        // Count samples backwards from newest.
        for file in series_files.iter().rev() {
            let data = std::fs::read(&file.path)?;
            let reader = ChunkReader::new(&data)?;
            total += reader.header().count as usize;
        }
        for file in series_files.iter() {
            if total <= limit {
                break;
            }
            let data = std::fs::read(&file.path)?;
            let reader = ChunkReader::new(&data)?;
            total -= reader.header().count as usize;
            let _ = std::fs::remove_file(&file.path);
        }
    }
    Ok(())
}

/// Simple compaction: merge adjacent small chunks for the same series.
pub fn compact_small_chunks(
    dir: &Path,
    chunk_size_target: usize,
    compression: CompressionKind,
) -> crate::Result<()> {
    let files = list_chunk_files(dir)?;
    let mut by_series: BTreeMap<Vec<u8>, Vec<ChunkFile>> = BTreeMap::new();
    for file in files {
        by_series.entry(file.series_key.clone()).or_default().push(file);
    }

    for (series_key, mut series_files) in by_series {
        series_files.sort_by_key(|f| f.min_ts);
        let mut current: Vec<Sample> = Vec::new();
        let mut current_size = 0usize;
        let mut paths_to_remove: Vec<PathBuf> = Vec::new();

        for file in series_files {
            let data = std::fs::read(&file.path)?;
            let reader = ChunkReader::new(&data)?;
            let samples = reader.samples()?;
            let sample_size: usize = samples.iter().map(|s| 8 + s.value.encode().len()).sum();

            if current_size + sample_size > chunk_size_target && !current.is_empty() {
                write_compacted_chunk(dir, &series_key, &current, compression)?;
                current.clear();
                current_size = 0;
            }
            current.extend(samples);
            current_size += sample_size;
            paths_to_remove.push(file.path);
        }
        if !current.is_empty() {
            write_compacted_chunk(dir, &series_key, &current, compression)?;
        }
        for p in paths_to_remove {
            let _ = std::fs::remove_file(&p);
        }
    }
    Ok(())
}

fn write_compacted_chunk(
    dir: &Path,
    series_key: &[u8],
    samples: &[Sample],
    compression: CompressionKind,
) -> crate::Result<PathBuf> {
    let mut builder = ChunkBuilder::new(series_key.to_vec(), compression);
    for s in samples {
        builder.push(s.clone())?;
    }
    let bytes = builder.finish()?;
    let path = chunk_path(dir, series_key, samples.first().map(|s| s.timestamp).unwrap_or(0));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    storage_file::atomic_write(&path, &bytes)?;
    Ok(path)
}

/// Compute a deterministic chunk file path.
pub fn chunk_path(dir: &Path, series_key: &[u8], min_ts: Timestamp) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    series_key.hash(&mut hasher);
    let hash = hasher.finish();
    let prefix = format!("{:04x}", hash & 0xffff);
    dir.join(CHUNKS_DIR)
        .join(prefix)
        .join(format!("{hash}_{min_ts}_{}.chunk", Timestamp::MAX))
}

/// Rewrite a chunk file with a concrete max timestamp after it is finalized.
pub fn finalize_chunk_path(dir: &Path, temp_path: &Path, series_key: &[u8], max_ts: Timestamp) -> crate::Result<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    series_key.hash(&mut hasher);
    let hash = hasher.finish();
    let prefix = format!("{:04x}", hash & 0xffff);
    let final_path = dir
        .join(CHUNKS_DIR)
        .join(prefix)
        .join(format!("{hash}_{}_{max_ts}.chunk", extract_min_ts(temp_path).unwrap_or(0)));
    std::fs::rename(temp_path, &final_path)?;
    Ok(final_path)
}

fn extract_min_ts(path: &Path) -> Option<Timestamp> {
    let name = path.file_stem().and_then(|s| s.to_str())?;
    let parts: Vec<_> = name.split('_').collect();
    parts.get(parts.len().saturating_sub(2))?.parse().ok()
}
