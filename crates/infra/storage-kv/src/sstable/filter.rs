//! Bloom filter block for SSTables.

use bitvec::prelude::*;

/// Build a Bloom filter from keys.
pub struct BloomFilterBuilder {
    keys: Vec<Vec<u8>>,
    bits_per_key: usize,
}

impl BloomFilterBuilder {
    pub fn new(bits_per_key: usize) -> Self {
        Self {
            keys: Vec::new(),
            bits_per_key,
        }
    }

    pub fn add_key(&mut self, key: &[u8]) {
        self.keys.push(key.to_vec());
    }

    pub fn finish(&self) -> Vec<u8> {
        let num_keys = self.keys.len();
        let total_bits = (num_keys * self.bits_per_key).max(64);
        let num_probes = ((self.bits_per_key as f64) * 0.693) as usize + 1;
        let mut bits = bitvec![u8, Msb0; 0; total_bits];

        for key in &self.keys {
            let mut h = bloom_hash(key);
            let delta = h.rotate_left(15);
            for _ in 0..num_probes {
                let bit = (h as usize) % total_bits;
                bits.set(bit, true);
                h = h.wrapping_add(delta);
            }
        }

        bits.as_raw_slice().to_vec()
    }
}

/// Reader for a Bloom filter block.
pub struct BloomFilterReader {
    bits: BitVec<u8, Msb0>,
    bits_per_key: usize,
    num_probes: usize,
}

impl BloomFilterReader {
    pub fn new(data: &[u8], bits_per_key: usize) -> Self {
        let bits = BitVec::<u8, Msb0>::from_slice(data);
        let num_probes = ((bits_per_key as f64) * 0.693) as usize + 1;
        Self {
            bits,
            bits_per_key,
            num_probes,
        }
    }

    /// Return true if the key may be present (false means definitely absent).
    pub fn may_contain(&self, key: &[u8]) -> bool {
        if self.bits.is_empty() {
            return true;
        }
        let total_bits = self.bits.len();
        let mut h = bloom_hash(key);
        let delta = h.rotate_left(15);
        for _ in 0..self.num_probes {
            let bit = (h as usize) % total_bits;
            if !self.bits[bit] {
                return false;
            }
            h = h.wrapping_add(delta);
        }
        true
    }

    pub fn bits_per_key(&self) -> usize {
        self.bits_per_key
    }
}

fn bloom_hash(key: &[u8]) -> u32 {
    // Use a simple but decent hash. Production systems often use MurmurHash3.
    let mut h: u32 = 0x811c_9dc5;
    for &b in key {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_basic() {
        let mut builder = BloomFilterBuilder::new(10);
        for i in 0..100u32 {
            builder.add_key(&i.to_le_bytes());
        }
        let data = builder.finish();
        let reader = BloomFilterReader::new(&data, 10);

        for i in 0..100u32 {
            assert!(reader.may_contain(&i.to_le_bytes()));
        }

        let mut false_positives = 0;
        for i in 100..200u32 {
            if reader.may_contain(&i.to_le_bytes()) {
                false_positives += 1;
            }
        }
        assert!(false_positives < 5, "false positives = {}", false_positives);
    }
}
