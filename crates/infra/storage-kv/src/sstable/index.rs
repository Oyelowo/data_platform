//! SSTable index block builder and iterator.

use bytes::Bytes;

use super::block::{BlockBuilder, BlockComparator, BlockIterator};
use super::format::BlockHandle;

/// Separator key used in index entries.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub handle: BlockHandle,
}

/// Builder for the index block.
pub struct IndexBuilder {
    block_builder: BlockBuilder,
}

impl IndexBuilder {
    pub fn new(restart_interval: usize) -> Self {
        Self {
            block_builder: BlockBuilder::new(restart_interval, BlockComparator::Bytewise),
        }
    }

    /// Add an entry for a data block.
    ///
    /// `start` is the last key of the previous data block (or the first key of
    /// the first data block). `limit` is the first key of the *next* data block
    /// when one exists; for the last data block `limit` should equal `start` so
    /// that the separator is exactly the block's last key.
    ///
    /// The stored separator is the shortest key `s` such that
    /// `start <= s < limit`. This guarantees that a lookup for any key inside
    /// the data block will seek to this index entry, while a lookup for a key
    /// in a later block will skip past it.
    pub fn add_index_entry(&mut self, start: &[u8], limit: &[u8], handle: BlockHandle) {
        let separator = shortest_separator(start, limit);

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

/// Produce the shortest key `s` such that `start <= s < limit`.
///
/// This follows the LevelDB bytewise-comparator `FindShortestSeparator`
/// algorithm:
///
/// 1. Find the length of the common prefix of `start` and `limit`.
/// 2. If one string is a prefix of the other, no key can be shortened while
///    staying strictly below `limit`; return `start` unchanged.
/// 3. Otherwise, if `start[diff] + 1 < limit[diff]`, increment that byte in
///    `start` by one and truncate the result to `diff + 1` bytes.
/// 4. If incrementing would not stay below `limit`, return `start` unchanged.
pub fn shortest_separator(start: &[u8], limit: &[u8]) -> Vec<u8> {
    let min_len = start.len().min(limit.len());
    let mut diff = 0;
    while diff < min_len && start[diff] == limit[diff] {
        diff += 1;
    }

    if diff == min_len {
        // One key is a prefix of the other. `start` is already < `limit` and
        // is the shortest separator.
        return start.to_vec();
    }

    let start_byte = start[diff];
    let limit_byte = limit[diff];
    if start_byte < 0xff && start_byte + 1 < limit_byte {
        let mut sep = Vec::with_capacity(diff + 1);
        sep.extend_from_slice(&start[..diff]);
        sep.push(start_byte + 1);
        return sep;
    }

    // Cannot shorten without risking `>= limit`; use the full last key.
    start.to_vec()
}

/// Iterator over index entries.
pub struct IndexIterator {
    inner: BlockIterator,
}

impl IndexIterator {
    pub fn new(data: Bytes) -> Self {
        Self {
            inner: BlockIterator::new(data, BlockComparator::Bytewise),
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
