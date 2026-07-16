//! In-memory transaction implementation.

use bytes::Bytes;
use std::collections::BTreeMap;
use std::sync::Arc;

use storage_traits::{Cursor, Error, IsolationLevel, Result, Transaction, TxnOptions};

use crate::cursor::MemoryCursor;
use crate::engine::MemoryEngine;

/// A transaction over an in-memory engine.
///
/// Writes are buffered locally until commit, so uncommitted writes are not
/// visible to other transactions and rollback simply discards the buffer.
#[derive(Clone, Debug)]
pub struct MemoryTransaction {
    data: Arc<storage_skipmap::SkipMap<Bytes, Bytes>>,
    /// Local write buffer. `None` means the key was deleted.
    local: BTreeMap<Bytes, Option<Bytes>>,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
}

impl MemoryTransaction {
    /// Create a new transaction.
    pub(crate) fn new(
        data: Arc<storage_skipmap::SkipMap<Bytes, Bytes>>,
        opts: TxnOptions,
    ) -> Self {
        Self {
            data,
            local: BTreeMap::new(),
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
        }
    }

    fn ensure_active(&self) -> Result<()> {
        if !self.active {
            return Err(Error::InactiveTransaction);
        }
        Ok(())
    }
}

impl Transaction for MemoryTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.ensure_active()?;
        MemoryEngine::check_key(key)?;

        // Read-your-writes: consult local buffer first.
        if let Some(value) = self.local.get(key) {
            return Ok(value.clone());
        }

        let key_bytes = Bytes::copy_from_slice(key);
        Ok(self.data.get(&key_bytes))
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        MemoryEngine::check_key(key)?;
        MemoryEngine::check_value(value)?;
        self.local.insert(
            Bytes::copy_from_slice(key),
            Some(Bytes::copy_from_slice(value)),
        );
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        MemoryEngine::check_key(key)?;
        self.local.insert(Bytes::copy_from_slice(key), None);
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl Cursor<Error = Self::Error>> {
        self.ensure_active()?;

        // Merge the shared map and the local write buffer into a snapshot.
        let start_bytes = start.map(Bytes::copy_from_slice);
        let end_bytes = end.map(Bytes::copy_from_slice);
        let mut merged: BTreeMap<Bytes, Option<Bytes>> = self
            .data
            .range(start_bytes.as_ref(), end_bytes.as_ref())
            .into_iter()
            .map(|(k, v)| (k, Some(v)))
            .collect();

        for (k, v) in &self.local {
            if let Some(v) = v {
                merged.insert(k.clone(), Some(v.clone()));
            } else {
                merged.remove(k);
            }
        }

        // Filter by range again after applying local writes.
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

        Ok(MemoryCursor::from_snapshot(buffer))
    }

    fn commit(mut self) -> Result<()> {
        self.ensure_active()?;
        for (key, value) in self.local {
            match value {
                Some(v) => {
                    self.data.insert(key, v);
                }
                None => {
                    self.data.remove(&key);
                }
            }
        }
        self.active = false;
        Ok(())
    }

    fn rollback(mut self) -> Result<()> {
        self.ensure_active()?;
        self.active = false;
        Ok(())
    }

    fn set_isolation(&mut self, level: IsolationLevel) -> Result<()> {
        self.ensure_active()?;
        self.isolation = level;
        Ok(())
    }
}
