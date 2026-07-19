//! Adaptive Radix Trie map implementation.

use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;

use crate::error::{Error, Result};
use crate::node::{Node, MAX_KEY_LEN};

/// Options controlling `ArtMap` behavior and limits.
#[derive(Clone, Debug)]
pub struct ArtMapOptions {
    /// Maximum key length in bytes.
    pub max_key_len: usize,
    /// Maximum value length in bytes.
    pub max_value_len: usize,
    /// Optional hard limit on the number of entries.
    pub max_entries: Option<usize>,
}

impl Default for ArtMapOptions {
    fn default() -> Self {
        Self {
            max_key_len: MAX_KEY_LEN,
            max_value_len: 8 * 1024 * 1024, // 8 MiB
            max_entries: None,
        }
    }
}

/// An in-memory Adaptive Radix Trie mapping byte keys to byte values.
#[derive(Debug)]
pub struct ArtMap {
    root: RwLock<Option<Arc<Node>>>,
    options: ArtMapOptions,
    len: AtomicUsize,
}

impl ArtMap {
    /// Create a new empty `ArtMap` with the given options.
    pub fn new(options: ArtMapOptions) -> Self {
        Self {
            root: RwLock::new(None),
            options,
            len: AtomicUsize::new(0),
        }
    }

    fn check_key(&self, key: &[u8]) -> Result<()> {
        if key.len() > self.options.max_key_len {
            return Err(Error::KeyTooLong {
                len: key.len(),
                max: self.options.max_key_len,
            });
        }
        Ok(())
    }

    fn check_value(&self, value: &[u8]) -> Result<()> {
        if value.len() > self.options.max_value_len {
            return Err(Error::ValueTooLong {
                len: value.len(),
                max: self.options.max_value_len,
            });
        }
        Ok(())
    }

    /// Look up a key and return a clone of its value, if present.
    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        // TODO: implement ART traversal.
        let _ = key;
        None
    }

    /// Insert a key/value pair.
    ///
    /// Returns the previous value if the key already existed.
    pub fn insert(&self, key: &[u8], value: &[u8]) -> Result<Option<Bytes>> {
        self.check_key(key)?;
        self.check_value(value)?;
        if let Some(limit) = self.options.max_entries {
            if self.len.load(Ordering::Relaxed) >= limit {
                return Err(Error::EntryLimitReached(limit));
            }
        }
        // TODO: implement insert.
        let _ = value;
        Ok(None)
    }

    /// Remove a key and return its value if it existed.
    pub fn remove(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.check_key(key)?;
        // TODO: implement remove.
        Ok(None)
    }

    /// Return the number of entries in the map.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    /// Return true if the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_map_is_empty() {
        let map = ArtMap::new(ArtMapOptions::default());
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }
}
