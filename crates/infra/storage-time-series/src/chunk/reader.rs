//! Read and iterate over a serialized chunk.

use crate::chunk::encoding::decode_samples;
use crate::format::{ChunkHeader, Sample, Timestamp};
use crate::options::CompressionKind;

/// Reader for a serialized chunk.
#[derive(Debug, Clone)]
pub struct ChunkReader<'a> {
    header: ChunkHeader,
    payload: &'a [u8],
}

impl<'a> ChunkReader<'a> {
    /// Create a chunk reader from serialized bytes.
    pub fn new(bytes: &'a [u8]) -> crate::Result<Self> {
        let header = ChunkHeader::decode(bytes)?;
        // We need to know the payload offset. The header length is:
        // 4 + 4 + 4 + series_key.len() + 4 + 8 + 8 + 1 + 4
        let header_len = 4 + 4 + 4 + header.series_key.len() + 4 + 8 + 8 + 1 + 4;
        if bytes.len() < header_len + 4 {
            return Err(crate::Error::corruption("chunk missing payload crc"));
        }
        let payload = &bytes[header_len..bytes.len() - 4];
        let stored_payload_crc = storage_format::read_u32_le(&bytes[bytes.len() - 4..]);
        let computed_payload_crc = storage_format::crc32c(payload);
        if stored_payload_crc != computed_payload_crc {
            return Err(crate::Error::corruption("chunk payload checksum mismatch"));
        }
        Ok(Self { header, payload })
    }

    /// Return the chunk header.
    pub fn header(&self) -> &ChunkHeader {
        &self.header
    }

    /// Return all samples in the chunk.
    pub fn samples(&self) -> crate::Result<Vec<Sample>> {
        decode_samples(self.payload, self.header.compression)
    }

    /// Return an iterator over all samples.
    pub fn iter(&self) -> crate::Result<impl Iterator<Item = Sample> + '_> {
        let samples = self.samples()?;
        Ok(samples.into_iter())
    }

    /// Return samples in the half-open timestamp range `[start, end)`.
    pub fn range(&self, start: Timestamp, end: Timestamp) -> crate::Result<Vec<Sample>> {
        let all = self.samples()?;
        Ok(all
            .into_iter()
            .filter(|s| s.timestamp >= start && s.timestamp < end)
            .collect())
    }

    /// Aggregate values in this chunk.
    pub fn aggregate(&self, agg: crate::query::aggregate::Aggregation) -> crate::Result<crate::query::aggregate::AggregateResult> {
        use crate::query::aggregate::{AggregateResult, Aggregation};
        let samples = self.samples()?;
        let values: Vec<f64> = samples
            .into_iter()
            .filter_map(|s| match s.value {
                crate::format::Value::F64(v) => Some(v),
                _ => None,
            })
            .collect();
        if values.is_empty() {
            return Ok(AggregateResult::Empty);
        }
        match agg {
            Aggregation::Count => Ok(AggregateResult::Scalar(values.len() as f64)),
            Aggregation::Sum => Ok(AggregateResult::Scalar(values.iter().sum())),
            Aggregation::Avg => Ok(AggregateResult::Scalar(
                values.iter().sum::<f64>() / values.len() as f64,
            )),
            Aggregation::Min => Ok(AggregateResult::Scalar(
                values.iter().copied().fold(f64::INFINITY, f64::min),
            )),
            Aggregation::Max => Ok(AggregateResult::Scalar(
                values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            )),
            Aggregation::Rate => {
                if values.len() < 2 {
                    Ok(AggregateResult::Empty)
                } else {
                    let first = values.first().copied().unwrap_or(0.0);
                    let last = values.last().copied().unwrap_or(0.0);
                    Ok(AggregateResult::Scalar(last - first))
                }
            }
        }
    }

    /// Return the compression kind recorded in the header.
    pub fn compression(&self) -> CompressionKind {
        self.header.compression
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::builder::ChunkBuilder;
    use crate::format::Value;
    use crate::query::aggregate::Aggregation;

    fn make_samples(n: usize) -> Vec<Sample> {
        (0..n as u64)
            .map(|i| Sample {
                timestamp: i * 10,
                value: Value::F64(i as f64),
            })
            .collect()
    }

    #[test]
    fn chunk_roundtrip() {
        let mut builder = ChunkBuilder::new(b"cpu".to_vec(), CompressionKind::Gorilla);
        for s in make_samples(50) {
            builder.push(s).unwrap();
        }
        let bytes = builder.finish().unwrap();
        let reader = ChunkReader::new(&bytes).unwrap();
        let samples = reader.samples().unwrap();
        assert_eq!(samples.len(), 50);
    }

    #[test]
    fn chunk_range_query() {
        let mut builder = ChunkBuilder::new(b"cpu".to_vec(), CompressionKind::Gorilla);
        for s in make_samples(100) {
            builder.push(s).unwrap();
        }
        let bytes = builder.finish().unwrap();
        let reader = ChunkReader::new(&bytes).unwrap();
        let range = reader.range(200, 500).unwrap();
        assert_eq!(range.len(), 30); // timestamps 200..500 step 10
    }

    #[test]
    fn chunk_aggregate_sum() {
        let mut builder = ChunkBuilder::new(b"cpu".to_vec(), CompressionKind::Gorilla);
        for s in make_samples(10) {
            builder.push(s).unwrap();
        }
        let bytes = builder.finish().unwrap();
        let reader = ChunkReader::new(&bytes).unwrap();
        let result = reader.aggregate(Aggregation::Sum).unwrap();
        assert_eq!(result, crate::query::aggregate::AggregateResult::Scalar(45.0));
    }
}
