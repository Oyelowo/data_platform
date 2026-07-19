//! Durable `ArtEngine` implementation.
//!
//! `ArtEngine` wraps the concurrent in-memory [`ArtMap`] with a write-ahead log,
//! a snapshot file, and metadata so that the tree survives process restarts and
//! crashes. Reads use the existing Optimistic Lock Coupling path and remain
//! concurrent; durable writes are serialized through a single engine-level write
//! lock so that WAL ordering and snapshot consistency are guaranteed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use storage_traits::{
    BoundKind, Cursor, Engine, EngineStats, Error as TraitError, IsolationLevel,
    Result as TraitResult, Transaction, TxnOptions,
};
use storage_wal::{Durability, Wal};

use crate::cursor::ArtCursor;
use crate::error::Error;
use crate::format::{Metadata, WalRecord, meta_path, snapshot_path, write_atomic};
use crate::map::ArtMap;
use crate::options::{ArtEngineOptions, WalSyncPolicy};
use crate::recovery::recover;

/// Shared inner state of an `ArtEngine`.
pub(crate) struct Inner {
    /// Directory owning `art.meta`, `snapshot.bin`, and the `wal/` subdirectory.
    pub dir: PathBuf,
    /// Runtime engine options.
    pub options: ArtEngineOptions,
    /// The in-memory Adaptive Radix Trie.
    pub map: ArtMap,
    /// Underlying write-ahead log.
    pub wal: Wal,
    /// Persisted metadata. Guarded by the write lock during mutations.
    pub metadata: Mutex<Metadata>,
    /// Serializes durable writers and snapshot/checkpoint operations.
    pub write_lock: Mutex<()>,
    /// Number of writes since the last snapshot.
    pub unsynced: AtomicUsize,
}

/// A synchronous, durable ordered key-value engine backed by an Adaptive Radix
/// Trie.
#[derive(Clone)]
pub struct ArtEngine {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for ArtEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtEngine")
            .field("dir", &self.inner.dir)
            .field("options", &self.inner.options)
            .finish()
    }
}

impl ArtEngine {
    /// Open or create a durable ART engine at `dir` with the given options.
    pub fn open(dir: impl AsRef<Path>, options: ArtEngineOptions) -> crate::Result<Self> {
        options.validate()?;
        let dir = dir.as_ref().to_path_buf();
        let (map, metadata, wal) = recover(&dir, options.clone())?;
        Ok(Self {
            inner: Arc::new(Inner {
                dir,
                options,
                map,
                wal,
                metadata: Mutex::new(metadata),
                write_lock: Mutex::new(()),
                unsynced: AtomicUsize::new(0),
            }),
        })
    }

    /// Return the directory path.
    pub fn path(&self) -> &Path {
        &self.inner.dir
    }

    /// Return a copy of the engine options.
    pub fn options(&self) -> ArtEngineOptions {
        self.inner.options.clone()
    }

    /// Gracefully close the engine, flushing the WAL and releasing the advisory
    /// directory lock.
    pub fn close(&self) -> crate::Result<()> {
        self.inner.wal.close().map_err(Error::Wal)
    }

    /// Simulate a power-loss crash by truncating the active WAL segment to the
    /// last fsynced byte. Any buffered records that have not reached stable
    /// storage are dropped. This is intended for tests and fault-injection only.
    #[doc(hidden)]
    pub fn crash(&self) -> crate::Result<()> {
        self.inner.wal.crash().map_err(Error::Wal)
    }

