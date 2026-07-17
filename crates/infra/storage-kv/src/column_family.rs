//! Column-family support.
//!
//! A column family is an independent keyspace with its own MemTables, immutable
//! queue, SSTables, block cache, and compaction state, sharing the WAL,
//! sequence allocator, manifest, and background workers with other CFs.  This is
//! the RocksDB model.
//!
//! The default column family (id 0, name "default") always exists.  The trait
//! `Engine` methods operate on it; `LsmEngine` additionally exposes CF-aware
//! methods that take a `ColumnFamilyHandle`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::cache::{BlockCache, BlockCaches};
use crate::file_number::FileNumberAllocator;
use crate::immutable::ImmutableMemTables;
use crate::memtable::MemTable;
use crate::metrics::Metrics;
use crate::obsolete_files::ObsoleteFiles;
use crate::options::LsmOptions;
use crate::version_set::VersionSet;
use crate::{FileNumber, Result};

/// Opaque identifier for a column family.
pub type ColumnFamilyId = u32;

/// Per-column-family state.  Equivalent to a standalone LSM tree except that it
/// shares the WAL and global sequence allocator with the other CFs.
pub struct ColumnFamily {
    pub id: ColumnFamilyId,
    pub name: String,
    pub options: LsmOptions,
    pub memtable: Mutex<Arc<MemTable>>,
    pub immutable: ImmutableMemTables,
    pub version_set: Arc<VersionSet>,
    pub caches: BlockCaches,
    pub metrics: Arc<Metrics>,
    pub active_flushes: usize,
    pub active_compactions: usize,
    pub obsolete_files: ObsoleteFiles,
    /// Serializes MemTable freezes for this column family so that immutable-queue
    /// order always matches version order, even when a freezer stalls waiting for
    /// the background worker to drain a full queue.
    pub freeze_lock: Arc<Mutex<()>>,
}

impl ColumnFamily {
    /// Create the default column family.
    pub fn default(options: LsmOptions) -> Self {
        Self::new(0, "default", options, FileNumberAllocator::default())
    }

