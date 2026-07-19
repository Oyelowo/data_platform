//! Transaction implementation for `storage-search`.

use std::collections::{BTreeMap, HashMap};

use bytes::Bytes;
use storage_traits::{Cursor, Engine, IsolationLevel, Result as TraitResult, Transaction, TxnOptions};

use crate::cursor::SearchCursor;
use crate::document::Document;
use crate::engine::SearchEngine;
use crate::error::Error;
use crate::format::{decode_engine_key, decode_field_value};

/// A transaction over a [`SearchEngine`].
pub struct SearchTransaction {
    engine: SearchEngine,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
    local_puts: BTreeMap<Vec<u8>, Vec<u8>>,
    local_deletes: HashMap<Vec<u8>, ()>,
    local_index: BTreeMap<Vec<u8>, Document>,
}

impl SearchTransaction {
    pub(crate) fn new(engine: SearchEngine, opts: TxnOptions) -> Self {
        Self {
            engine,
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
            local_puts: BTreeMap::new(),
            local_deletes: HashMap::new(),
            local_index: BTreeMap::new(),
        }
    }

    fn ensure_active(&self) -> crate::Result<()> {
        if !self.active {
            return Err(Error::InactiveTransaction);
        }
        Ok(())
    }
}

impl Transaction for SearchTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        self.ensure_active()?;
        if self.local_deletes.contains_key(key) {
            return Ok(None);
        }
        if let Some(v) = self.local_puts.get(key) {
            return Ok(Some(Bytes::from(v.clone())));
        }
        self.engine.get(key)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_puts.insert(key.to_vec(), value.to_vec());
        self.local_deletes.remove(key);

        // If the key encodes (doc_id, field), also stage a document update.
        if let Ok((doc_id, field_name)) = decode_engine_key(key) {
            let value = decode_field_value(value)?;
            let mut doc = self
                .local_index
                .remove(doc_id)
                .or_else(|| self.engine.get_document(doc_id).unwrap_or(None))
                .unwrap_or_default();
            doc.fields.insert(field_name.to_string(), value);
            self.local_index.insert(doc_id.to_vec(), doc);
        }

        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_deletes.insert(key.to_vec(), ());
        self.local_puts.remove(key);
        if let Ok((doc_id, _)) = decode_engine_key(key) {
            self.local_index.remove(doc_id);
        }
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<impl Cursor<Error = Self::Error>, Self::Error> {
        self.ensure_active()?;
        let mut map: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

        let engine_map = self.engine.scan(start, end)?;
        for item in engine_map {
            let (k, v) = item?;
            map.insert(k.to_vec(), v.to_vec());
        }

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

        Ok(SearchCursor::from_map(map))
    }

    fn commit(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        let _guard = self.engine.inner().write_lock.lock();
        for (key, _) in self.local_deletes {
            self.engine.delete_unlocked(&key)?;
        }
        for (key, value) in self.local_puts {
            self.engine.put_unlocked(&key, &value)?;
        }
        for (doc_id, doc) in self.local_index {
            self.engine.index_document_unlocked(&doc_id, doc)?;
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