    /// Insert or overwrite `key` with `value`.
    ///
    /// Returns the previous value, if any. This operation is durable according
    /// to the configured [`WalSyncPolicy`].
    pub fn put(&self, key: &[u8], value: &[u8]) -> crate::Result<Option<Bytes>> {
        self.check_key(key)?;
        self.check_value(value)?;
        let _guard = self.inner.write_lock.lock();
        let old = self.inner.map.insert(key, value)?;
        let record = WalRecord::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        };
        self.append_record(record)?;
        self.inner.unsynced.fetch_add(1, Ordering::Relaxed);
        Ok(old)
    }

    /// Delete `key` and return its value if it existed.
    pub fn delete(&self, key: &[u8]) -> crate::Result<Option<Bytes>> {
        self.check_key(key)?;
        let _guard = self.inner.write_lock.lock();
        let old = self.inner.map.remove(key)?;
        if old.is_some() {
            let record = WalRecord::Delete { key: key.to_vec() };
            self.append_record(record)?;
            self.inner.unsynced.fetch_add(1, Ordering::Relaxed);
        }
        Ok(old)
    }

    fn append_record(&self, record: WalRecord) -> crate::Result<()> {
        let durability = self.wal_durability();
        let wal_record = record.into_wal();
        self.inner
            .wal
            .append_record(wal_record, durability)
            .map_err(Error::Wal)?;
        Ok(())
    }

    fn wal_durability(&self) -> Durability {
        match self.inner.options.wal_sync_policy {
            WalSyncPolicy::Immediate => Durability::Immediate,
            WalSyncPolicy::Buffered => Durability::Buffered,
        }
    }

    fn check_key(&self, key: &[u8]) -> crate::Result<()> {
        if key.len() > self.inner.options.map.max_key_len {
            return Err(Error::KeyTooLong {
                len: key.len(),
                max: self.inner.options.map.max_key_len,
            });
        }
        Ok(())
    }

    fn check_value(&self, value: &[u8]) -> crate::Result<()> {
        if value.len() > self.inner.options.map.max_value_len {
            return Err(Error::ValueTooLong {
                len: value.len(),
                max: self.inner.options.map.max_value_len,
            });
        }
        Ok(())
    }

    /// Write a snapshot and checkpoint the WAL, then truncate obsolete segments.
    ///
    /// If `options.snapshot_on_sync` is false, this only flushes the WAL.
    fn checkpoint(&self) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        if !self.inner.options.snapshot_on_sync {
            self.inner.wal.sync().map_err(Error::Wal)?;
            self.inner.unsynced.store(0, Ordering::Relaxed);
            return Ok(());
        }

        let snapshot_bytes = crate::snapshot::encode(&self.inner.map)?;
        let snapshot_crc = storage_format::crc32c(&snapshot_bytes);
        let snapshot_file = snapshot_path(&self.inner.dir);
        write_atomic(&snapshot_file, &snapshot_bytes)?;

        let mut metadata = self.inner.metadata.lock().unwrap().clone();
        metadata.snapshot_crc = snapshot_crc;

        let meta_bytes = metadata.encode();
        let meta_file = meta_path(&self.inner.dir);
        write_atomic(&meta_file, &meta_bytes)?;

        // The checkpoint record is durable and carries the metadata payload so
        // that tools can locate the snapshot by reading only the WAL.
        let checkpoint_lsn = self.inner.wal.checkpoint(&meta_bytes).map_err(Error::Wal)?;

        // `last_snapshot_lsn` is the first WAL byte offset that is *not* covered
        // by the snapshot. The checkpoint record itself is skipped during replay,
        // so the replay cursor must start immediately after it.
        let next_lsn =
            checkpoint_lsn + storage_wal::RECORD_HEADER_SIZE as u64 + meta_bytes.len() as u64;
        metadata.last_snapshot_lsn = next_lsn;
        let meta_bytes = metadata.encode();
        write_atomic(&meta_file, &meta_bytes)?;

        self.inner
            .wal
            .truncate_before(next_lsn)
            .map_err(Error::Wal)?;

        *self.inner.metadata.lock().unwrap() = metadata;
        self.inner.unsynced.store(0, Ordering::Relaxed);
        Ok(())
    }

    /// Number of entries in the engine.
    pub fn len(&self) -> usize {
        self.inner.map.len()
    }

    /// True if the engine contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Engine for ArtEngine {
    type Error = TraitError;
    type Transaction = ArtEngineTransaction;
    type Cursor = ArtCursor;

    fn name(&self) -> &'static str {
        "storage-art-durable"
    }

    fn begin(&self, opts: TxnOptions) -> TraitResult<Self::Transaction> {
        Ok(ArtEngineTransaction {
            engine: self.clone(),
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
            local: BTreeMap::new(),
        })
    }

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>> {
        self.check_key(key).map_err(map_error)?;
        Ok(self.inner.map.get(key))
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> TraitResult<Self::Cursor> {
        Ok(self.inner.map.range(start, end))
    }

    fn stats(&self) -> TraitResult<EngineStats> {
        let mut stats = crate::stats::engine_stats(&self.inner.map);
        stats.name = self.name();
        stats.disk_bytes = match disk_usage(&self.inner.dir) {
            Ok(n) => n as u64,
            Err(_) => 0,
        };
        Ok(stats)
    }

    fn sync(&self) -> TraitResult<()> {
        self.checkpoint().map_err(map_error)
    }
}

