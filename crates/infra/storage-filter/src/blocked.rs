//! Cache-friendly blocked Bloom filter.
//!
//! A blocked Bloom filter stores one small Bloom filter per cache line,
//! improving CPU cache behavior during negative lookups.

use crate::bloom::{bloom_hash, bloom_probes};
use bitvec::prelude::*;

const BLOCK_BITS: usize = 512; // 64 bytes

/// A blocked Bloom filter.
#[derive(Debug, Clone)]
pub struct BlockedBloomFilter {
    blocks: Vec<BitVec<u8, Msb0>>,
    bits_per_key: usize,
    num_probes: usize,
}

impl BlockedBloomFilter {
    /// Create an empty blocked Bloom filter with space for `capacity` keys.
    pub fn with_capacity(capacity: usize, bits_per_key: usize) -> Self {
        let num_blocks = (capacity * bits_per_key).div_ceil(BLOCK_BITS).max(1);
        let mut blocks = Vec::with_capacity(num_blocks);
        for _ in 0..num_blocks {
            blocks.push(bitvec![u8, Msb0; 0; BLOCK_BITS]);
        }
        Self {
            blocks,
            bits_per_key,
            num_probes: bloom_probes(bits_per_key),
        }
    }

    /// Add a key to the filter.
    pub fn add(&mut self, key: &[u8]) {
        let block_idx = self.block_index(key);
        let bits = &mut self.blocks[block_idx];
        let mut h = bloom_hash(key);
        let delta = h.rotate_left(15);
        for _ in 0..self.num_probes {
            let bit = (h as usize) % BLOCK_BITS;
            bits.set(bit, true);
            h = h.wrapping_add(delta);
        }
    }

    /// Return `true` if `key` may be present.
    pub fn may_contain(&self, key: &[u8]) -> bool {
        let block_idx = self.block_index(key);
        let bits = &self.blocks[block_idx];
        let mut h = bloom_hash(key);
        let delta = h.rotate_left(15);
        for _ in 0..self.num_probes {
            let bit = (h as usize) % BLOCK_BITS;
            if !bits[bit] {
                return false;
            }
            h = h.wrapping_add(delta);
        }
        true
    }

    fn block_index(&self, key: &[u8]) -> usize {
        (bloom_hash(key) as usize) % self.blocks.len()
    }

    /// Return the configured bits-per-key value.
    pub fn bits_per_key(&self) -> usize {
        self.bits_per_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_bloom_basic() {
        let mut filter = BlockedBloomFilter::with_capacity(1000, 10);
        for i in 0..100u32 {
            filter.add(&i.to_le_bytes());
        }
        for i in 0..100u32 {
            assert!(filter.may_contain(&i.to_le_bytes()));
        }
    }
}
