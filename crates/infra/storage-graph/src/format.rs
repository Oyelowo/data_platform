//! On-disk format definitions for `storage-graph`.
//!
//! All multi-byte integers are little-endian. Metadata, node/edge records, and
//! WAL records carry CRC32C checksums so that torn writes and corruption are
//! detected on open.

use bytes::{Buf, BufMut};
use serde::{Deserialize, Serialize};

use crate::id::{InternalEdgeId, InternalNodeId};
use crate::model::{Edge, Node, PropertyMap};
use crate::options::GraphOptions;

/// Magic number for graph engine files.
pub const MAGIC: u32 = 0x47_50_45_31; // "GPE1"

/// Current on-disk format version.
pub const VERSION: u32 = 1;

/// File name for the engine metadata file.
pub const META_FILE: &str = "META";

/// Subdirectory for WAL segments.
pub const WAL_DIR: &str = "WAL";

/// File name for persisted index snapshot.
pub const INDEX_FILE: &str = "INDEX";

/// File name for the append-only node store.
pub const NODE_STORE_FILE: &str = "NODES";

/// File name for the append-only edge store.
pub const EDGE_STORE_FILE: &str = "EDGES";

/// Prefix for composite node keys in the `Engine` trait API.
pub const NODE_KEY_PREFIX: &[u8] = b"node:";

/// Prefix for composite edge keys in the `Engine` trait API.
pub const EDGE_KEY_PREFIX: &[u8] = b"edge:";

/// Prefix for composite node property keys in the `Engine` trait API.
pub const NODE_PROP_PREFIX: &[u8] = b"prop:node:";

/// Prefix for composite edge property keys in the `Engine` trait API.
pub const EDGE_PROP_PREFIX: &[u8] = b"prop:edge:";

/// Stable address of a record in an append-only store file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RecordAddress {
    /// Logical file identifier.
    pub file_id: u64,
    /// Byte offset of the record payload within the file.
    pub offset: u64,
    /// Length of the record payload in bytes.
    pub len: u32,
}

impl RecordAddress {
    /// Create a new record address.
    pub fn new(file_id: u64, offset: u64, len: u32) -> Self {
        Self {
            file_id,
            offset,
            len,
        }
    }

    /// Encode the address to a compact byte representation.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20);
        buf.put_u64_le(self.file_id);
        buf.put_u64_le(self.offset);
        buf.put_u32_le(self.len);
        buf
    }

    /// Decode an address from bytes.
    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        if buf.len() < 20 {
            return Err(crate::Error::corruption("record address too short"));
        }
        let mut cursor = buf;
        let file_id = cursor.get_u64_le();
        let offset = cursor.get_u64_le();
        let len = cursor.get_u32_le();
        Ok(Self {
            file_id,
            offset,
            len,
        })
    }
}

/// Encoded form of a node as stored on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeRecord {
    /// User id.
    pub id: Vec<u8>,
    /// Labels.
    pub labels: Vec<String>,
    /// Properties.
    pub properties: PropertyMap,
}

impl From<&Node> for NodeRecord {
    fn from(node: &Node) -> Self {
        Self {
            id: node.id.clone(),
            labels: node.labels.iter().cloned().collect(),
            properties: node.properties.clone(),
        }
    }
}

impl From<NodeRecord> for Node {
    fn from(record: NodeRecord) -> Self {
        Self {
            id: record.id,
            labels: record.labels.into_iter().collect(),
            properties: record.properties,
        }
    }
}

/// Encoded form of an edge as stored on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeRecord {
    /// User id.
    pub id: Vec<u8>,
    /// Source node id.
    pub from: Vec<u8>,
    /// Target node id.
    pub to: Vec<u8>,
    /// Label.
    pub label: String,
    /// Properties.
    pub properties: PropertyMap,
}

impl From<&Edge> for EdgeRecord {
    fn from(edge: &Edge) -> Self {
        Self {
            id: edge.id.clone(),
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            properties: edge.properties.clone(),
        }
    }
}

