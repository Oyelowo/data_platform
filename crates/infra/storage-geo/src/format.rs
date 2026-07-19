//! On-disk format definitions for `storage-geo`.
//!
//! All multi-byte integers are little-endian. Metadata, feature records, and the
//! persisted R-tree carry CRC32C checksums.

use std::collections::BTreeMap;

use bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use crate::feature::Geometry;
use crate::options::GeoOptions;
use crate::store::FeatureAddress;

/// Magic number for geospatial engine files.
pub const MAGIC: u32 = 0x47_45_4F_31; // "GEO1"

/// Current on-disk format version.
pub const VERSION: u32 = 1;

/// File name for the engine metadata file.
pub const META_FILE: &str = "META";

/// Subdirectory for WAL segment files.
pub const WAL_DIR: &str = "WAL";

/// File name prefix for the append-only feature store.
pub const STORE_FILE_PREFIX: &str = "features";

/// File name for the persisted R-tree index.
pub const INDEX_FILE: &str = "rtree.index";

/// Separator used in composite `Engine` trait keys.
pub const KEY_SEPARATOR: u8 = 0;

/// On-disk metadata header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// Format magic.
    pub magic: u32,
    /// Format version.
    pub version: u32,
    /// Engine options.
    pub options: GeoOptions,
    /// Current feature store file id.
    pub store_file_id: u32,
    /// Map from feature id to its current on-disk address.
    pub live: BTreeMap<Vec<u8>, FeatureAddress>,
    /// Approximate number of deleted/updated bytes awaiting compaction.
    pub stale_bytes: u64,
    /// WAL checkpoint LSN. Records with LSN `<=` this value are reflected in the
    /// on-disk store/index. `None` means no checkpoint has been taken yet.
    pub wal_checkpoint_lsn: Option<u64>,
}

impl Metadata {
    /// Create metadata from validated options.
    pub fn new(options: GeoOptions) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            options,
            store_file_id: 0,
            live: BTreeMap::new(),
            stale_bytes: 0,
            wal_checkpoint_lsn: None,
        }
    }

    /// Return the relative store file name for the current `store_file_id`.
    pub fn store_file_name(&self) -> String {
        store_file_name(self.store_file_id)
    }

    /// Serialize metadata to bytes with a trailing CRC32C checksum.
    pub fn encode(&self) -> crate::Result<Vec<u8>> {
        let body = bincode::serialize(self)
            .map_err(|e| crate::Error::corruption(e.to_string()))?;
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
            .map_err(|e| crate::Error::corruption(e.to_string()))?;
        Ok(meta)
    }
}

/// Build the file name for a feature store with the given file id.
pub fn store_file_name(file_id: u32) -> String {
    format!("{STORE_FILE_PREFIX}_{file_id:08x}")
}

/// A single WAL record payload for the geospatial engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalRecord {
    /// Insert or replace a feature.
    InsertFeature {
        /// Feature id.
        id: Vec<u8>,
        /// WKB-encoded geometry.
        geometry: Vec<u8>,
        /// JSON-encoded properties.
        properties: Vec<u8>,
    },
    /// Delete a feature.
    DeleteFeature {
        /// Feature id.
        id: Vec<u8>,
    },
    /// Update the properties of an existing feature.
    UpdateProperties {
        /// Feature id.
        id: Vec<u8>,
        /// JSON-encoded properties.
        properties: Vec<u8>,
    },
}

impl WalRecord {
    /// Encode a WAL record to bytes.
    pub fn encode(&self) -> crate::Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| crate::Error::corruption(e.to_string()))
    }

    /// Decode a WAL record from bytes.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        bincode::deserialize(buf).map_err(|e| crate::Error::corruption(e.to_string()))
    }
}

/// Persistent representation of a feature in the append-only store.
pub struct FeatureRecord;

impl FeatureRecord {
    /// Encode a feature into its on-disk representation.
    ///
    /// Layout: `[record_len][id_len][id][geom_len][geometry][props_len][props][crc]`.
    /// All length fields are little-endian `u32`. `record_len` is the length of
    /// everything that follows it, including the trailing CRC.
    pub fn encode(feature: &crate::feature::Feature) -> crate::Result<Vec<u8>> {
        let geom = crate::wkb::encode(&feature.geometry)?;
        let props = serde_json::to_vec(&feature.properties)
            .map_err(|e| crate::Error::property_encoding(e.to_string()))?;

        let body_len = 4
            + feature.id.len()
            + 4
            + geom.len()
            + 4
            + props.len()
            + 4;
        let mut buf = Vec::with_capacity(4 + body_len);
        buf.put_u32_le(body_len as u32);
        buf.put_u32_le(feature.id.len() as u32);
        buf.extend_from_slice(&feature.id);
        buf.put_u32_le(geom.len() as u32);
        buf.extend_from_slice(&geom);
        buf.put_u32_le(props.len() as u32);
        buf.extend_from_slice(&props);
        let crc = storage_format::crc32c(&buf[4..]);
        buf.put_u32_le(crc);
        Ok(buf)
    }

