//! On-disk format definitions for `storage-time-series`.
//!
//! All multi-byte integers are little-endian. Metadata and chunk headers carry
//! CRC32C checksums so that torn writes and corruption are detected on open.

use std::collections::HashSet;

use bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use crate::index::LabelIndex;
use crate::options::{CompressionKind, TimeSeriesOptions, ValueKind};

/// Magic number for time-series engine files.
pub const MAGIC: u32 = 0x54_53_45_31; // "TSE1"

/// Current on-disk format version.
pub const VERSION: u32 = 1;

/// File name for the engine metadata file.
pub const META_FILE: &str = "META";

/// Subdirectory for WAL segments.
pub const WAL_DIR: &str = "WAL";

/// Subdirectory for chunk files.
pub const CHUNKS_DIR: &str = "chunks";

/// Subdirectory for the label index.
pub const INDEX_DIR: &str = "index";

/// File name for persisted label index.
pub const LABEL_INDEX_FILE: &str = "labels.index";

/// A sample value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// 64-bit floating point scalar.
    F64(f64),
    /// Opaque byte payload.
    Bytes(Vec<u8>),
}

impl Value {
    /// Return the value kind.
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::F64(_) => ValueKind::F64,
            Value::Bytes(_) => ValueKind::Bytes,
        }
    }

    /// Encode the value to bytes for the `Engine` trait API.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Value::F64(v) => {
                let mut buf = Vec::with_capacity(9);
                buf.push(ValueKind::F64 as u8);
                buf.extend_from_slice(&v.to_be_bytes());
                buf
            }
            Value::Bytes(b) => {
                let mut buf = Vec::with_capacity(1 + b.len());
                buf.push(ValueKind::Bytes as u8);
                buf.extend_from_slice(b);
                buf
            }
        }
    }

    /// Decode a value from the `Engine` trait byte encoding.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        if buf.is_empty() {
            return Err(crate::Error::corruption("empty value bytes"));
        }
        match buf[0] {
            kind if kind == ValueKind::F64 as u8 => {
                if buf.len() < 9 {
                    return Err(crate::Error::corruption("truncated f64 value"));
                }
                let bytes = buf[1..9].try_into().map_err(|_| {
                    crate::Error::corruption("cannot read f64 bytes")
                })?;
                Ok(Value::F64(f64::from_be_bytes(bytes)))
            }
            kind if kind == ValueKind::Bytes as u8 => Ok(Value::Bytes(buf[1..].to_vec())),
            other => Err(crate::Error::corruption(format!(
                "unknown value kind byte {other}"
            ))),
        }
    }
}

/// A timestamp/sample pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Nanoseconds since epoch.
    pub timestamp: Timestamp,
    /// Sample value.
    pub value: Value,
}

/// Nanosecond timestamp alias.
pub type Timestamp = u64;

/// On-disk metadata header for a time-series engine database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// Format magic.
    pub magic: u32,
    /// Format version.
    pub version: u32,
    /// Engine options.
    pub options: TimeSeriesOptions,
    /// Set of all known series keys.
    pub series: HashSet<Vec<u8>>,
    /// Monotonic chunk id counter.
    pub next_chunk_id: u64,
    /// Last WAL checkpoint LSN.
    pub wal_checkpoint_lsn: u64,
    /// Label index snapshot.
    pub label_index: LabelIndex,
}

impl Metadata {
    /// Create metadata from validated options.
    pub fn new(options: TimeSeriesOptions) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            options,
            series: HashSet::new(),
            next_chunk_id: 1,
            wal_checkpoint_lsn: 0,
            label_index: LabelIndex::new(),
        }
    }

    /// Serialize metadata to bytes with a trailing CRC32C checksum.
    pub fn encode(&self) -> crate::Result<Vec<u8>> {
        let body = bincode::serialize(self).map_err(crate::Error::corruption)?;
        let mut buf = Vec::with_capacity(body.len() + 12);
        buf.put_u32_le(MAGIC);
        buf.put_u32_le(VERSION);
        buf.extend_from_slice(&body);
        let crc = storage_format::crc32c(&buf);
        buf.put_u32_le(crc);
        Ok(buf)
    }

    /// Deserialize metadata from bytes and verify checksums.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        if buf.len() < 12 {
            return Err(crate::Error::corruption("metadata file too short"));
        }
        let magic = storage_format::read_u32_le(buf);
        let version = storage_format::read_u32_le(&buf[4..]);
        if magic != MAGIC {
            return Err(crate::Error::corruption(format!(
                "bad metadata magic: {magic:#x}"
            )));
        }
        if version != VERSION {
            return Err(crate::Error::corruption(format!(
                "unsupported metadata version: {version}"
            )));
        }
        let body_end = buf.len() - 4;
        let stored_crc = storage_format::read_u32_le(&buf[body_end..]);
        let computed_crc = storage_format::crc32c(&buf[..body_end]);
        if stored_crc != computed_crc {
            return Err(crate::Error::corruption("metadata checksum mismatch"));
        }
        let meta: Metadata = bincode::deserialize(&buf[8..body_end])
            .map_err(crate::Error::corruption)?;
        Ok(meta)
    }
}

