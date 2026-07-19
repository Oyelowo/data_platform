//! On-disk format definitions for `storage-search`.

use bytes::BufMut;
use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::options::SearchOptions;
use crate::schema::{FieldValue, Schema};

/// Magic number for search engine files.
pub const MAGIC: u32 = 0x53_45_41_52; // "SEAR"

/// Current on-disk format version.
pub const VERSION: u32 = 1;

/// File name for the engine metadata file.
pub const META_FILE: &str = "META";

/// Subdirectory for WAL segments.
pub const WAL_DIR: &str = "WAL";

/// Subdirectory for segment files.
pub const SEGMENTS_DIR: &str = "segments";

/// On-disk metadata header for a search engine database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// Format magic.
    pub magic: u32,
    /// Format version.
    pub version: u32,
    /// Engine options.
    pub options: SearchOptions,
    /// Document schema.
    pub schema: Schema,
    /// Ordered list of segment ids in the catalog.
    pub segment_ids: Vec<u64>,
    /// Last WAL checkpoint LSN.
    pub wal_checkpoint_lsn: u64,
}

impl Metadata {
    /// Create metadata from validated options and schema.
    pub fn new(options: SearchOptions, schema: Schema) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            options,
            schema,
            segment_ids: Vec::new(),
            wal_checkpoint_lsn: 0,
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

/// A single WAL record payload for the search engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalRecord {
    /// Index or replace a document.
    IndexDocument {
        /// Document identifier.
        doc_id: Vec<u8>,
        /// Stored document.
        document: Document,
    },
    /// Delete a document.
    DeleteDocument {
        /// Document identifier.
        doc_id: Vec<u8>,
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

/// Encode a stored field value for the `Engine` byte-key API.
pub fn encode_field_value(value: &FieldValue) -> Vec<u8> {
    match value {
        FieldValue::Text(t) => {
            let mut buf = Vec::with_capacity(1 + t.len());
            buf.push(0u8);
            buf.extend_from_slice(t.as_bytes());
            buf
        }
        FieldValue::Bytes(b) => {
            let mut buf = Vec::with_capacity(1 + b.len());
            buf.push(1u8);
            buf.extend_from_slice(b);
            buf
        }
    }
}

/// Decode a stored field value from the `Engine` byte-key API.
pub fn decode_field_value(buf: &[u8]) -> crate::Result<FieldValue> {
    if buf.is_empty() {
        return Err(crate::Error::corruption("empty field value bytes"));
    }
    match buf[0] {
        0 => {
            let text = std::str::from_utf8(&buf[1..])
                .map_err(|_| crate::Error::corruption("invalid utf-8 text field"))?;
            Ok(FieldValue::Text(text.to_string()))
        }
        1 => Ok(FieldValue::Bytes(buf[1..].to_vec())),
        other => Err(crate::Error::corruption(format!(
            "unknown field value kind byte {other}"
        ))),
    }
}

/// Encode an `Engine` key from `(doc_id, field_name)`.
pub fn encode_engine_key(doc_id: &[u8], field_name: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(doc_id.len() + field_name.len() + 1);
    key.extend_from_slice(doc_id);
    key.push(0u8);
    key.extend_from_slice(field_name.as_bytes());
    key
}

/// Decode an `Engine` key into `(doc_id, field_name)`.
pub fn decode_engine_key(key: &[u8]) -> crate::Result<(&[u8], &str)> {
    let pos = key
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| crate::Error::corruption("engine key missing field separator"))?;
    let doc_id = &key[..pos];
    let field_name = std::str::from_utf8(&key[pos + 1..])
        .map_err(|_| crate::Error::corruption("engine key field name is not utf-8"))?;
    Ok((doc_id, field_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let meta = Metadata::new(SearchOptions::default(), Schema::new());
        let encoded = meta.encode().unwrap();
        let decoded = Metadata::decode(&encoded).unwrap();
        assert_eq!(meta, decoded);
    }

    #[test]
    fn wal_record_roundtrip() {
        let rec = WalRecord::IndexDocument {
            doc_id: b"doc1".to_vec(),
            document: Document::new().with_text("title", "hello"),
        };
        let encoded = rec.encode().unwrap();
        let decoded = WalRecord::decode(&encoded).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn engine_key_roundtrip() {
        let key = encode_engine_key(b"doc1", "title");
        let (doc_id, field) = decode_engine_key(&key).unwrap();
        assert_eq!(doc_id, b"doc1");
        assert_eq!(field, "title");
    }
}
