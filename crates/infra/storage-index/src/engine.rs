//! `IndexEngine` implementation.

use std::sync::atomic::{AtomicU32, Ordering};

use bytes::Bytes;
use parking_lot::Mutex;
use storage_traits::cursor::Cursor;
use storage_traits::engine::Engine;
use storage_traits::indexed::IndexedEngine;
use storage_traits::options::TxnOptions;
use storage_traits::transaction::Transaction;

use crate::catalog::{IndexCatalog, IndexDef, IndexId, IndexState};
use crate::cursor::{IndexCursor, IndexEntryCursor};
use crate::error::Error;
use crate::Result;
use crate::keys::{
    index_end, index_start, index_start_with, primary_key, unpack_primary_key,
};
use crate::ops::{maybe_record, update_indexes};
use crate::record::Record;

const CATALOG_KEY: &[u8] = b"__catalog__";
const NEXT_ID_KEY: &[u8] = b"__next_index_id__";

/// A durable secondary-index engine built on top of an ordered storage engine.
pub struct IndexEngine<S: Engine> {
    storage: S,
    catalog: Mutex<IndexCatalog>,
    catalog_lock: Mutex<()>,
    next_index_id: AtomicU32,
}

impl<S: Engine> IndexEngine<S> {
    /// Open or create an `IndexEngine` backed by `storage`.
    pub fn open(storage: S) -> Result<Self, S::Error> {
        let catalog: IndexCatalog = match storage.get(CATALOG_KEY)? {
            Some(bytes) => bincode::deserialize(&bytes)
                .map_err(|e| Error::CorruptCatalog(e.to_string()))?,
            None => IndexCatalog::new(),
        };

        let next_id = match storage.get(NEXT_ID_KEY)? {
            Some(bytes) => {
                if bytes.len() != 4 {
                    return Err(Error::CorruptCatalog(
                        "next index id is not 4 bytes".into(),
                    ));
                }
                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            }
            None => {
                // If the catalog is non-empty but there is no next-id record,
                // derive it from the existing indexes.
                catalog
                    .indexes
                    .values()
                    .map(|def| def.id.0)
                    .max()
                    .map(|m| m + 1)
                    .unwrap_or(1)
            }
        };

        Ok(Self {
            storage,
            catalog: Mutex::new(catalog),
            catalog_lock: Mutex::new(()),
            next_index_id: AtomicU32::new(next_id),
        })
    }

    /// Return a snapshot of the current catalog.
    pub fn catalog(&self) -> IndexCatalog {
        self.catalog.lock().clone()
    }

    /// Scan the entries of a secondary index.
    ///
    /// The returned cursor yields `(primary_key, primary_key)` pairs (the value
    /// is the primary key bytes). Use the primary key to look up the full
    /// record with `get`.
    pub fn index_scan(
        &self,
        id: IndexId,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<IndexEntryCursor<S::Cursor>, S::Error> {
        let internal_start = start
            .map(|s| index_start_with(id.0, s))
            .unwrap_or_else(|| index_start(id.0));
        let internal_end = end
            .map(|e| index_start_with(id.0, e))
            .unwrap_or_else(|| index_end(id.0));
        let cursor = self.storage.scan(Some(&internal_start), Some(&internal_end))?;
        Ok(IndexEntryCursor::new(cursor))
    }

    fn save_catalog(&self, catalog: &IndexCatalog, next_id: u32, txn: &mut S::Transaction) -> Result<(), S::Error> {
        let catalog_bytes = bincode::serialize(catalog)
            .map_err(|e| Error::CorruptCatalog(e.to_string()))?;
        txn.put(CATALOG_KEY, &catalog_bytes)?;
        txn.put(NEXT_ID_KEY, &next_id.to_be_bytes())?;
        Ok(())
    }
}

impl<S: Engine> Engine for IndexEngine<S> {
    type Error = Error<S::Error>;
    type Transaction = IndexTransaction<S::Transaction>;
    type Cursor = IndexCursor<S::Cursor>;

    fn name(&self) -> &'static str {
        "storage-index"
    }

    fn begin(&self, opts: TxnOptions) -> crate::Result<Self::Transaction, S::Error> {
        let txn = self.storage.begin(opts)?;
        Ok(IndexTransaction {
            txn,
            catalog: self.catalog.lock().clone(),
        })
    }

    fn get(&self, key: &[u8]) -> crate::Result<Option<Bytes>, S::Error> {
        Ok(self.storage.get(&primary_key(key))?)
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> crate::Result<Self::Cursor, S::Error> {
        // Scan the whole primary keyspace (TAG_PRIMARY .. TAG_INDEX) and filter
        // by the user-visible bounds in the cursor. This avoids borrowing local
        // temporary keys for the lifetime of the returned cursor.
        let cursor = self.storage.scan(
            Some(&[crate::keys::TAG_PRIMARY]),
            Some(&[crate::keys::TAG_INDEX]),
        )?;
        Ok(IndexCursor::new(
            cursor,
            start.map(Bytes::copy_from_slice),
            end.map(Bytes::copy_from_slice),
        ))
    }

    fn stats(&self) -> crate::Result<storage_traits::stats::EngineStats, S::Error> {
        Ok(self.storage.stats()?)
    }

    fn sync(&self) -> crate::Result<(), S::Error> {
        Ok(self.storage.sync()?)
    }
}

impl<S: Engine> IndexedEngine for IndexEngine<S> {
    type IndexId = IndexId;