/// A single WAL record payload for the time-series engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalRecord {
    /// Insert or update a sample.
    Put {
        /// Canonical series key.
        series_key: Vec<u8>,
        /// Sample timestamp.
        timestamp: Timestamp,
        /// Sample value.
        value: Value,
    },
    /// Delete all samples for a series.
    DeleteSeries {
        /// Canonical series key.
        series_key: Vec<u8>,
    },
    /// Delete samples in a half-open time range for a series.
    DeleteRange {
        /// Canonical series key.
        series_key: Vec<u8>,
        /// Start timestamp (inclusive).
        start: Timestamp,
        /// End timestamp (exclusive).
        end: Timestamp,
    },
    /// Checkpoint marker carrying the latest metadata snapshot.
    Checkpoint {
        /// Metadata at the time of the checkpoint.
        metadata: Metadata,
    },
}

impl WalRecord {
    /// Encode a WAL record to bytes.
    pub fn encode(&self) -> crate::Result<Vec<u8>> {
        bincode::serialize(self).map_err(crate::Error::corruption)
    }

    /// Decode a WAL record from bytes.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        bincode::deserialize(buf).map_err(crate::Error::corruption)
    }
}

/// Header for a time-series chunk file.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHeader {
    /// Format magic.
    pub magic: u32,
    /// Format version.
    pub version: u32,
    /// Series key this chunk belongs to.
    pub series_key: Vec<u8>,
    /// Number of samples in the chunk.
    pub count: u32,
    /// Minimum timestamp in the chunk.
    pub min_ts: Timestamp,
    /// Maximum timestamp in the chunk.
    pub max_ts: Timestamp,
    /// Compression kind used for the chunk payload.
    pub compression: CompressionKind,
    /// CRC32C of the header bytes before the CRC field.
    pub crc: u32,
}

impl ChunkHeader {
    /// Encode the chunk header.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(32 + self.series_key.len());
        buf.put_u32_le(self.magic);
        buf.put_u32_le(self.version);
        buf.put_u32_le(self.series_key.len() as u32);
        buf.extend_from_slice(&self.series_key);
        buf.put_u32_le(self.count);
        buf.put_u64_le(self.min_ts);
        buf.put_u64_le(self.max_ts);
        buf.put_u8(self.compression as u8);
        let crc = storage_format::crc32c(&buf);
        buf.put_u32_le(crc);
        buf
    }

    /// Decode a chunk header from bytes.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        if buf.len() < 24 {
            return Err(crate::Error::corruption("chunk header too short"));
        }
        let mut cursor = buf;
        let magic = cursor.get_u32_le();
        let version = cursor.get_u32_le();
        let series_key_len = cursor.get_u32_le() as usize;
        let header_len_before_crc = 12 + series_key_len + 4 + 8 + 8 + 1;
        if buf.len() < header_len_before_crc + 4 {
            return Err(crate::Error::corruption("chunk header truncated"));
        }
        let series_key = cursor.copy_to_bytes(series_key_len).to_vec();
        let count = cursor.get_u32_le();
        let min_ts = cursor.get_u64_le();
        let max_ts = cursor.get_u64_le();
        let compression_byte = cursor.get_u8();
        let compression = match compression_byte {
            b if b == CompressionKind::None as u8 => CompressionKind::None,
            b if b == CompressionKind::Gorilla as u8 => CompressionKind::Gorilla,
            b if b == CompressionKind::Zstd as u8 => CompressionKind::Zstd,
            other => {
                return Err(crate::Error::corruption(format!(
                    "unknown compression kind {other}"
                )))
            }
        };
        let stored_crc = cursor.get_u32_le();
        let computed_crc = storage_format::crc32c(&buf[..header_len_before_crc]);
        if stored_crc != computed_crc {
            return Err(crate::Error::corruption("chunk header checksum mismatch"));
        }
        if magic != MAGIC {
            return Err(crate::Error::corruption(format!(
                "bad chunk magic: {magic:#x}"
            )));
        }
        if version != VERSION {
            return Err(crate::Error::corruption(format!(
                "unsupported chunk version: {version}"
            )));
        }
        Ok(Self {
            magic,
            version,
            series_key,
            count,
            min_ts,
            max_ts,
            compression,
            crc: stored_crc,
        })
    }
}

