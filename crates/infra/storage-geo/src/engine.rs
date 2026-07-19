//! `GeoEngine` and its `storage_traits::Engine` implementation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use storage_traits::{
    BoundKind, Engine, EngineStats, Error as TraitError, Result as TraitResult, TxnOptions,
};

use crate::compaction;
use crate::cursor::GeoCursor;
use crate::error::Error;
use crate::feature::{Feature, Geometry, PropertyMap};
use crate::format::{
    decode_key, encode_feature_value, Metadata, WalRecord, INDEX_FILE, META_FILE,
};
use crate::index::{IndexBuilder, SpatialIndex};
use crate::options::GeoOptions;
use crate::query::{execute, SpatialQuery};
use crate::stats::GeoStats;
use crate::store::{FeatureAddress, FeatureStore};
use crate::transaction::GeoTransaction;
use crate::wal::GeoWal;

/// Inner engine state shared between handles and transactions.
pub(crate) struct Inner {
    pub dir: PathBuf,
    pub options: GeoOptions,
    pub metadata: RwLock<Metadata>,
    pub store: RwLock<Arc<FeatureStore>>,
    pub index: RwLock<SpatialIndex>,
    pub wal: GeoWal,
    pub write_lock: Mutex<()>,
}

/// A synchronous, durable geospatial storage engine.
#[derive(Clone)]
pub struct GeoEngine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for GeoEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoEngine")
            .field("dir", &self.inner.dir)
            .field("options", &self.inner.options)
            .finish()
    }
}

