//! Bloom filter block for SSTables.
//!
//! The implementation lives in the shared [`storage_filter`] crate; this module
//! re-exports it so the rest of the engine keeps the same names.

pub use storage_filter::{BloomFilterBuilder, BloomFilterReader};

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

    /// Regression: total_bits must be a multiple of 8 so the reader does not
    /// see zero padding bits in the final byte. 82 keys * 10 bits/key = 820
    /// bits, which used to leave 4 zero padding bits and produce false
    /// negatives.
    #[test]
    fn bloom_no_false_negatives_with_partial_final_byte() {
        let mut builder = BloomFilterBuilder::new(10);
        for i in 0..82u32 {
            builder.add_key(format!("t{}-k{}", i % 2, i / 2).as_bytes());
        }
        let data = builder.finish();
        let reader = BloomFilterReader::new(&data, 10);

        for i in 0..82u32 {
            let key = format!("t{}-k{}", i % 2, i / 2);
            assert!(
                reader.may_contain(key.as_bytes()),
                "false negative for key {}",
                key
            );
        }
    }
}
