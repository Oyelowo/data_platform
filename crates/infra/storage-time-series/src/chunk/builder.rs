//! Build a serialized chunk from an ordered sequence of samples.

use bytes::BufMut;

use crate::chunk::encoding::encode_samples;
use crate::format::{ChunkHeader, Sample, Timestamp, MAGIC, VERSION};
use crate::options::CompressionKind;

/// In-memory chunk builder.
#[derive(Debug)]
pub struct ChunkBuilder {
    series_key: Vec<u8>,
    compression: CompressionKind,
    samples: Vec<Sample>,
    min_ts: Timestamp,
    max_ts: Timestamp,
}

impl ChunkBuilder {
    /// Create a new builder for `series_key` using `compression`.
    pub fn new(series_key: Vec<u8>, compression: CompressionKind) -> Self {
        Self {
            series_key,
            compression,
            samples: Vec::new(),
            min_ts: 0,
            max_ts: 0,
        }
    }

    /// Append a sample. Samples must be in strictly ascending timestamp order.
    pub fn push(&mut self, sample: Sample) -> crate::Result<()> {
        if let Some(last) = self.samples.last()
            && sample.timestamp <= last.timestamp
        {
            return Err(crate::Error::invalid_argument(
                "samples must be in ascending timestamp order",
            ));
        }
        if self.samples.is_empty() {
            self.min_ts = sample.timestamp;
            self.max_ts = sample.timestamp;
        } else {
            self.max_ts = sample.timestamp;
        }
        self.samples.push(sample);
        Ok(())
    }

    /// Return the number of buffered samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Return whether the builder is empty.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Return the approximate uncompressed byte size of the buffered samples.
    pub fn uncompressed_size(&self) -> usize {
        self.samples
            .iter()
            .map(|s| 8 + s.value.encode().len())
            .sum()
    }

    /// Return the series key for this chunk.
    pub fn series_key(&self) -> &[u8] {
        &self.series_key
    }

    /// Return the minimum timestamp buffered so far.
    pub fn min_ts(&self) -> Timestamp {
        self.min_ts
    }

    /// Return the maximum timestamp buffered so far.
    pub fn max_ts(&self) -> Timestamp {
        self.max_ts
    }

    /// Serialize the chunk into bytes (header + payload + payload CRC).
    pub fn finish(self) -> crate::Result<Vec<u8>> {
        if self.samples.is_empty() {
            return Err(crate::Error::invalid_argument("cannot finish empty chunk"));
        }
        let payload = encode_samples(&self.samples, self.compression)?;
        let payload_crc = storage_format::crc32c(&payload);
        let header = ChunkHeader {
            magic: MAGIC,
            version: VERSION,
            series_key: self.series_key,
            count: self.samples.len() as u32,
            min_ts: self.min_ts,
            max_ts: self.max_ts,
            compression: self.compression,
            crc: 0,
        };
        let mut buf = header.encode();
        buf.extend_from_slice(&payload);
        buf.put_u32_le(payload_crc);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    #[test]
    fn build_and_finish_chunk() {
        let mut builder = ChunkBuilder::new(b"cpu\0host=db1".to_vec(), CompressionKind::Gorilla);
        for i in 0..10u64 {
            builder
                .push(Sample {
                    timestamp: i,
                    value: Value::F64(i as f64),
                })
                .unwrap();
        }
        let chunk = builder.finish().unwrap();
        assert!(!chunk.is_empty());
    }

    #[test]
    fn out_of_order_rejected() {
        let mut builder = ChunkBuilder::new(b"cpu".to_vec(), CompressionKind::None);
        builder
            .push(Sample {
                timestamp: 2,
                value: Value::F64(1.0),
            })
            .unwrap();
        let err = builder
            .push(Sample {
                timestamp: 1,
                value: Value::F64(1.0),
            })
            .unwrap_err();
        assert!(matches!(err, crate::Error::InvalidArgument(_)));
    }
}