impl From<EdgeRecord> for Edge {
    fn from(record: EdgeRecord) -> Self {
        Self {
            id: record.id,
            from: record.from,
            to: record.to,
            label: record.label,
            properties: record.properties,
        }
    }
}

/// On-disk metadata header for a graph engine database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// Format magic.
    pub magic: u32,
    /// Format version.
    pub version: u32,
    /// Engine options.
    pub options: GraphOptions,
    /// Next internal node id to assign.
    pub next_node_id: InternalNodeId,
    /// Next internal edge id to assign.
    pub next_edge_id: InternalEdgeId,
    /// Current node store file id.
    pub node_file_id: u64,
    /// Current edge store file id.
    pub edge_file_id: u64,
    /// Last WAL checkpoint LSN.
    pub wal_checkpoint_lsn: u64,
    /// CRC32C of the persisted index snapshot, if present.
    pub index_checksum: Option<u32>,
}

impl Metadata {
    /// Create metadata from validated options.
    pub fn new(options: GraphOptions) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            options,
            next_node_id: InternalNodeId(0),
            next_edge_id: InternalEdgeId(0),
            node_file_id: 0,
            edge_file_id: 0,
            wal_checkpoint_lsn: 0,
            index_checksum: None,
        }
    }

    /// Serialize metadata to bytes with a trailing CRC32C checksum.
    pub fn encode(&self) -> crate::Result<Vec<u8>> {
        let body = bincode::serialize(self).map_err(|e| crate::Error::corruption(e.to_string()))?;
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

/// A single WAL record payload for the graph engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalRecord {
    /// Create or replace a node.
    CreateNode {
        /// Node id.
        id: Vec<u8>,
        /// Labels.
        labels: Vec<String>,
        /// Properties.
        properties: PropertyMap,
    },
    /// Delete a node and its incident edges.
    DeleteNode {
        /// Node id.
        id: Vec<u8>,
    },
    /// Create or replace an edge.
    CreateEdge {
        /// Edge id.
        id: Vec<u8>,
        /// Source node id.
        from: Vec<u8>,
        /// Target node id.
        to: Vec<u8>,
        /// Label.
        label: String,
        /// Properties.
        properties: PropertyMap,
    },
    /// Delete an edge.
    DeleteEdge {
        /// Edge id.
        id: Vec<u8>,
    },
    /// Set or overwrite a node property.
    SetNodeProperty {
        /// Node id.
        id: Vec<u8>,
        /// Property key.
        key: String,
        /// Property value.
        value: Vec<u8>,
    },
    /// Delete a node property.
    DeleteNodeProperty {
        /// Node id.
        id: Vec<u8>,
        /// Property key.
        key: String,
    },
    /// Set or overwrite an edge property.
    SetEdgeProperty {
        /// Edge id.
        id: Vec<u8>,
        /// Property key.
        key: String,
        /// Property value.
        value: Vec<u8>,
    },
    /// Delete an edge property.
    DeleteEdgeProperty {
        /// Edge id.
        id: Vec<u8>,
        /// Property key.
        key: String,
    },
    /// Add a label to a node.
    AddNodeLabel {
        /// Node id.
        id: Vec<u8>,
        /// Label to add.
        label: String,
    },
    /// Remove a label from a node.
    RemoveNodeLabel {
        /// Node id.
        id: Vec<u8>,
        /// Label to remove.
        label: String,
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

/// Encode a node for storage with a leading length and CRC32C trailer.
pub fn encode_node_record(node: &NodeRecord) -> crate::Result<Vec<u8>> {
    let payload = bincode::serialize(node).map_err(|e| crate::Error::corruption(e.to_string()))?;
    let mut buf = Vec::with_capacity(4 + payload.len() + 4);
    buf.put_u32_le(payload.len() as u32);
    buf.extend_from_slice(&payload);
    let crc = storage_format::crc32c(&buf[..4 + payload.len()]);
    buf.put_u32_le(crc);
    Ok(buf)
}

/// Decode a node record from stored bytes, verifying its CRC32C.
pub fn decode_node_record(buf: &[u8]) -> crate::Result<NodeRecord> {
    if buf.len() < 8 {
        return Err(crate::Error::corruption("node record too short"));
    }
    let payload_len = storage_format::read_u32_le(buf) as usize;
    let expected = 4 + payload_len + 4;
    if buf.len() < expected {
        return Err(crate::Error::corruption("truncated node record"));
    }
    let stored_crc = storage_format::read_u32_le(&buf[4 + payload_len..]);
    let computed_crc = storage_format::crc32c(&buf[..4 + payload_len]);
    if stored_crc != computed_crc {
        return Err(crate::Error::corruption("node record checksum mismatch"));
    }
    bincode::deserialize(&buf[4..4 + payload_len]).map_err(|e| crate::Error::corruption(e.to_string()))
}

/// Encode an edge for storage with a leading length and CRC32C trailer.
pub fn encode_edge_record(edge: &EdgeRecord) -> crate::Result<Vec<u8>> {
    let payload = bincode::serialize(edge).map_err(|e| crate::Error::corruption(e.to_string()))?;
    let mut buf = Vec::with_capacity(4 + payload.len() + 4);
    buf.put_u32_le(payload.len() as u32);
    buf.extend_from_slice(&payload);
    let crc = storage_format::crc32c(&buf[..4 + payload.len()]);
    buf.put_u32_le(crc);
    Ok(buf)
}

/// Decode an edge record from stored bytes, verifying its CRC32C.
pub fn decode_edge_record(buf: &[u8]) -> crate::Result<EdgeRecord> {
    if buf.len() < 8 {
        return Err(crate::Error::corruption("edge record too short"));
    }
    let payload_len = storage_format::read_u32_le(buf) as usize;
    let expected = 4 + payload_len + 4;
    if buf.len() < expected {
        return Err(crate::Error::corruption("truncated edge record"));
    }
    let stored_crc = storage_format::read_u32_le(&buf[4 + payload_len..]);
    let computed_crc = storage_format::crc32c(&buf[..4 + payload_len]);
    if stored_crc != computed_crc {
        return Err(crate::Error::corruption("edge record checksum mismatch"));
    }
    bincode::deserialize(&buf[4..4 + payload_len]).map_err(|e| crate::Error::corruption(e.to_string()))
}

/// Encode an `Engine` trait key for a whole node.
pub fn encode_node_key(id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(NODE_KEY_PREFIX.len() + id.len());
    key.extend_from_slice(NODE_KEY_PREFIX);
    key.extend_from_slice(id);
    key
}

/// Encode an `Engine` trait key for a whole edge.
pub fn encode_edge_key(id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(EDGE_KEY_PREFIX.len() + id.len());
    key.extend_from_slice(EDGE_KEY_PREFIX);
    key.extend_from_slice(id);
    key
}

/// Encode an `Engine` trait key for a single node property.
pub fn encode_node_property_key(id: &[u8], property_key: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(NODE_PROP_PREFIX.len() + id.len() + 1 + property_key.len());
    key.extend_from_slice(NODE_PROP_PREFIX);
    key.extend_from_slice(id);
    key.push(b':');
    key.extend_from_slice(property_key.as_bytes());
    key
}

/// Encode an `Engine` trait key for a single edge property.
pub fn encode_edge_property_key(id: &[u8], property_key: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(EDGE_PROP_PREFIX.len() + id.len() + 1 + property_key.len());
    key.extend_from_slice(EDGE_PROP_PREFIX);
    key.extend_from_slice(id);
    key.push(b':');
    key.extend_from_slice(property_key.as_bytes());
    key
}

/// Decode an `Engine` trait key.
///
/// Returns `(kind, id, property_key)` where `kind` is `"node"` or `"edge"`
/// and `property_key` is `Some` for property keys.
pub fn decode_key(key: &[u8]) -> crate::Result<(&str, &[u8], Option<&str>)> {
    if let Some(rest) = key.strip_prefix(NODE_KEY_PREFIX) {
        return Ok(("node", rest, None));
    }
    if let Some(rest) = key.strip_prefix(EDGE_KEY_PREFIX) {
        return Ok(("edge", rest, None));
    }
    if let Some(rest) = key.strip_prefix(NODE_PROP_PREFIX) {
        let (id, prop) = split_property_suffix(rest)?;
        return Ok(("node", id, Some(prop)));
    }
    if let Some(rest) = key.strip_prefix(EDGE_PROP_PREFIX) {
        let (id, prop) = split_property_suffix(rest)?;
        return Ok(("edge", id, Some(prop)));
    }
    Err(crate::Error::corruption("unknown composite key prefix"))
}

fn split_property_suffix(rest: &[u8]) -> crate::Result<(&[u8], &str)> {
    match rest.iter().rposition(|&b| b == b':') {
        Some(pos) => {
            let id = &rest[..pos];
            let prop = std::str::from_utf8(&rest[pos + 1..])
                .map_err(|_| crate::Error::corruption("property key is not valid utf-8"))?;
            Ok((id, prop))
        }
        None => Err(crate::Error::corruption("missing property key separator")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let meta = Metadata::new(GraphOptions::default());
        let encoded = meta.encode().unwrap();
        let decoded = Metadata::decode(&encoded).unwrap();
        assert_eq!(meta, decoded);
    }

    #[test]
    fn wal_record_roundtrip() {
        let rec = WalRecord::CreateNode {
            id: b"n1".to_vec(),
            labels: vec!["User".into()],
            properties: PropertyMap::from([("name".into(), b"Ada".to_vec())]),
        };
        let encoded = rec.encode().unwrap();
        let decoded = WalRecord::decode(&encoded).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn node_record_encoding_roundtrip() {
        let node = NodeRecord {
            id: b"n1".to_vec(),
            labels: vec!["User".into()],
            properties: PropertyMap::from([("name".into(), b"Ada".to_vec())]),
        };
        let encoded = encode_node_record(&node).unwrap();
        let decoded = decode_node_record(&encoded).unwrap();
        assert_eq!(node, decoded);
    }

    #[test]
    fn edge_record_encoding_roundtrip() {
        let edge = EdgeRecord {
            id: b"e1".to_vec(),
            from: b"n1".to_vec(),
            to: b"n2".to_vec(),
            label: "FOLLOWS".into(),
            properties: PropertyMap::new(),
        };
        let encoded = encode_edge_record(&edge).unwrap();
        let decoded = decode_edge_record(&encoded).unwrap();
        assert_eq!(edge, decoded);
    }

    #[test]
    fn record_address_roundtrip() {
        let addr = RecordAddress::new(1, 1024, 256);
        let encoded = addr.encode();
        let decoded = RecordAddress::decode(&encoded).unwrap();
        assert_eq!(addr, decoded);
    }

    #[test]
    fn composite_keys() {
        let nk = encode_node_key(b"n1");
        let (kind, id, prop) = decode_key(&nk).unwrap();
        assert_eq!(kind, "node");
        assert_eq!(id, b"n1");
        assert_eq!(prop, None);

        let npk = encode_node_property_key(b"n1", "name");
        let (kind, id, prop) = decode_key(&npk).unwrap();
        assert_eq!(kind, "node");
        assert_eq!(id, b"n1");
        assert_eq!(prop, Some("name"));

        let ek = encode_edge_key(b"e1");
        let (kind, id, prop) = decode_key(&ek).unwrap();
        assert_eq!(kind, "edge");
        assert_eq!(id, b"e1");
        assert_eq!(prop, None);

        let epk = encode_edge_property_key(b"e1", "since");
        let (kind, id, prop) = decode_key(&epk).unwrap();
        assert_eq!(kind, "edge");
        assert_eq!(id, b"e1");
        assert_eq!(prop, Some("since"));
    }
}
