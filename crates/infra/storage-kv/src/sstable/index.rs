//! SSTable index block builder and iterator.

use super::format::BlockHandle;
use super::block::{BlockBuilder, BlockIterator};

/// Separator key used in index entries.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub handle: BlockHandle,
}

/// Builder for the index block.
pub struct IndexBuilder {
    block_builder: BlockBuilder,
    last_key: Vec<u8>,
}

impl IndexBuilder {
    pub fn new(restart_interval: usize) -> Self {
        Self {
            block_builder: BlockBuilder::new(restart_interval),
            last_key: Vec::new(),
        }
    }

    /// Add an entry for a data block. `key` should be the last key of that
    /// block; the stored separator is the shortest string >= `key`.
    pub fn add_index_entry(&mut self, key: &[u8], handle: BlockHandle) {
        let separator = if self.last_key.is_empty() {
            key.to_vec()
        } else {
            shortest_separator(&self.last_key, key)
        };
        self.last_key = key.to_vec();

        let mut value = Vec::new();
        handle.encode(&mut value);
        self.block_builder.add(&separator, &value);
    }

    pub fn finish(&mut self) -> &[u8] {
        self.block_builder.finish()
    }

    pub fn current_size_estimate(&self) -> usize {
        self.block_builder.current_size_estimate()
    }
}

/// Produce the shortest key that is >= start and < limit. A simple correct
/// implementation returns `limit` unchanged.
pub fn shortest_separator(start: &[u8], limit: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i < start.len() && i < limit.len() && start[i] == limit[i] {
        i += 1;
    }
    if i < limit.len() {
        let mut sep = Vec::with_capacity(i + 1);
        sep.extend_from_slice(&limit[..i + 1]);
        return sep;
    }
    limit.to_vec()
}

/// Iterator over index entries.
pub struct IndexIterator<'a> {
    inner: BlockIterator<'a>,
}

impl<'a> IndexIterator<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            inner: BlockIterator::new(data),
        }
    }

    pub fn seek(&mut self, target: &[u8]) {
        self.inner.seek(target);
    }

    pub fn seek_to_first(&mut self) {
        self.inner.seek_to_first();
    }

    pub fn key(&self) -> &[u8] {
        self.inner.key()
    }

    pub fn value(&self) -> BlockHandle {
        BlockHandle::decode(self.inner.value()).0
    }

    pub fn valid(&self) -> bool {
        self.inner.valid()
    }

    pub fn next(&mut self) {
        self.inner.next();
    }
}