    /// Create a new column family sharing a global file-number allocator.
    pub fn new(
        id: ColumnFamilyId,
        name: &str,
        options: LsmOptions,
        file_numbers: FileNumberAllocator,
    ) -> Self {
        let version_set = Arc::new(VersionSet::with_allocator(options.num_levels, file_numbers));
        let metrics = Arc::new(Metrics::default());
        Self {
            id,
            name: name.to_string(),
            options: options.clone(),
            memtable: Mutex::new(Arc::new(MemTable::new())),
            immutable: ImmutableMemTables::new(
                options.max_write_buffer_number.saturating_sub(1).max(1),
            ),
            version_set,
            caches: BlockCaches::new(
                Arc::new(BlockCache::with_capacity(options.block_cache_size)),
                (options.compressed_block_cache_size > 0).then(|| {
                    Arc::new(BlockCache::with_capacity(
                        options.compressed_block_cache_size,
                    ))
                }),
                Arc::clone(&metrics),
            ),
            metrics,
            active_flushes: 0,
            active_compactions: 0,
            obsolete_files: ObsoleteFiles::new(),
            freeze_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Approximate total byte size of the mutable + immutable MemTables.
    pub fn approximate_memtable_size(&self) -> usize {
        let mut size = self.memtable.lock().unwrap().approximate_size();
        size += self.immutable.approximate_size();
        size
    }

    /// Return the next file number that will be assigned.
    pub fn next_file_number(&self) -> FileNumber {
        self.version_set.next_file_number()
    }
}

/// Lightweight cloneable handle to a column family.
#[derive(Debug, Clone)]
pub struct ColumnFamilyHandle {
    pub(crate) id: ColumnFamilyId,
    pub(crate) name: String,
}

impl Default for ColumnFamilyHandle {
    fn default() -> Self {
        Self {
            id: 0,
            name: "default".to_string(),
        }
    }
}

impl ColumnFamilyHandle {
    pub fn id(&self) -> ColumnFamilyId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Set of column families keyed by id and name.
pub struct ColumnFamilySet {
    by_id: HashMap<ColumnFamilyId, ColumnFamily>,
    by_name: HashMap<String, ColumnFamilyId>,
    next_id: ColumnFamilyId,
    /// Global file-number allocator shared by every column family so that all
    /// SSTable files in the same database directory have unique numbers.
    file_numbers: FileNumberAllocator,
}

impl ColumnFamilySet {
    /// Create a set containing only the default column family.
    pub fn with_default(options: LsmOptions) -> Self {
        Self::with_allocator(options, FileNumberAllocator::default())
    }

    /// Create a set containing only the default column family, using an explicit
    /// starting file number.  Useful during recovery.
    pub fn with_allocator(options: LsmOptions, file_numbers: FileNumberAllocator) -> Self {
        let mut set = Self {
            by_id: HashMap::new(),
            by_name: HashMap::new(),
            next_id: 1, // 0 reserved for default
            file_numbers: file_numbers.clone(),
        };
        let default = ColumnFamily::new(0, "default", options, file_numbers);
        set.by_id.insert(default.id, default);
        set.by_name.insert("default".to_string(), 0);
        set
    }

    /// Return a mutable reference to the default column family.
    pub(crate) fn default_mut(&mut self) -> &mut ColumnFamily {
        self.by_id
            .get_mut(&0)
            .expect("default column family always exists")
    }

    /// Return a reference to the default column family.
    pub fn default(&self) -> &ColumnFamily {
        self.by_id
            .get(&0)
            .expect("default column family always exists")
    }

    /// Return a reference to a column family by id.
    pub fn get(&self, id: ColumnFamilyId) -> Option<&ColumnFamily> {
        self.by_id.get(&id)
    }

    /// Return a mutable reference to a column family by id.
    pub(crate) fn get_mut(&mut self, id: ColumnFamilyId) -> Option<&mut ColumnFamily> {
        self.by_id.get_mut(&id)
    }

    /// Return a reference to a column family by name.
    pub fn get_by_name(&self, name: &str) -> Option<&ColumnFamily> {
        self.by_name.get(name).and_then(|id| self.by_id.get(id))
    }

    /// Return a handle for the default column family.
    pub fn default_handle(&self) -> ColumnFamilyHandle {
        ColumnFamilyHandle {
            id: 0,
            name: "default".to_string(),
        }
    }

    /// Return a handle for a column family by name, if it exists.
    pub fn handle(&self, name: &str) -> Option<ColumnFamilyHandle> {
        self.by_name.get(name).map(|id| ColumnFamilyHandle {
            id: *id,
            name: name.to_string(),
        })
    }

    /// Create a new column family and return its handle.
    ///
    /// Returns an error if the name is already in use.
    pub fn create(&mut self, name: &str, options: LsmOptions) -> Result<ColumnFamilyHandle> {
        let id = self.next_id;
        self.next_id += 1;
        self.create_with_id(id, name, options)
    }

    /// Create a column family with a specific id.  Used during recovery so that
    /// column-family ids stay stable across restarts.
    pub(crate) fn create_with_id(
        &mut self,
        id: ColumnFamilyId,
        name: &str,
        options: LsmOptions,
    ) -> Result<ColumnFamilyHandle> {
        if id == 0 {
            return Err(crate::Error::InvalidArgument(
                "column family id 0 is reserved".into(),
            ));
        }
        if self.by_name.contains_key(name) {
            return Err(crate::Error::InvalidArgument(format!(
                "column family '{}' already exists",
                name
            )));
        }
        if self.by_id.contains_key(&id) {
            return Err(crate::Error::InvalidArgument(format!(
                "column family id {} already exists",
                id
            )));
        }
        if name == "default" {
            return Err(crate::Error::InvalidArgument(
                "column family name 'default' is reserved".into(),
            ));
        }
        self.next_id = self.next_id.max(id + 1);
        let cf = ColumnFamily::new(id, name, options, self.file_numbers.clone());
        self.by_id.insert(id, cf);
        self.by_name.insert(name.to_string(), id);
        Ok(ColumnFamilyHandle {
            id,
            name: name.to_string(),
        })
    }

    /// Drop a column family by id.  Returns an error if it is the default CF.
    pub fn drop(&mut self, id: ColumnFamilyId) -> Result<()> {
        let _ = self.remove(id)?;
        Ok(())
    }

    /// Remove a column family by id and return it.  Used when the caller needs
    /// to keep the CF's metadata around (e.g. to clean up its files after drop).
    pub(crate) fn remove(&mut self, id: ColumnFamilyId) -> Result<ColumnFamily> {
        if id == 0 {
            return Err(crate::Error::InvalidArgument(
                "cannot drop the default column family".into(),
            ));
        }
        let cf = self
            .by_id
            .remove(&id)
            .ok_or_else(|| crate::Error::InvalidArgument("column family not found".into()))?;
        self.by_name.remove(&cf.name);
        Ok(cf)
    }

    /// Return the set of all file numbers referenced by any live `Version` in
    /// any column family.
    pub(crate) fn live_file_numbers(&self) -> HashSet<FileNumber> {
        let mut live = HashSet::new();
        for cf in self.iter() {
            live.extend(cf.version_set.live_file_numbers());
        }
        live
    }

    /// Iterate over all column families.
    pub fn iter(&self) -> impl Iterator<Item = &ColumnFamily> {
        self.by_id.values()
    }

    /// Return the number of column families.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> LsmOptions {
        LsmOptions::default()
    }

    #[test]
    fn default_family_exists() {
        let set = ColumnFamilySet::with_default(opts());
        assert_eq!(set.len(), 1);
        assert_eq!(set.default().name, "default");
        assert_eq!(set.default_handle().id(), 0);
    }

    #[test]
    fn create_and_lookup() {
        let mut set = ColumnFamilySet::with_default(opts());
        let h = set.create("cf1", opts()).unwrap();
        assert_eq!(h.id(), 1);
        assert_eq!(h.name(), "cf1");
        assert!(set.handle("cf1").is_some());
        assert_eq!(set.get(1).unwrap().name, "cf1");
    }

    #[test]
    fn duplicate_name_fails() {
        let mut set = ColumnFamilySet::with_default(opts());
        set.create("cf1", opts()).unwrap();
        assert!(set.create("cf1", opts()).is_err());
    }

    #[test]
    fn cannot_drop_default() {
        let mut set = ColumnFamilySet::with_default(opts());
        assert!(set.drop(0).is_err());
    }

    #[test]
    fn drop_family() {
        let mut set = ColumnFamilySet::with_default(opts());
        let h = set.create("cf1", opts()).unwrap();
        set.drop(h.id()).unwrap();
        assert!(set.handle("cf1").is_none());
        assert_eq!(set.len(), 1);
    }
}