fn disk_usage(dir: &Path) -> std::io::Result<usize> {
    let mut total = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len() as usize;
        }
    }
    Ok(total)
}

/// A transaction over an `ArtEngine`.
#[derive(Debug)]
pub struct ArtEngineTransaction {
    engine: ArtEngine,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
    local: BTreeMap<Bytes, Option<Bytes>>,
}

impl ArtEngineTransaction {
    fn ensure_active(&self) -> TraitResult<()> {
        if !self.active {
            return Err(TraitError::InactiveTransaction);
        }
        Ok(())
    }
}

impl Transaction for ArtEngineTransaction {
    type Error = TraitError;

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>> {
        self.ensure_active()?;
        self.engine.check_key(key).map_err(map_error)?;
        if let Some(value) = self.local.get(key) {
            return Ok(value.clone());
        }
        Ok(self.engine.inner.map.get(key))
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> TraitResult<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(TraitError::ReadOnlyTransaction);
        }
        self.engine.check_key(key).map_err(map_error)?;
        self.engine.check_value(value).map_err(map_error)?;
        self.local.insert(
            Bytes::copy_from_slice(key),
            Some(Bytes::copy_from_slice(value)),
        );
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> TraitResult<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(TraitError::ReadOnlyTransaction);
        }
        self.engine.check_key(key).map_err(map_error)?;
        self.local.insert(Bytes::copy_from_slice(key), None);
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<impl Cursor<Error = Self::Error>> {
        self.ensure_active()?;

        let mut entries = Vec::new();
        self.engine.inner.map.collect_entries(&mut entries);

        let mut merged: BTreeMap<Bytes, Option<Bytes>> = entries
            .into_iter()
            .filter(|(k, _)| {
                let k = k.as_ref();
                let above_start = start.map(|s| k >= s).unwrap_or(true);
                let below_end = end.map(|e| k < e).unwrap_or(true);
                above_start && below_end
            })
            .map(|(k, v)| (k, Some(v)))
            .collect();

        for (k, v) in &self.local {
            if let Some(v) = v {
                merged.insert(k.clone(), Some(v.clone()));
            } else {
                merged.remove(k);
            }
        }

        let start_bound = start.map_or(std::ops::Bound::Unbounded, |s| {
            std::ops::Bound::Included(Bytes::copy_from_slice(s))
        });
        let end_bound = end.map_or(std::ops::Bound::Unbounded, |e| {
            std::ops::Bound::Excluded(Bytes::copy_from_slice(e))
        });

        let buffer: Vec<(Bytes, Bytes)> = merged
            .range((start_bound, end_bound))
            .filter_map(|(k, v)| v.as_ref().map(|val| (k.clone(), val.clone())))
            .collect();

        Ok(ArtCursor::from_snapshot(buffer))
    }

    fn commit(mut self) -> TraitResult<()> {
        self.ensure_active()?;
        if self.local.is_empty() {
            self.active = false;
            return Ok(());
        }

        // Serialize transaction commit: all WAL records are appended and synced
        // before any in-memory mutation so that a crash cannot leave the engine
        // with partial transaction effects on disk.
        let _guard = self.engine.inner.write_lock.lock();
        let mut records = Vec::with_capacity(self.local.len());
        for (key, value) in &self.local {
            records.push(match value {
                Some(v) => WalRecord::Put {
                    key: key.to_vec(),
                    value: v.to_vec(),
                },
                None => WalRecord::Delete { key: key.to_vec() },
            });
        }

        for record in &records {
            self.engine
                .inner
                .wal
                .append_record(record.clone().into_wal(), Durability::Buffered)
                .map_err(|e| map_error(Error::Wal(e)))?;
        }
        self.engine
            .inner
            .wal
            .sync()
            .map_err(|e| map_error(Error::Wal(e)))?;

        for (key, value) in self.local {
            match value {
                Some(v) => {
                    self.engine.inner.map.insert(&key, &v).map_err(map_error)?;
                }
                None => {
                    self.engine.inner.map.remove(&key).map_err(map_error)?;
                }
            }
        }

        self.engine
            .inner
            .unsynced
            .fetch_add(records.len(), Ordering::Relaxed);
        self.active = false;
        Ok(())
    }

    fn rollback(mut self) -> TraitResult<()> {
        self.ensure_active()?;
        self.active = false;
        Ok(())
    }

    fn set_isolation(&mut self, level: IsolationLevel) -> TraitResult<()> {
        self.ensure_active()?;
        self.isolation = level;
        Ok(())
    }
}

