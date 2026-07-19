//! `VectorEngine` and its `storage_traits::Engine` implementation.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use storage_traits::{
    BoundKind, Cursor, Engine, EngineStats, Error as TraitError, IsolationLevel, Result as TraitResult,
    Transaction, TxnOptions,
};

use crate::cursor::VectorCursor;
use crate::error::Error;
use crate::format::{decode_f32_vec, encode_f32_vec, Metadata, VectorRecord, WalRecord, META_FILE};
use crate::index::{BruteForceIndex, HnswIndex, IvfIndex, SearchResult, VectorIndex};
use crate::options::{IndexType, VectorOptions};
use crate::recovery;
use crate::stats::VectorStats;
use crate::storage::VectorStorage;
use crate::wal::VectorWal;

/// Inner engine state shared between the public handle and transactions.
struct Inner {
    dir: PathBuf,
    options: VectorOptions,
    metadata: RwLock<Metadata>,
    storage: Arc<VectorStorage>,
    wal: VectorWal,
    index: RwLock<Box<dyn VectorIndex>>,
    /// Protects writers so that only one transaction commits at a time.
    write_lock: Mutex<()>,
}

/// A synchronous, durable vector storage engine.
#[derive(Clone)]
pub struct VectorEngine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for VectorEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorEngine")
            .field("dir", &self.inner.dir)
            .field("options", &self.inner.options)
            .finish()
    }
}

impl VectorEngine {
    /// Open or create a vector engine at `dir` with `options`.
    pub fn open(dir: impl AsRef<Path>, options: VectorOptions) -> crate::Result<Self> {
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

        // Validate on-disk options match requested options.
        if metadata.options.dimension != options.dimension {
            return Err(Error::InvalidArgument(
                "cannot open engine with different dimension".into(),
            ));
        }
        if metadata.options.metric != options.metric {
            return Err(Error::InvalidArgument(
                "cannot open engine with different metric".into(),
            ));
        }
        // Persist the requested options (index type may differ across opens).
        metadata.options = options.clone();

        let storage = Arc::new(VectorStorage::new(options.clone(), &dir));
        let mut index: Box<dyn VectorIndex> = match options.index_type {
            IndexType::BruteForce => Box::new(BruteForceIndex::new(options.metric)),
            IndexType::Hnsw => Box::new(HnswIndex::new(options.metric, options.hnsw)),
            IndexType::Ivf => Box::new(IvfIndex::new(options.metric, options.ivf)),
        };

        let wal = VectorWal::open(&dir)?;
        recovery::recover(&wal, &storage, &mut *index, &mut metadata)?;

        let engine = Self {
            inner: Arc::new(Inner {
                dir,
                options,
                metadata: RwLock::new(metadata),
                storage,
                wal,
                index: RwLock::new(index),
                write_lock: Mutex::new(()),
            }),
        };
        engine.persist_meta()?;
        Ok(engine)
    }

