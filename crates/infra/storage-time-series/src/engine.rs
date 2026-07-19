//! `TimeSeriesEngine` and its `storage_traits::Engine` implementation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use storage_traits::{
    BoundKind, Engine, EngineStats, Error as TraitError, Result as TraitResult, TxnOptions,
};

use crate::chunk::builder::ChunkBuilder;
use crate::chunk::reader::ChunkReader;
use crate::compaction::{apply_retention, chunk_path, compact_small_chunks, finalize_chunk_path, list_chunk_files};
use crate::cursor::TimeSeriesCursor;
use crate::error::Error;
use crate::format::{
    Metadata, Sample, Timestamp, Value, WalRecord, META_FILE, decode_composite_key,
    encode_composite_key,
};
use crate::memtable::MemTable;
use crate::options::TimeSeriesOptions;
use crate::query::{Query, QueryResult};
use crate::recovery;
use crate::stats::TimeSeriesStats;
use crate::transaction::TimeSeriesTransaction;
use crate::wal::TimeSeriesWal;

/// Inner engine state shared between the public handle and transactions.
pub(crate) struct Inner {
    pub dir: PathBuf,
    pub options: TimeSeriesOptions,
    pub metadata: RwLock<Metadata>,
    pub memtable: Mutex<MemTable>,
    pub wal: TimeSeriesWal,
    pub write_lock: Mutex<()>,
}

/// A synchronous, durable time-series storage engine.
#[derive(Clone)]
pub struct TimeSeriesEngine {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for TimeSeriesEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeSeriesEngine")
            .field("dir", &self.inner.dir)
            .field("options", &self.inner.options)
            .finish()
    }
}

impl TimeSeriesEngine {
    /// Open or create a time-series engine at `dir` with `options`.
    pub fn open(dir: impl AsRef<Path>, options: TimeSeriesOptions) -> crate::Result<Self> {
        options.validate()?;
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;

        let meta_path = dir.join(META_FILE);
        let mut metadata = if meta_path.exists() {
            let bytes = std::fs::read(&meta_path)?;
            Metadata::decode(&bytes)?
        } else {
            Metadata::new(options.clone())
        };

        // Validate on-disk options match requested options where meaningful.
        if metadata.options.value_kind != options.value_kind {
            return Err(Error::invalid_argument(
                "cannot open engine with different value_kind",
            ));
        }
        // Persist the requested options.
        metadata.options = options.clone();

        let wal = TimeSeriesWal::open(&dir, options.wal_sync_policy)?;
        let mut memtable = MemTable::new();
        recovery::replay_wal(&wal, &mut memtable, &mut metadata, &options)?;

        let engine = Self {
            inner: Arc::new(Inner {
                dir,
                options,
                metadata: RwLock::new(metadata),
                memtable: Mutex::new(memtable),
                wal,
                write_lock: Mutex::new(()),
            }),
        };
        engine.persist_meta()?;
        Ok(engine)
    }