    fn create_index(&self, name: &str, columns: &[&str]) -> crate::Result<Self::IndexId, S::Error> {
        if name.is_empty() {
            return Err(Error::InvalidArgument("index name must be non-empty".into()));
        }
        if columns.is_empty() {
            return Err(Error::InvalidArgument(
                "at least one column is required".into(),
            ));
        }

        let _guard = self.catalog_lock.lock();
        let mut txn = self.storage.begin(TxnOptions::default())?;
        let mut catalog = self.catalog.lock().clone();

        if let Some(def) = catalog.indexes.get(name)
            && def.state == IndexState::Active
        {
            return Err(Error::DuplicateName(name.to_string()));
        }

        let id = IndexId(self.next_index_id.load(Ordering::SeqCst));
        let next_id = id.0.wrapping_add(1);

        let new_def = IndexDef {
            id,
            columns: columns.iter().map(|c| (*c).to_string()).collect(),
            state: IndexState::Active,
        };

        // Backfill existing records for this new index in the same transaction.
        // This keeps the catalog and the index entries atomically consistent.
        let primary_start = primary_key(&[]);
        let primary_end = index_start(0);
        let cursor = self
            .storage
            .scan(Some(&primary_start), Some(&primary_end))?;
        for result in cursor {
            let (internal_key, value) = result?;
            let pk = unpack_primary_key(&internal_key).unwrap_or(&internal_key);
            if let Some(record) = maybe_record(&value) {
                crate::ops::backfill_record(&mut txn, &new_def, pk, &record)?;
            }
        }

        catalog.indexes.insert(name.to_string(), new_def.clone());
        self.save_catalog(&catalog, next_id, &mut txn)?;
        txn.commit()?;

        self.catalog.lock().indexes.insert(name.to_string(), new_def);
        self.next_index_id.store(next_id, Ordering::SeqCst);
        Ok(id)
    }

    fn drop_index(&self, id: Self::IndexId) -> crate::Result<(), S::Error> {
        let _guard = self.catalog_lock.lock();

        // Find the index name by id.
        let name = {
            let catalog = self.catalog.lock();
            catalog
                .indexes
                .iter()
                .find(|(_, def)| def.id == id)
                .map(|(name, _)| name.clone())
                .ok_or_else(|| Error::UnknownIndex(format!("id {:?}", id)))?
        };

        // Phase 1: mark dropping.
        {
            let mut txn = self.storage.begin(TxnOptions::default())?;
            let mut catalog = self.catalog.lock().clone();
            if let Some(def) = catalog.indexes.get_mut(&name) {
                def.state = IndexState::Dropping;
            }
            self.save_catalog(&catalog, self.next_index_id.load(Ordering::SeqCst), &mut txn)?;
            txn.commit()?;
            if let Some(def) = self.catalog.lock().indexes.get_mut(&name) {
                def.state = IndexState::Dropping;
            }
        }

        // Phase 2: delete index entries and remove catalog entry.
        {
            let mut txn = self.storage.begin(TxnOptions::default())?;
            let start = index_start(id.0);
            let end = index_end(id.0);
            let cursor = self.storage.scan(Some(&start), Some(&end))?;
            for result in cursor {
                let (key, _) = result?;
                txn.delete(&key)?;
            }

            let mut catalog = self.catalog.lock().clone();
            catalog.indexes.remove(&name);
            self.save_catalog(&catalog, self.next_index_id.load(Ordering::SeqCst), &mut txn)?;
            txn.commit()?;
            self.catalog.lock().indexes.remove(&name);
        }

        Ok(())
    }
}

/// Transaction wrapper that maintains secondary indexes on writes.
pub struct IndexTransaction<Txn: Transaction> {
    txn: Txn,
    catalog: IndexCatalog,
}

impl<Txn: Transaction> Transaction for IndexTransaction<Txn> {
    type Error = Error<Txn::Error>;

    fn get(&self, key: &[u8]) -> std::result::Result<Option<Bytes>, Self::Error> {
        Ok(self.txn.get(&primary_key(key))?)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> std::result::Result<(), Self::Error> {
        let pk = primary_key(key);
        let old_value = self.txn.get(&pk)?;
        let old_record = old_value.as_ref().and_then(maybe_record);
        let new_record = Record::decode(value);

        update_indexes(
            &mut self.txn,
            &self.catalog,
            key,
            old_record.as_ref(),
            new_record.as_ref(),
        )?;

        self.txn.put(&pk, value)?;
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> std::result::Result<(), Self::Error> {
        let pk = primary_key(key);
        let old_value = self.txn.get(&pk)?;
        if let Some(record) = old_value.as_ref().and_then(maybe_record) {
            update_indexes(&mut self.txn, &self.catalog, key, Some(&record), None)?;
        }
        self.txn.delete(&pk)?;
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> std::result::Result<impl Cursor<Error = Self::Error>, Self::Error> {
        let cursor = self.txn.scan(
            Some(&[crate::keys::TAG_PRIMARY]),
            Some(&[crate::keys::TAG_INDEX]),
        )?;
        Ok(IndexCursor::new(
            cursor,
            start.map(Bytes::copy_from_slice),
            end.map(Bytes::copy_from_slice),
        ))
    }

    fn commit(self) -> std::result::Result<(), Self::Error> {
        Ok(self.txn.commit()?)
    }

    fn rollback(self) -> std::result::Result<(), Self::Error> {
        Ok(self.txn.rollback()?)
    }

    fn set_isolation(
        &mut self,
        level: storage_traits::options::IsolationLevel,
    ) -> std::result::Result<(), Self::Error> {
        Ok(self.txn.set_isolation(level)?)
    }
}