    /// Insert or replace a vector under `key`.
    pub fn put(&self, key: &[u8], vector: &[f32]) -> crate::Result<()> {
        if key.len() > self.inner.options.max_key_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Key,
                limit: self.inner.options.max_key_len,
                got: key.len(),
            }
            .into());
        }
        if vector.len() != self.inner.options.dimension {
            return Err(Error::dimension_mismatch(
                self.inner.options.dimension,
                vector.len(),
            ));
        }

        let _guard = self.inner.write_lock.lock();
        self.put_unlocked(key, vector)
    }

    fn put_unlocked(&self, key: &[u8], vector: &[f32]) -> crate::Result<()> {
        let (id, is_new) = {
            let mut meta = self.inner.metadata.write();
            match meta.key_to_id.get(key).copied() {
                Some(id) => (id, false),
                None => {
                    let id = meta.next_id;
                    meta.next_id += 1;
                    meta.key_to_id.insert(key.to_vec(), id);
                    (id, true)
                }
            }
        };

        let record = VectorRecord {
            id,
            key: key.to_vec(),
            vector: vector.to_vec(),
        };
        self.inner.wal.append(WalRecord::Put {
            key: key.to_vec(),
            vector: vector.to_vec(),
        })?;
        self.inner.storage.put(record.clone())?;
        {
            let mut index = self.inner.index.write();
            if !is_new {
                index.delete(id);
            }
            index.insert(&record);
        }
        Ok(())
    }

    /// Delete the vector under `key`.
    pub fn delete(&self, key: &[u8]) -> crate::Result<bool> {
        let _guard = self.inner.write_lock.lock();
        self.delete_unlocked(key)
    }

    fn delete_unlocked(&self, key: &[u8]) -> crate::Result<bool> {
        let id = {
            let mut meta = self.inner.metadata.write();
            meta.key_to_id.remove(key)
        };
        match id {
            Some(id) => {
                self.inner
                    .wal
                    .append(WalRecord::Delete { key: key.to_vec() })?;
                self.inner.storage.delete(id);
                self.inner.index.write().delete(id);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Get the vector stored under `key`.
    pub fn get(&self, key: &[u8]) -> crate::Result<Option<Vec<f32>>> {
        let meta = self.inner.metadata.read();
        let id = match meta.key_to_id.get(key).copied() {
            Some(id) => id,
            None => return Ok(None),
        };
        drop(meta);
        Ok(self.inner.storage.get_vector(id))
    }

    /// Return the user key for an internal vector id, if any.
    pub fn key_by_id(&self, id: u64) -> Option<Vec<u8>> {
        let meta = self.inner.metadata.read();
        meta.key_to_id
            .iter()
            .find(|&(_, &v)| v == id)
            .map(|(k, _)| k.clone())
    }

    /// Search for the `k` nearest neighbors of `query`.
    pub fn search(&self, query: &[f32], k: usize) -> crate::Result<Vec<SearchResult>> {
        if query.len() != self.inner.options.dimension {
            return Err(Error::dimension_mismatch(
                self.inner.options.dimension,
                query.len(),
            ));
        }
        if k == 0 {
            return Ok(Vec::new());
        }

        // Small datasets are faster and more accurate with an exact scan.
        let threshold = self.inner.options.brute_force_threshold;
        if threshold > 0 && self.inner.storage.len() <= threshold {
            return Ok(self.brute_force_search(query, k));
        }

        let ef = match self.inner.options.index_type {
            IndexType::Hnsw => self.inner.options.hnsw.ef_search,
            _ => k,
        };
        let index = self.inner.index.read();
        Ok(index.search(query, k, ef))
    }

    fn brute_force_search(&self, query: &[f32], k: usize) -> Vec<SearchResult> {
        use crate::distance::{cosine_distance, euclidean, neg_dot_product};
        let distance_fn: fn(&[f32], &[f32]) -> f32 = match self.inner.options.metric {
            crate::distance::DistanceMetric::Euclidean => euclidean,
            crate::distance::DistanceMetric::Cosine => cosine_distance,
            crate::distance::DistanceMetric::DotProduct => neg_dot_product,
        };
        let mut heap = std::collections::BinaryHeap::new();
        for rec in self.inner.storage.records() {
            let dist = distance_fn(query, &rec.vector);
            heap.push(crate::index::SearchResult {
                id: rec.id,
                distance: dist,
            });
            if heap.len() > k {
                heap.pop();
            }
        }
        let mut results: Vec<crate::index::SearchResult> = heap.into_iter().collect();
        results.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        results
    }

    /// Return engine statistics.
    pub fn stats(&self) -> crate::Result<VectorStats> {
        let meta = self.inner.metadata.read();
        let index = self.inner.index.read();
        let storage_len = self.inner.storage.len();
        Ok(VectorStats {
            name: "storage-vector",
            num_vectors: storage_len as u64,
            dimension: self.inner.options.dimension as u64,
            disk_bytes: approx_dir_bytes(&self.inner.dir)?,
            memory_bytes: (storage_len * self.inner.options.dimension * 4) as u64,
            metrics: {
                let mut m = HashMap::new();
                m.insert("index_len".into(), index.len() as u64);
                m.insert("next_id".into(), meta.next_id);
                m
            },
        })
    }

    /// Flush vector pages, metadata, and WAL.
    pub fn sync(&self) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        self.persist_meta()?;
        self.inner.storage.flush()?;
        self.inner.wal.sync()?;
        Ok(())
    }

    /// Persist the current metadata file.
    pub fn persist_meta(&self) -> crate::Result<()> {
        let meta = self.inner.metadata.read();
        let encoded = meta.encode()?;
        storage_file::atomic_write(&self.inner.dir.join(META_FILE), &encoded)?;
        Ok(())
    }

    /// Close the engine, releasing the WAL lock.
    pub fn close(&self) -> crate::Result<()> {
        self.sync()?;
        self.inner.wal.close()?;
        Ok(())
    }
}

fn approx_dir_bytes(dir: &Path) -> crate::Result<u64> {
    let mut total = 0u64;
    if let Ok(entries) = walkdir::read_dir(dir) {
        for entry in entries {
            if let Ok(md) = entry.metadata() {
                total += md.len();
            }
        }
    }
    Ok(total)
}

impl Engine for VectorEngine {
    type Error = Error;
    type Transaction = VectorTransaction;
    type Cursor = VectorCursor;

    fn name(&self) -> &'static str {
        "storage-vector"
    }

    fn begin(&self, opts: TxnOptions) -> TraitResult<Self::Transaction, Self::Error> {
        Ok(VectorTransaction {
            engine: self.clone(),
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
            local_puts: BTreeMap::new(),
            local_deletes: HashMap::new(),
        })
    }

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        match VectorEngine::get(self, key)? {
            Some(v) => Ok(Some(Bytes::from(encode_f32_vec(&v)))),
            None => Ok(None),
        }
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<Self::Cursor, Self::Error> {
        let meta = self.inner.metadata.read();
        let mut map: BTreeMap<Vec<u8>, Vec<f32>> = BTreeMap::new();
        for (key, &id) in meta.key_to_id.iter() {
            if let Some(vector) = self.inner.storage.get_vector(id) {
                let include = {
                    let above_start = start.map(|s| key.as_slice() >= s).unwrap_or(true);
                    let below_end = end.map(|e| key.as_slice() < e).unwrap_or(true);
                    above_start && below_end
                };
                if include {
                    map.insert(key.clone(), vector);
                }
            }
        }
        Ok(VectorCursor::new(map))
    }

    fn stats(&self) -> TraitResult<EngineStats, Self::Error> {
        let s = self.stats()?;
        Ok(EngineStats {
            name: s.name,
            disk_bytes: s.disk_bytes,
            memory_bytes: s.memory_bytes,
            num_keys: Some(s.num_vectors),
            metrics: s.metrics,
        })
    }

    fn sync(&self) -> TraitResult<(), Self::Error> {
        VectorEngine::sync(self)
    }
}