    pub(crate) fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }

    /// Insert or overwrite a sample for a series.
    pub fn put(&self, series_key: Vec<u8>, timestamp: Timestamp, value: Value) -> crate::Result<()> {
        if series_key.len() > self.inner.options.max_key_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Key,
                limit: self.inner.options.max_key_len,
                got: series_key.len(),
            }
            .into());
        }
        let _guard = self.inner.write_lock.lock();
        self.put_unlocked(series_key, timestamp, value)
    }

    pub(crate) fn put_unlocked(
        &self,
        series_key: Vec<u8>,
        timestamp: Timestamp,
        value: Value,
    ) -> crate::Result<()> {
        let record = WalRecord::Put {
            series_key: series_key.clone(),
            timestamp,
            value: value.clone(),
        };
        self.inner.wal.append(record)?;
        {
            let mut meta = self.inner.metadata.write();
            meta.series.insert(series_key.clone());
            meta.label_index.insert(series_key.clone())?;
        }
        self.inner
            .memtable
            .lock()
            .insert(series_key, Sample { timestamp, value });
        Ok(())
    }

    /// Delete all samples for a series.
    pub fn delete_series(&self, series_key: &[u8]) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        self.delete_series_unlocked(series_key)
    }

    pub(crate) fn delete_series_unlocked(&self, series_key: &[u8]) -> crate::Result<()> {
        self.inner
            .wal
            .append(WalRecord::DeleteSeries {
                series_key: series_key.to_vec(),
            })?;
        {
            let mut meta = self.inner.metadata.write();
            meta.series.remove(series_key);
            meta.label_index.remove(series_key);
        }
        self.inner.memtable.lock().delete_series(series_key);
        // Remove chunk files for this series.
        for file in list_chunk_files(&self.inner.dir)? {
            if file.series_key == series_key {
                let _ = std::fs::remove_file(&file.path);
            }
        }
        Ok(())
    }

    /// Delete samples in a half-open time range for a series.
    pub fn delete_range(
        &self,
        series_key: &[u8],
        start: Timestamp,
        end: Timestamp,
    ) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        self.delete_range_unlocked(series_key, start, end)
    }

    pub(crate) fn delete_range_unlocked(
        &self,
        series_key: &[u8],
        start: Timestamp,
        end: Timestamp,
    ) -> crate::Result<()> {
        self.inner
            .wal
            .append(WalRecord::DeleteRange {
                series_key: series_key.to_vec(),
                start,
                end,
            })?;
        self.inner
            .memtable
            .lock()
            .delete_range(series_key, start, end);
        // Rewrite affected chunk files.
        let mut rewritten = Vec::new();
        for file in list_chunk_files(&self.inner.dir)? {
            if file.series_key != series_key || file.max_ts < start || file.min_ts >= end {
                continue;
            }
            let data = std::fs::read(&file.path)?;
            let reader = ChunkReader::new(&data)?;
            let samples: Vec<Sample> = reader
                .samples()?
                .into_iter()
                .filter(|s| s.timestamp < start || s.timestamp >= end)
                .collect();
            if samples.is_empty() {
                let _ = std::fs::remove_file(&file.path);
                continue;
            }
            let mut builder =
                ChunkBuilder::new(series_key.to_vec(), self.inner.options.compression);
            for s in samples {
                builder.push(s)?;
            }
            let bytes = builder.finish()?;
            let path = chunk_path(&self.inner.dir, series_key, file.min_ts);
            storage_file::atomic_write(&path, &bytes)?;
            let final_path = finalize_chunk_path(&self.inner.dir, &path, series_key, file.max_ts)?;
            rewritten.push(final_path);
        }
        Ok(())
    }

    /// Get the latest sample for a series.
    pub fn get_latest(&self, series_key: &[u8]) -> crate::Result<Option<Sample>> {
        let memtable = self.inner.memtable.lock();
        let memtable_latest = memtable.latest(series_key);
        drop(memtable);

        let mut latest: Option<Sample> = memtable_latest;
        for file in list_chunk_files(&self.inner.dir)? {
            if file.series_key != series_key {
                continue;
            }
            let data = std::fs::read(&file.path)?;
            let reader = ChunkReader::new(&data)?;
            if let Some(s) = reader.samples()?.into_iter().last()
                && latest.as_ref().is_none_or(|l| s.timestamp > l.timestamp)
            {
                latest = Some(s);
            }
        }
        Ok(latest)
    }

    /// Get samples for a series in the half-open range `[start, end)`.
    pub fn get_range(
        &self,
        series_key: &[u8],
        start: Timestamp,
        end: Timestamp,
    ) -> crate::Result<Vec<Sample>> {
        let mut merged: BTreeMap<Timestamp, Sample> = BTreeMap::new();
        for sample in self.inner.memtable.lock().range(series_key, start, end) {
            merged.insert(sample.timestamp, sample);
        }
        for file in list_chunk_files(&self.inner.dir)? {
            if file.series_key != series_key || file.max_ts < start || file.min_ts >= end {
                continue;
            }
            let data = std::fs::read(&file.path)?;
            let reader = ChunkReader::new(&data)?;
            for sample in reader.range(start, end)? {
                merged.insert(sample.timestamp, sample);
            }
        }
        Ok(merged.into_values().collect())
    }

    /// Execute a tag-filtered, time-range query.
    pub fn query(&self, query: Query) -> crate::Result<QueryResult> {
        let series_keys = {
            let meta = self.inner.metadata.read();
            meta.label_index.match_series(&query.metric, &query.filters)?
        };
        let mut series: BTreeMap<Vec<u8>, Vec<Sample>> = BTreeMap::new();
        let mut aggregates: Option<BTreeMap<Vec<u8>, crate::query::aggregate::AggregateResult>> =
            None;
        if query.aggregation.is_some() {
            aggregates = Some(BTreeMap::new());
        }
        for series_key in series_keys {
            let samples = self.get_range(&series_key, query.time_range.0, query.time_range.1)?;
            match (query.aggregation, &mut aggregates) {
                (Some(agg), Some(aggregates)) => {
                    let result = crate::query::aggregate::aggregate_samples(&samples, agg)?;
                    aggregates.insert(series_key.clone(), result);
                }
                (None, _) => {
                    series.insert(series_key, samples);
                }
                _ => {}
            }
        }
        Ok(QueryResult { series, aggregates })
    }

    /// Flush memtable, apply retention, compact chunks, and persist metadata.
    pub fn sync(&self) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Error::Io(std::io::Error::other(e)))?
            .as_nanos() as u64;

        self.flush_memtable(now)?;
        apply_retention(&self.inner.dir, self.inner.options.retention, now)?;
        compact_small_chunks(
            &self.inner.dir,
            self.inner.options.chunk_size_target,
            self.inner.options.compression,
        )?;
        self.persist_meta()?;
        self.inner.wal.sync()?;
        self.inner.wal.truncate_completed()?;
        Ok(())
    }

    fn flush_memtable(&self, now: Timestamp) -> crate::Result<()> {
        let mut memtable = self.inner.memtable.lock();
        let (builders, retained) = memtable.flush(
            self.inner.options.compression,
            self.inner.options.chunk_size_target,
            self.inner.options.retention,
            now,
        );
        *memtable = retained;
        drop(memtable);

        for builder in builders {
            let series_key = builder.series_key().to_vec();
            let max_ts = builder.max_ts();
            let bytes = builder.finish()?;
            if bytes.is_empty() {
                continue;
            }
            let path = chunk_path(&self.inner.dir, &series_key, 0);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            storage_file::atomic_write(&path, &bytes)?;
            let _ = finalize_chunk_path(&self.inner.dir, &path, &series_key, max_ts);
        }
        Ok(())
    }

    /// Persist the current metadata file atomically.
    pub fn persist_meta(&self) -> crate::Result<()> {
        let meta = self.inner.metadata.read();
        let encoded = meta.encode()?;
        storage_file::atomic_write(&self.inner.dir.join(META_FILE), &encoded)?;
        Ok(())
    }

    /// Close the engine gracefully.
    pub fn close(&self) -> crate::Result<()> {
        self.sync()?;
        self.inner.wal.close()?;
        Ok(())
    }

    /// Return engine statistics.
    pub fn stats(&self) -> crate::Result<TimeSeriesStats> {
        let meta = self.inner.metadata.read();
        let memtable = self.inner.memtable.lock();
        let chunk_files = list_chunk_files(&self.inner.dir)?.len() as u64;
        Ok(TimeSeriesStats {
            name: "storage-time-series",
            disk_bytes: approx_dir_bytes(&self.inner.dir)?,
            memory_bytes: memtable.bytes() as u64,
            num_series: meta.series.len() as u64,
            memtable_samples: memtable.len() as u64,
            chunk_files,
            metrics: {
                let mut m = std::collections::HashMap::new();
                m.insert("max_key_len".into(), self.inner.options.max_key_len as u64);
                m
            },
        })
    }
}