    /// Decode a feature record from raw bytes.
    pub fn decode(data: &[u8]) -> crate::Result<crate::feature::Feature> {
        if data.len() < 4 {
            return Err(crate::Error::corruption("feature record too short"));
        }
        let mut cursor = data;
        let record_len = cursor.get_u32_le() as usize;
        if cursor.len() < record_len {
            return Err(crate::Error::corruption("feature record body truncated"));
        }
        if cursor.len() > record_len {
            // Only the first `record_len` bytes belong to this record.
            cursor = &cursor[..record_len];
        }

        let id_len = cursor.get_u32_le() as usize;
        if cursor.len() < id_len {
            return Err(crate::Error::corruption("feature record id truncated"));
        }
        let id = cursor[..id_len].to_vec();
        cursor.advance(id_len);

        let geom_len = cursor.get_u32_le() as usize;
        if cursor.len() < geom_len {
            return Err(crate::Error::corruption("feature record geometry truncated"));
        }
        let geometry = crate::wkb::decode(&cursor[..geom_len])?;
        cursor.advance(geom_len);

        let props_len = cursor.get_u32_le() as usize;
        if cursor.len() < props_len + 4 {
            return Err(crate::Error::corruption(
                "feature record properties truncated",
            ));
        }
        let properties = serde_json::from_slice(&cursor[..props_len])
            .map_err(|e| crate::Error::property_encoding(e.to_string()))?;
        cursor.advance(props_len);

        let stored_crc = cursor.get_u32_le();
        let computed_crc = storage_format::crc32c(&data[4..4 + record_len - 4]);
        if stored_crc != computed_crc {
            return Err(crate::Error::corruption("feature record checksum mismatch"));
        }

        Ok(crate::feature::Feature {
            id,
            geometry,
            properties,
        })
    }
}

/// Encode a feature id for the `Engine` trait API.
pub fn encode_id_key(id: &[u8]) -> Vec<u8> {
    id.to_vec()
}

/// Encode a composite `(feature_id, property_key)` key.
pub fn encode_property_key(id: &[u8], property_key: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(id.len() + 1 + property_key.len());
    key.extend_from_slice(id);
    key.push(KEY_SEPARATOR);
    key.extend_from_slice(property_key.as_bytes());
    key
}

/// Decode an `Engine` trait key.
///
/// Returns `(feature_id, None)` for a bare id and `(feature_id, Some(property_key))`
/// for a composite key.
pub fn decode_key(key: &[u8]) -> crate::Result<(&[u8], Option<&str>)> {
    if key.is_empty() {
        return Err(crate::Error::invalid_argument("empty engine key"));
    }
    match key.iter().position(|&b| b == KEY_SEPARATOR) {
        Some(pos) => {
            let property = std::str::from_utf8(&key[pos + 1..])
                .map_err(|_| crate::Error::corruption("property key is not valid utf-8"))?;
            Ok((&key[..pos], Some(property)))
        }
        None => Ok((key, None)),
    }
}

/// Encode the full feature as an opaque byte value for the `Engine` trait.
pub fn encode_feature_value(feature: &crate::feature::Feature) -> crate::Result<Vec<u8>> {
    FeatureRecord::encode(feature)
}

/// Decode a feature from an opaque byte value.
pub fn decode_feature_value(value: &[u8]) -> crate::Result<crate::feature::Feature> {
    FeatureRecord::decode(value)
}

/// Encode a single property value for the `Engine` trait API.
pub fn encode_property_value(value: &[u8]) -> Vec<u8> {
    value.to_vec()
}

/// Decode a single property value from the `Engine` trait API.
pub fn decode_property_value(value: &[u8]) -> Vec<u8> {
    value.to_vec()
}

/// Encode a [`Geometry`] into a compact byte value for external consumers.
pub fn encode_geometry_value(geometry: &Geometry) -> crate::Result<Vec<u8>> {
    crate::wkb::encode(geometry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{Feature, Geometry, PropertyMap};
    use geo::Point;

    #[test]
    fn metadata_roundtrip() {
        let mut meta = Metadata::new(GeoOptions::default());
        meta.live.insert(
            b"f1".to_vec(),
            FeatureAddress {
                file_id: 0,
                offset: 42,
                len: 100,
            },
        );
        let encoded = meta.encode().unwrap();
        let decoded = Metadata::decode(&encoded).unwrap();
        assert_eq!(meta, decoded);
    }

    #[test]
    fn wal_record_roundtrip() {
        let rec = WalRecord::InsertFeature {
            id: b"f1".to_vec(),
            geometry: vec![1, 2, 3],
            properties: vec![4, 5],
        };
        let encoded = rec.encode().unwrap();
        let decoded = WalRecord::decode(&encoded).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn feature_record_roundtrip() {
        let mut props = PropertyMap::new();
        props.insert("name".to_string(), b"paris".to_vec());
        let feature = Feature::new(
            b"city/1",
            Geometry::Point(Point::new(2.35, 48.85)),
            props,
        );
        let encoded = FeatureRecord::encode(&feature).unwrap();
        let decoded = FeatureRecord::decode(&encoded).unwrap();
        assert_eq!(feature, decoded);
    }

    #[test]
    fn composite_key_roundtrip() {
        let key = encode_property_key(b"f1", "name");
        let (id, prop) = decode_key(&key).unwrap();
        assert_eq!(id, b"f1");
        assert_eq!(prop, Some("name"));

        let id_key = encode_id_key(b"f1");
        let (id2, prop2) = decode_key(&id_key).unwrap();
        assert_eq!(id2, b"f1");
        assert_eq!(prop2, None);
    }
}