/// A transaction over a [`VectorEngine`].
pub struct VectorTransaction {
    engine: VectorEngine,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
    local_puts: BTreeMap<Vec<u8>, Vec<f32>>,
    local_deletes: HashMap<Vec<u8>, ()>,
}

impl VectorTransaction {
    fn ensure_active(&self) -> crate::Result<()> {
        if !self.active {
            return Err(Error::InactiveTransaction);
        }
        Ok(())
    }
}

impl Transaction for VectorTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        self.ensure_active()?;
        if self.local_deletes.contains_key(key) {
            return Ok(None);
        }
        if let Some(v) = self.local_puts.get(key) {
            return Ok(Some(Bytes::from(encode_f32_vec(v))));
        }
        match self.engine.get(key)? {
            Some(v) => Ok(Some(Bytes::from(encode_f32_vec(&v)))),
            None => Ok(None),
        }
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        let vector = decode_f32_vec(value)?;
        self.local_puts.insert(key.to_vec(), vector);
        self.local_deletes.remove(key);
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_deletes.insert(key.to_vec(), ());
        self.local_puts.remove(key);
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<impl Cursor<Error = Self::Error>, Self::Error> {
        self.ensure_active()?;
        let mut map: BTreeMap<Vec<u8>, Vec<f32>> = BTreeMap::new();

        // Start from engine state.
        let engine_map = self.engine.scan(start, end)?;
        for item in engine_map {
            let (k, v) = item?;
            map.insert(k.to_vec(), decode_f32_vec(&v)?);
        }

        // Apply local deletes and puts.
        for k in self.local_deletes.keys() {
            map.remove(k);
        }
        for (k, v) in &self.local_puts {
            let include = {
                let above_start = start.map(|s| k.as_slice() >= s).unwrap_or(true);
                let below_end = end.map(|e| k.as_slice() < e).unwrap_or(true);
                above_start && below_end
            };
            if include {
                map.insert(k.clone(), v.clone());
            }
        }
        Ok(VectorCursor::new(map))
    }

    fn commit(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        let _guard = self.engine.inner.write_lock.lock();
        for (key, _) in self.local_deletes {
            self.engine.delete_unlocked(&key)?;
        }
        for (key, vector) in self.local_puts {
            self.engine.put_unlocked(&key, &vector)?;
        }
        self.active = false;
        Ok(())
    }

    fn rollback(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        self.active = false;
        Ok(())
    }

    fn set_isolation(&mut self, level: IsolationLevel) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        self.isolation = level;
        Ok(())
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

// walkdir is not a dependency; use std::fs::read_dir with recursion.
mod walkdir {
    use std::path::Path;

    pub fn read_dir(path: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
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
}
