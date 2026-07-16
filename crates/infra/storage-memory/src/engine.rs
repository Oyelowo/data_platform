//! In-memory storage engine implementation.

use bytes::Bytes;
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
/// The engine is fully thread-safe and uses `storage-skipmap` for lock-free
/// reads and writes. It is suitable for tests, caches, and as a reference
/// implementation for the storage trait API.
#[derive(Clone, Debug)]
pub struct MemoryEngine {
    data: Arc<storage_skipmap::SkipMap<Bytes, Bytes>>,
}

impl MemoryEngine {
    /// Create a new, empty in-memory engine.
    pub fn new() -> Self {
        Self {
            data: Arc::new(storage_skipmap::SkipMap::new()),
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
        let key_bytes = Bytes::copy_from_slice(key);
        Ok(self.data.get(&key_bytes))
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor> {
        let start_bytes = start.map(Bytes::copy_from_slice);
        let end_bytes = end.map(Bytes::copy_from_slice);
        let buffer = self
            .data
            .range(start_bytes.as_ref(), end_bytes.as_ref())
            .into_iter()
            .collect();
        Ok(MemoryCursor::from_snapshot(buffer))
    }

    fn stats(&self) -> Result<EngineStats> {
        let entries = self.data.iter();
        let mut memory_bytes = 0u64;
        for (k, v) in &entries {
            memory_bytes += k.len() as u64 + v.len() as u64;
        }
        Ok(EngineStats {
            name: self.name(),
            disk_bytes: 0,
            memory_bytes,
            num_keys: Some(entries.len() as u64),
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
