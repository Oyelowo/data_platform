//! On-disk format definitions for `storage-vector`.
//!
//! All multi-byte integers are little-endian. Every file header and page footer
//! carries a CRC32C checksum so that torn writes and corruption are detected
//! on open.

use std::collections::HashMap;

use bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use crate::options::VectorOptions;

/// Magic number for vector engine files.
pub const MAGIC: u32 = 0x56_45_43_54; // "VECT"

/// Current on-disk format version.
pub const VERSION: u32 = 1;

/// File name for the engine metadata file.
pub const META_FILE: &str = "META";

/// Subdirectory for WAL segments.
pub const WAL_DIR: &str = "WAL";

/// Subdirectory for vector page files.
pub const VECTOR_DIR: &str = "vectors";

/// Subdirectory for persisted index files.
pub const INDEX_DIR: &str = "index";

/// File name for persisted HNSW graph.
pub const HNSW_FILE: &str = "hnsw.graph";

/// File name for persisted IVF index.
pub const IVF_FILE: &str = "ivf.index";

/// On-disk metadata header for a vector engine database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// Format magic.
    pub magic: u32,
    /// Format version.
    pub version: u32,
    /// Engine options.
    pub options: VectorOptions,
    /// Maps a user key to its internal vector id.
    pub key_to_id: HashMap<Vec<u8>, u64>,
    /// Monotonic vector id counter.
    pub next_id: u64,
}

impl Metadata {
    /// Create metadata from validated options.
    pub fn new(options: VectorOptions) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            options,
            key_to_id: HashMap::new(),
            next_id: 1,
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
        let meta: Metadata =
            bincode::deserialize(&buf[8..body_end]).map_err(crate::Error::corruption)?;
        Ok(meta)
    }
}

/// A single WAL record payload for the vector engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalRecord {
    /// Insert or update a vector.
    Put {
        /// User key.
        key: Vec<u8>,
        /// Vector value.
        vector: Vec<f32>,
    },
    /// Delete a vector.
    Delete {
        /// User key.
        key: Vec<u8>,
    },
    /// Truncate the WAL after a successful checkpoint.
    Checkpoint {
        /// Next vector id after the checkpoint.
        next_id: u64,
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

/// Header for a vector page file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageHeader {
    /// Page format magic.
    pub magic: u32,
    /// Page format version.
    pub version: u32,
    /// Number of vectors in this page.
    pub count: u32,
    /// Vector dimension.
    pub dimension: u32,
    /// Byte offset to the first vector record.
    pub data_offset: u32,
}

impl PageHeader {
    /// Encode the page header.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20);
        buf.put_u32_le(self.magic);
        buf.put_u32_le(self.version);
        buf.put_u32_le(self.count);
        buf.put_u32_le(self.dimension);
        buf.put_u32_le(self.data_offset);
        buf
    }

    /// Decode a page header from bytes.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        if buf.len() < 20 {
            return Err(crate::Error::corruption("page header too short"));
        }
        let mut cursor = buf;
        let magic = cursor.get_u32_le();
        let version = cursor.get_u32_le();
        let count = cursor.get_u32_le();
        let dimension = cursor.get_u32_le();
        let data_offset = cursor.get_u32_le();
        if magic != MAGIC {
            return Err(crate::Error::corruption("bad page magic"));
        }
        if version != VERSION {
            return Err(crate::Error::corruption("bad page version"));
        }
        Ok(Self {
            magic,
            version,
            count,
            dimension,
            data_offset,
        })
    }
}

/// A single vector record inside a page file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorRecord {
    /// Internal vector id.
    pub id: u64,
    /// User key.
    pub key: Vec<u8>,
    /// Raw vector components.
    pub vector: Vec<f32>,
}

impl VectorRecord {
    /// Encode a vector record.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8 + 4 + self.key.len() + 4 + self.vector.len() * 4);
        buf.put_u64_le(self.id);
        buf.put_u32_le(self.key.len() as u32);
        buf.extend_from_slice(&self.key);
        buf.put_u32_le(self.vector.len() as u32);
        for &x in &self.vector {
            buf.put_f32_le(x);
        }
        buf
    }

    /// Decode a vector record from bytes.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        if buf.len() < 16 {
            return Err(crate::Error::corruption("vector record too short"));
        }
        let id = storage_format::read_u64_le(buf);
        let key_len = storage_format::read_u32_le(&buf[8..]) as usize;
        if buf.len() < 16 + key_len {
            return Err(crate::Error::corruption("vector record key truncated"));
        }
        let key = buf[12..12 + key_len].to_vec();
        let vec_len = storage_format::read_u32_le(&buf[12 + key_len..]) as usize;
        let expected_len = 16 + key_len + vec_len * 4;
        if buf.len() < expected_len {
            return Err(crate::Error::corruption("vector record vector truncated"));
        }
        let mut vector = Vec::with_capacity(vec_len);
        let vec_start = 16 + key_len;
        for i in 0..vec_len {
            let bytes = &buf[vec_start + i * 4..vec_start + (i + 1) * 4];
            vector.push(f32::from_le_bytes(bytes.try_into().map_err(|_| {
                crate::Error::corruption("invalid f32 bytes")
            })?));
        }
        Ok(Self { id, key, vector })
    }
}

/// Encode a `Vec<f32>` into little-endian bytes.
pub fn encode_f32_vec(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4 + 4);
    buf.put_u32_le(v.len() as u32);
    for &x in v {
        buf.put_f32_le(x);
    }
    buf
}

/// Decode a `Vec<f32>` from little-endian bytes.
pub fn decode_f32_vec(buf: &[u8]) -> crate::Result<Vec<f32>> {
    if buf.len() < 4 {
        return Err(crate::Error::corruption("f32 vector buffer too short"));
    }
    let mut cursor = buf;
    let len = cursor.get_u32_le() as usize;
    if buf.len() < 4 + len * 4 {
        return Err(crate::Error::corruption("f32 vector truncated"));
    }
    let mut v = Vec::with_capacity(len);
    for _ in 0..len {
        v.push(cursor.get_f32_le());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let meta = Metadata::new(VectorOptions::default());
        let encoded = meta.encode().unwrap();
        let decoded = Metadata::decode(&encoded).unwrap();
        assert_eq!(meta, decoded);
    }

    #[test]
    fn wal_record_roundtrip() {
        let rec = WalRecord::Put {
            key: b"hello".to_vec(),
            vector: vec![1.0f32, 2.0, 3.0],
        };
        let encoded = rec.encode().unwrap();
        let decoded = WalRecord::decode(&encoded).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn vector_record_roundtrip() {
        let rec = VectorRecord {
            id: 7,
            key: b"doc".to_vec(),
            vector: vec![0.1f32, 0.2, 0.3],
        };
        let encoded = rec.encode();
        let decoded = VectorRecord::decode(&encoded).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn page_header_roundtrip() {
        let header = PageHeader {
            magic: MAGIC,
            version: VERSION,
            count: 100,
            dimension: 128,
            data_offset: 20,
        };
        let encoded = header.encode();
        let decoded = PageHeader::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }
}