impl GeoEngine {
    /// Open or create a geospatial engine at `dir` with `options`.
    pub fn open(dir: impl AsRef<Path>, options: GeoOptions) -> crate::Result<Self> {
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

        // Persist requested options; some fields may differ across opens.
        metadata.options = options.clone();

        let store = Arc::new(FeatureStore::open(&dir, metadata.store_file_id)?);
        let wal = GeoWal::open(&dir, options.wal_sync_policy)?;

        crate::recovery::rebuild_live_map_from_store(&store, &mut metadata)?;
        crate::recovery::replay_wal(&wal, &store, &mut metadata)?;
        let index = build_initial_index(&dir, &store, &metadata)?;

        let engine = Self {
            inner: Arc::new(Inner {
                dir,
                options,
                metadata: RwLock::new(metadata.clone()),
                store: RwLock::new(store),
                index: RwLock::new(index),
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

    /// Insert or replace a feature.
    pub fn insert_feature(
        &self,
        id: impl Into<Vec<u8>>,
        geometry: Geometry,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        let id = id.into();
        if id.len() > self.inner.options.max_key_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Key,
                limit: self.inner.options.max_key_len,
                got: id.len(),
            }
            .into());
        }
        geometry.validate()?;
        let _guard = self.inner.write_lock.lock();
        let feature = Feature::new(id, geometry, properties);
        self.insert_feature_unlocked(feature)
    }

    pub(crate) fn insert_feature_unlocked(&self, feature: Feature) -> crate::Result<()> {
        self.inner.wal.append(WalRecord::InsertFeature {
            id: feature.id.clone(),
            geometry: crate::wkb::encode(&feature.geometry)?,
            properties: serde_json::to_vec(&feature.properties)
                .map_err(|e| Error::property_encoding(e.to_string()))?,
        })?;
        let address = {
            let store = self.inner.store.read();
            store.insert(&feature)?
        };
        {
            let mut meta = self.inner.metadata.write();
            if let Some(old) = meta.live.insert(feature.id.clone(), address) {
                meta.stale_bytes += old.len as u64;
            }
        }
        {
            let mut index = self.inner.index.write();
            index.delete(&feature.id);
            index.insert(&feature, address);
        }
        Ok(())
    }

    /// Delete a feature by id.
    pub fn delete_feature(&self, id: &[u8]) -> crate::Result<bool> {
        let _guard = self.inner.write_lock.lock();
        self.delete_feature_unlocked(id)
    }

    pub(crate) fn delete_feature_unlocked(&self, id: &[u8]) -> crate::Result<bool> {
        let old = {
            let mut meta = self.inner.metadata.write();
            meta.live.remove(id)
        };
        match old {
            Some(address) => {
                self.inner.wal.append(WalRecord::DeleteFeature { id: id.to_vec() })?;
                {
                    let mut meta = self.inner.metadata.write();
                    meta.stale_bytes += address.len as u64;
                }
                {
                    let mut index = self.inner.index.write();
                    index.delete(id);
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Update a single property of a feature.
    pub fn update_property(
        &self,
        id: &[u8],
        property_key: &str,
        value: Vec<u8>,
    ) -> crate::Result<bool> {
        if property_key.is_empty() {
            return Err(Error::invalid_argument("property key must not be empty"));
        }
        if value.len() > self.inner.options.max_value_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Value,
                limit: self.inner.options.max_value_len,
                got: value.len(),
            }
            .into());
        }
        let _guard = self.inner.write_lock.lock();
        self.update_properties_unlocked(id, property_key, value)
    }

    pub(crate) fn update_properties_unlocked(
        &self,
        id: &[u8],
        property_key: &str,
        value: Vec<u8>,
    ) -> crate::Result<bool> {
        let mut feature = match self.get_feature(id)? {
            Some(f) => f,
            None => return Ok(false),
        };
        feature
            .properties
            .insert(property_key.to_string(), value.clone());
        self.inner.wal.append(WalRecord::UpdateProperties {
            id: id.to_vec(),
            properties: serde_json::to_vec(&feature.properties)
                .map_err(|e| Error::property_encoding(e.to_string()))?,
        })?;
        let address = {
            let store = self.inner.store.read();
            store.insert(&feature)?
        };
        {
            let mut meta = self.inner.metadata.write();
            if let Some(old) = meta.live.insert(id.to_vec(), address) {
                meta.stale_bytes += old.len as u64;
            }
        }
        {
            let mut index = self.inner.index.write();
            index.delete(id);
            index.insert(&feature, address);
        }
        Ok(true)
    }

    /// Fetch a feature by id.
    pub fn get_feature(&self, id: &[u8]) -> crate::Result<Option<Feature>> {
        let address = {
            let meta = self.inner.metadata.read();
            meta.live.get(id).copied()
        };
        match address {
            Some(address) => self.inner.store.read().get(address),
            None => Ok(None),
        }
    }

    /// Fetch a single property value by feature id and property key.
    pub fn get_property(&self, id: &[u8], property_key: &str) -> crate::Result<Option<Vec<u8>>> {
        let feature = self.get_feature(id)?;
        Ok(feature.and_then(|f| f.properties.get(property_key).cloned()))
    }

    /// Execute a spatial query.
    pub fn query(&self, query: &SpatialQuery) -> crate::Result<Vec<Feature>> {
        let index = self.inner.index.read();
        let store = self.inner.store.read();
        execute(&index, &store, query)
    }

    /// Flush store, rebuild/persist index, write metadata, and checkpoint WAL.
    pub fn sync(&self) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        self.inner.store.read().sync()?;
        self.maybe_compact()?;
        self.rebuild_and_persist_index()?;
        self.persist_meta_with_checkpoint()?;
        self.inner.wal.sync()?;
        self.inner.wal.truncate_completed()?;
        Ok(())
    }

    fn maybe_compact(&self) -> crate::Result<()> {
        let (should_compact, store) = {
            let meta = self.inner.metadata.read();
            let live_bytes: u64 = meta.live.values().map(|a| a.len as u64).sum();
            let total = live_bytes + meta.stale_bytes;
            let threshold = self.inner.options.compaction_threshold;
            let should = total > 0 && (meta.stale_bytes as f64) / (total as f64) > threshold;
            (should, self.inner.store.read().clone())
        };
        if should_compact {
            let mut meta = self.inner.metadata.write();
            let new_store = compaction::compact(&self.inner.dir, &mut meta, &store)?;
            let new_store = Arc::new(new_store);
            *self.inner.store.write() = new_store.clone();
            // Rebuild the index from the compacted store.
            drop(meta);
            self.rebuild_and_persist_index()?;
        }
        Ok(())
    }

    /// Rebuild the R-tree from all live features and persist it to disk.
    fn rebuild_and_persist_index(&self) -> crate::Result<()> {
        let (features, addresses): (Vec<Feature>, Vec<FeatureAddress>) = {
            let meta = self.inner.metadata.read();
            let store = self.inner.store.read();
            let mut features = Vec::with_capacity(meta.live.len());
            let mut addresses = Vec::with_capacity(meta.live.len());
            for (id, address) in meta.live.iter() {
                if let Some(feature) = store.get(*address)? {
                    features.push(feature);
                    addresses.push(*address);
                } else {
                    return Err(Error::corruption(format!(
                        "feature {} missing from store at {:?}",
                        String::from_utf8_lossy(id),
                        address
                    )));
                }
            }
            (features, addresses)
        };

        let index = IndexBuilder::build(features.iter().zip(addresses));
        let encoded = index.encode()?;
        storage_file::atomic_write(&self.inner.dir.join(INDEX_FILE), &encoded)?;
        *self.inner.index.write() = index;
        Ok(())
    }

    /// Persist metadata and write a WAL checkpoint.
    fn persist_meta_with_checkpoint(&self) -> crate::Result<()> {
        let mut meta = self.inner.metadata.write();
        let lsn = self.inner.wal.checkpoint(&meta)?;
        meta.wal_checkpoint_lsn = Some(lsn);
        let encoded = meta.encode()?;
        storage_file::atomic_write(&self.inner.dir.join(META_FILE), &encoded)?;
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
    pub fn stats(&self) -> crate::Result<GeoStats> {
        let meta = self.inner.metadata.read();
        let index = self.inner.index.read();
        let num_features = meta.live.len() as u64;
        let memory_bytes = meta
            .live
            .iter()
            .map(|(k, a)| (k.len() + std::mem::size_of::<FeatureAddress>() + a.len as usize) as u64)
            .sum();
        Ok(GeoStats {
            name: "storage-geo",
            num_features,
            disk_bytes: approx_dir_bytes(&self.inner.dir)?,
            memory_bytes,
            metrics: {
                let mut m = std::collections::HashMap::new();
                m.insert("index_len".into(), index.len() as u64);
                m.insert("stale_bytes".into(), meta.stale_bytes);
                m.insert("max_key_len".into(), self.inner.options.max_key_len as u64);
                m
            },
        })
    }
}

fn build_initial_index(
    dir: &Path,
    store: &FeatureStore,
    metadata: &Metadata,
) -> crate::Result<SpatialIndex> {
    let index_path = dir.join(INDEX_FILE);
    if index_path.exists() {
        let bytes = std::fs::read(&index_path)?;
        if let Ok(index) = SpatialIndex::decode(&bytes) {
            return Ok(index);
        }
    }

    let mut features = Vec::with_capacity(metadata.live.len());
    let mut addresses = Vec::with_capacity(metadata.live.len());
    for (id, address) in metadata.live.iter() {
        match store.get(*address)? {
            Some(feature) => {
                features.push(feature);
                addresses.push(*address);
            }
            None => {
                return Err(Error::corruption(format!(
                    "feature {} missing from store at {:?}",
                    String::from_utf8_lossy(id),
                    address
                )));
            }
        }
    }
    Ok(IndexBuilder::build(features.iter().zip(addresses)))
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

impl Engine for GeoEngine {
    type Error = Error;
    type Transaction = GeoTransaction;
    type Cursor = GeoCursor;

    fn name(&self) -> &'static str {
        "storage-geo"
    }

    fn begin(&self, opts: TxnOptions) -> TraitResult<Self::Transaction, Self::Error> {
        Ok(GeoTransaction::new(self.clone(), opts))
    }

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        let (id, property) = decode_key(key)?;
        match property {
            Some(property_key) => Ok(self
                .get_property(id, property_key)?
                .map(Bytes::from)),
            None => {
                let feature = self.get_feature(id)?;
                Ok(match feature {
                    Some(f) => Some(Bytes::from(encode_feature_value(&f)?)),
                    None => None,
                })
            }
        }
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<Self::Cursor, Self::Error> {
        let meta = self.inner.metadata.read();
        let mut map: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let store = self.inner.store.read();
        for (id, address) in meta.live.iter() {
            let include = {
                let above_start = start.map(|s| id.as_slice() >= s).unwrap_or(true);
                let below_end = end.map(|e| id.as_slice() < e).unwrap_or(true);
                above_start && below_end
            };
            if include && let Some(feature) = store.get(*address)? {
                map.insert(id.clone(), encode_feature_value(&feature)?);
            }
        }
        Ok(GeoCursor::new(map))
    }

    fn stats(&self) -> TraitResult<EngineStats, Self::Error> {
        let s = self.stats()?;
        Ok(s.into_engine_stats())
    }

    fn sync(&self) -> TraitResult<(), Self::Error> {
        GeoEngine::sync(self)
    }
}