/// Encode a series key and timestamp into the `Engine` trait composite key.
pub fn encode_composite_key(series_key: &[u8], timestamp: Timestamp) -> Vec<u8> {
    let mut key = Vec::with_capacity(series_key.len() + 8 + 1);
    key.extend_from_slice(series_key);
    key.extend_from_slice(&timestamp.to_be_bytes());
    key.push(0u8); // value kind byte placeholder for Engine compatibility
    key
}

/// Decode a composite key into `(series_key, timestamp)`.
pub fn decode_composite_key(key: &[u8]) -> crate::Result<(&[u8], Timestamp)> {
    if key.len() < 9 {
        return Err(crate::Error::corruption("composite key too short"));
    }
    let (series_key, tail) = key.split_at(key.len() - 9);
    let ts = Timestamp::from_be_bytes(
        tail[..8]
            .try_into()
            .map_err(|_| crate::Error::corruption("cannot read timestamp bytes"))?,
    );
    Ok((series_key, ts))
}

/// Build a canonical series key from a metric and sorted tags.
pub fn build_series_key(metric: &[u8], tags: &[(String, String)]) -> Vec<u8> {
    let mut key = Vec::with_capacity(metric.len() + tags.iter().map(|(k, v)| k.len() + v.len() + 2).sum::<usize>());
    key.extend_from_slice(metric);
    for (k, v) in tags {
        key.push(0u8);
        key.extend_from_slice(k.as_bytes());
        key.push(b'=');
        key.extend_from_slice(v.as_bytes());
    }
    key
}

/// Parsed series key components.
pub type ParsedSeriesKey = (Vec<u8>, Vec<(String, String)>);

/// Parse a canonical series key back into `(metric, tags)`.
pub fn parse_series_key(key: &[u8]) -> crate::Result<ParsedSeriesKey> {
    let mut parts = key.split(|&b| b == 0);
    let metric = parts
        .next()
        .ok_or_else(|| crate::Error::corruption("empty series key"))?
        .to_vec();
    let mut tags = Vec::new();
    for part in parts {
        let s = std::str::from_utf8(part).map_err(|_| {
            crate::Error::corruption("series key tag is not valid utf-8")
        })?;
        let mut kv = s.splitn(2, '=');
        let tag_key = kv
            .next()
            .ok_or_else(|| crate::Error::corruption("missing tag key"))?
            .to_string();
        let tag_value = kv
            .next()
            .ok_or_else(|| crate::Error::corruption("missing tag value"))?
            .to_string();
        tags.push((tag_key, tag_value));
    }
    Ok((metric, tags))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let meta = Metadata::new(TimeSeriesOptions::default());
        let encoded = meta.encode().unwrap();
        let decoded = Metadata::decode(&encoded).unwrap();
        assert_eq!(meta, decoded);
    }

    #[test]
    fn wal_record_roundtrip() {
        let rec = WalRecord::Put {
            series_key: b"cpu\0host=db1".to_vec(),
            timestamp: 12345,
            value: Value::F64(42.0),
        };
        let encoded = rec.encode().unwrap();
        let decoded = WalRecord::decode(&encoded).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn chunk_header_roundtrip() {
        let header = ChunkHeader {
            magic: MAGIC,
            version: VERSION,
            series_key: b"cpu\0host=db1".to_vec(),
            count: 100,
            min_ts: 0,
            max_ts: 9999,
            compression: CompressionKind::Gorilla,
            crc: 0,
        };
        let encoded = header.encode();
        let decoded = ChunkHeader::decode(&encoded).unwrap();
        assert_eq!(header.magic, decoded.magic);
        assert_eq!(header.version, decoded.version);
        assert_eq!(header.series_key, decoded.series_key);
        assert_eq!(header.count, decoded.count);
        assert_eq!(header.min_ts, decoded.min_ts);
        assert_eq!(header.max_ts, decoded.max_ts);
        assert_eq!(header.compression, decoded.compression);
    }

    #[test]
    fn composite_key_roundtrip() {
        let key = encode_composite_key(b"cpu\0host=db1", 123456789);
        let (series_key, ts) = decode_composite_key(&key).unwrap();
        assert_eq!(series_key, b"cpu\0host=db1");
        assert_eq!(ts, 123456789);
    }

    #[test]
    fn series_key_build_and_parse() {
        let tags = [("host".to_string(), "db1".to_string()), ("region".to_string(), "us-east".to_string())];
        let key = build_series_key(b"cpu", &tags);
        let (metric, parsed) = parse_series_key(&key).unwrap();
        assert_eq!(metric, b"cpu");
        assert_eq!(parsed, tags);
    }

    #[test]
    fn value_roundtrip() {
        for value in [Value::F64(std::f64::consts::PI), Value::Bytes(vec![1, 2, 3])] {
            let encoded = value.encode();
            let decoded = Value::decode(&encoded).unwrap();
            assert_eq!(value, decoded);
        }
    }
}