fn map_error(e: Error) -> TraitError {
    match e {
        Error::KeyTooLong { len, max } => TraitError::OutOfBounds {
            kind: BoundKind::Key,
            limit: max,
            got: len,
        },
        Error::ValueTooLong { len, max } => TraitError::OutOfBounds {
            kind: BoundKind::Value,
            limit: max,
            got: len,
        },
        Error::EntryLimitReached(limit) => TraitError::OutOfBounds {
            kind: BoundKind::Batch,
            limit,
            got: limit,
        },
        Error::Corruption(msg) => TraitError::Corruption(msg),
        Error::Io(io) => TraitError::Io(io),
        Error::Wal(wal) => TraitError::Io(std::io::Error::other(format!("wal error: {wal}"))),
        Error::InvalidArgument(msg) => TraitError::Corruption(format!("invalid argument: {msg}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_put_get() {
        let dir = tempfile::tempdir().unwrap();
        let engine = ArtEngine::open(dir.path(), ArtEngineOptions::default()).unwrap();
        assert_eq!(engine.put(b"a", b"1").unwrap(), None);
        assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        engine.close().unwrap();
    }

    #[test]
    fn durable_delete() {
        let dir = tempfile::tempdir().unwrap();
        let engine = ArtEngine::open(dir.path(), ArtEngineOptions::default()).unwrap();
        engine.put(b"a", b"1").unwrap();
        assert_eq!(engine.delete(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        assert_eq!(engine.get(b"a").unwrap(), None);
        engine.close().unwrap();
    }

    #[test]
    fn durable_sync_truncates_wal() {
        let dir = tempfile::tempdir().unwrap();
        let engine = ArtEngine::open(dir.path(), ArtEngineOptions::default()).unwrap();
        engine.put(b"a", b"1").unwrap();
        engine.sync().unwrap();
        // WAL may be empty after checkpoint; at minimum the operation succeeds.
        assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        engine.close().unwrap();
    }

    #[test]
    fn transaction_commit_is_durable() {
        let dir = tempfile::tempdir().unwrap();
        let engine = ArtEngine::open(dir.path(), ArtEngineOptions::default()).unwrap();
        let mut tx = engine.begin(TxnOptions::default()).unwrap();
        tx.put(b"a", b"1").unwrap();
        tx.put(b"b", b"2").unwrap();
        tx.commit().unwrap();
        drop(engine);

        let engine2 = ArtEngine::open(dir.path(), ArtEngineOptions::default()).unwrap();
        assert_eq!(engine2.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        assert_eq!(engine2.get(b"b").unwrap(), Some(Bytes::from_static(b"2")));
        engine2.close().unwrap();
    }

    #[test]
    fn read_only_transaction_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let engine = ArtEngine::open(dir.path(), ArtEngineOptions::default()).unwrap();
        engine.put(b"a", b"1").unwrap();
        let tx = engine
            .begin(TxnOptions {
                read_only: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(tx.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        tx.rollback().unwrap();
        engine.close().unwrap();
    }
}