fn approx_dir_bytes(dir: &Path) -> crate::Result<u64> {
    let mut total = 0u64;
    if let Ok(entries) = walkdir(dir) {
        for entry in entries {
            if let Ok(md) = entry.metadata() {
                total += md.len();
            }
        }
    }
    Ok(total)
}

fn walkdir(path: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
    fn collect(path: &Path, out: &mut Vec<std::fs::DirEntry>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(&entry.path(), out)?;
            } else {
                out.push(entry);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    collect(path, &mut out)?;
    Ok(out)
}

impl Engine for TimeSeriesEngine {
    type Error = Error;
    type Transaction = TimeSeriesTransaction;
    type Cursor = TimeSeriesCursor;

    fn name(&self) -> &'static str {
        "storage-time-series"
    }

    fn begin(&self, opts: TxnOptions) -> TraitResult<Self::Transaction, Self::Error> {
        Ok(TimeSeriesTransaction::new(self.clone(), opts))
    }

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        let (series_key, timestamp) = decode_composite_key(key)?;
        let samples = self.get_range(series_key, timestamp, timestamp + 1)?;
        Ok(samples.into_iter().next().map(|s| Bytes::from(s.value.encode())))
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<Self::Cursor, Self::Error> {
        // Collect all matching samples across all series and merge.
        let mut map: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let series: Vec<Vec<u8>> = {
            let meta = self.inner.metadata.read();
            meta.series.iter().cloned().collect()
        };
        for series_key in series {
            let series_start_key = encode_composite_key(&series_key, 0);
            let series_end_key = encode_composite_key(&series_key, Timestamp::MAX);
            // Skip series entirely outside the requested key range.
            if start.is_some_and(|s| series_end_key.as_slice() < s) {
                continue;
            }
            if end.is_some_and(|e| series_start_key.as_slice() >= e) {
                continue;
            }
            let range_start = start
                .and_then(|s| {
                    if s.starts_with(&series_key) {
                        decode_composite_key(s).ok().map(|(_, ts)| ts)
                    } else {
                        Some(0)
                    }
                })
                .unwrap_or(0);
            let range_end = end
                .and_then(|e| {
                    if e.starts_with(&series_key) {
                        decode_composite_key(e).ok().map(|(_, ts)| ts)
                    } else {
                        Some(Timestamp::MAX)
                    }
                })
                .unwrap_or(Timestamp::MAX);
            let samples = self.get_range(&series_key, range_start, range_end)?;
            for sample in samples {
                let key = encode_composite_key(&series_key, sample.timestamp);
                if start.map(|s| key.as_slice() < s).unwrap_or(false) {
                    continue;
                }
                if end.map(|e| key.as_slice() >= e).unwrap_or(false) {
                    continue;
                }
                map.insert(key, sample.value.encode());
            }
        }
        Ok(TimeSeriesCursor::from_map(map))
    }

    fn stats(&self) -> TraitResult<EngineStats, Self::Error> {
        let s = self.stats()?;
        Ok(s.into_engine_stats())
    }

    fn sync(&self) -> TraitResult<(), Self::Error> {
        TimeSeriesEngine::sync(self)
    }
}

impl From<TraitError> for Error {
    fn from(e: TraitError) -> Self {
        match e {
            TraitError::Io(io) => Error::Io(io),
            TraitError::OutOfBounds { kind, limit, got } => Error::InvalidArgument(format!(
                "{kind} out of bounds: limit {limit}, got {got}"
            )),
            TraitError::InactiveTransaction => Error::InactiveTransaction,
            TraitError::ReadOnlyTransaction => Error::ReadOnlyTransaction,
            TraitError::Unsupported(msg) => Error::Unsupported(msg),
            TraitError::Corruption(msg) => Error::Corruption(msg),
            TraitError::NotFound(msg) => Error::NotFound(msg),
            TraitError::Conflict(msg) => Error::Conflict(msg),
            _ => Error::InvalidArgument("unknown trait error".into()),
        }
    }
}
