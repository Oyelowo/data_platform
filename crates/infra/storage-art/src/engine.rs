//! `storage_traits::Engine` implementation for `ArtMap`.

use std::collections::BTreeMap;

use bytes::Bytes;
use storage_traits::{
    BoundKind, Cursor, Engine, EngineStats, Error, IsolationLevel, Result, Transaction, TxnOptions,
};

use crate::cursor::ArtCursor;
use crate::map::ArtMap;

/// A transaction over an `ArtMap`.
///
/// Writes are buffered locally until commit. This provides read-your-writes
/// semantics and prevents dirty reads from other in-flight transactions.
#[derive(Debug)]
pub struct ArtTransaction {
    map: ArtMap,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
    local: BTreeMap<Bytes, Option<Bytes>>,
}

impl ArtTransaction {
    fn ensure_active(&self) -> Result<()> {
        if !self.active {
            return Err(Error::InactiveTransaction);
        }
        Ok(())
    }
}

impl Transaction for ArtTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.ensure_active()?;
        ArtMap::check_key_static(key)?;
        if let Some(value) = self.local.get(key) {
            return Ok(value.clone());
        }
        Ok(self.map.get(key))
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        ArtMap::check_key_static(key)?;
        ArtMap::check_value_static(value)?;
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
        ArtMap::check_key_static(key)?;
        self.local.insert(Bytes::copy_from_slice(key), None);
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl Cursor<Error = Self::Error>> {
        self.ensure_active()?;

        let mut entries = Vec::new();
        self.map.collect_entries(&mut entries);

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

    fn commit(mut self) -> Result<()> {
        self.ensure_active()?;
        for (key, value) in self.local {
            match value {
                Some(v) => {
                    self.map.insert(&key, &v).map_err(map_error)?;
                }
                None => {
                    self.map.remove(&key).map_err(map_error)?;
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

impl Engine for ArtMap {
    type Error = Error;
    type Transaction = ArtTransaction;
    type Cursor = ArtCursor;

    fn name(&self) -> &'static str {
        "storage-art"
    }

    fn begin(&self, opts: TxnOptions) -> Result<Self::Transaction> {
        Ok(ArtTransaction {
            map: self.clone(),
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
            local: BTreeMap::new(),
        })
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        ArtMap::check_key_static(key)?;
        Ok(self.get(key))
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor> {
        Ok(self.range(start, end))
    }

    fn stats(&self) -> Result<EngineStats> {
        Ok(crate::stats::engine_stats(self))
    }

    fn sync(&self) -> Result<()> {
        Ok(())
    }
}

fn map_error(e: crate::error::Error) -> Error {
    match e {
        crate::error::Error::KeyTooLong { len, max } => Error::OutOfBounds {
            kind: BoundKind::Key,
            limit: max,
            got: len,
        },
        crate::error::Error::ValueTooLong { len, max } => Error::OutOfBounds {
            kind: BoundKind::Value,
            limit: max,
            got: len,
        },
        crate::error::Error::EntryLimitReached(limit) => Error::OutOfBounds {
            kind: BoundKind::Batch,
            limit,
            got: limit,
        },
        crate::error::Error::Corruption(msg) => Error::Corruption(msg),
        crate::error::Error::Io(io) => Error::Io(io),
        crate::error::Error::Wal(wal) => {
            Error::Io(std::io::Error::other(format!("wal error: {wal}")))
        }
        crate::error::Error::InvalidArgument(msg) => Error::Corruption(msg),
    }
}

impl ArtMap {
    /// Key size check that returns `storage_traits::Error`.
    pub(crate) fn check_key_static(key: &[u8]) -> Result<()> {
        if key.len() > MAX_KEY_LEN {
            return Err(Error::OutOfBounds {
                kind: BoundKind::Key,
                limit: MAX_KEY_LEN,
                got: key.len(),
            });
        }
        Ok(())
    }

    /// Value size check that returns `storage_traits::Error`.
    pub(crate) fn check_value_static(value: &[u8]) -> Result<()> {
        if value.len() > DEFAULT_MAX_VALUE_SIZE {
            return Err(Error::OutOfBounds {
                kind: BoundKind::Value,
                limit: DEFAULT_MAX_VALUE_SIZE,
                got: value.len(),
            });
        }
        Ok(())
    }
}

use crate::node::MAX_KEY_LEN;

const DEFAULT_MAX_VALUE_SIZE: usize = 8 * 1024 * 1024;
