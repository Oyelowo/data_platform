//! In-memory storage engine implementation.

use bytes::Bytes;
use crossbeam_skiplist::SkipMap;
use std::sync::Arc;

use storage_traits::{Engine, EngineStats, Error, Result, TxnOptions};

use crate::cursor::MemoryCursor;
use crate::transaction::MemoryTransaction;

/// Maximum allowed key size in bytes.
pub const MAX_KEY_SIZE: usize = 8 * 1024 * 1024; // 8 MiB
/// Maximum allowed inline value size in bytes.
pub const MAX_VALUE_SIZE: usize = 512 * 1024 * 1024; // 512 MiB

/// A high-performance in-memory storage engine backed by a lock-free skip-map.
///
/// The engine is fully thread-safe and uses `crossbeam-skiplist` for
/// lock-free reads and writes. It is suitable for tests, caches, and as a
/// reference implementation for the storage trait API.
#[derive(Clone, Debug)]
pub struct MemoryEngine {
    data: Arc<SkipMap<Bytes, Bytes>>,
}

impl MemoryEngine {
    /// Create a new, empty in-memory engine.
    pub fn new() -> Self {
        Self {
            data: Arc::new(SkipMap::new()),
        }
    }

    /// Validate that a key is within size limits.
    pub(crate) fn check_key(key: &[u8]) -> Result<()> {
        if key.len() > MAX_KEY_SIZE {
            return Err(Error::OutOfBounds {
                kind: storage_traits::BoundKind::Key,
                limit: MAX_KEY_SIZE,
                got: key.len(),
            });
        }
        Ok(())
    }

    /// Validate that a value is within size limits.
    pub(crate) fn check_value(value: &[u8]) -> Result<()> {
        if value.len() > MAX_VALUE_SIZE {
            return Err(Error::OutOfBounds {
                kind: storage_traits::BoundKind::Value,
                limit: MAX_VALUE_SIZE,
                got: value.len(),
            });
        }
        Ok(())
    }
}

impl Default for MemoryEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for MemoryEngine {
    type Error = Error;
    type Transaction = MemoryTransaction;
    type Cursor = MemoryCursor;

    fn name(&self) -> &'static str {
        "memory"
    }

    fn begin(&self, opts: TxnOptions) -> Result<Self::Transaction> {
        Ok(MemoryTransaction::new(Arc::clone(&self.data), opts))
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        Self::check_key(key)?;
        Ok(self.data.get(key).map(|e| e.value().clone()))
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor> {
        Ok(MemoryCursor::new(Arc::clone(&self.data), start, end))
    }

    fn stats(&self) -> Result<EngineStats> {
        let mut memory_bytes = 0u64;
        let mut num_keys = 0u64;
        for entry in self.data.iter() {
            memory_bytes += entry.key().len() as u64 + entry.value().len() as u64;
            num_keys += 1;
        }
        Ok(EngineStats {
            name: self.name(),
            disk_bytes: 0,
            memory_bytes,
            num_keys: Some(num_keys),
        })
    }

    fn sync(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_is_empty() {
        let engine = MemoryEngine::new();
        assert_eq!(engine.get(b"a").unwrap(), None);
    }
}
