//! Manifest delta records encoded for the underlying `storage_wal`.

use bytes::{Buf, BufMut};

use crate::manifest::{FileMeta, Manifest};
use crate::{Error, Result};

const TAG_ADD_FILE: u8 = 1;
const TAG_SET_SCHEMA: u8 = 2;
const TAG_COMPACT: u8 = 3;

/// A single manifest delta record.
#[derive(Debug, Clone, PartialEq)]
pub enum ManifestRecord {
    /// Add a live Parquet file.
    AddFile {
        /// File metadata, including column statistics.
        file_meta: FileMeta,
    },
    /// Replace the table schema.
    SetSchema {
        /// JSON-encoded `TableSchema`.
        schema_json: String,
    },
    /// Atomically replace a set of files with a new set of files.
    Compact {
        /// New files to add.
        add: Vec<FileMeta>,
        /// Paths of files to remove.
        remove: Vec<std::path::PathBuf>,
    },
}

impl ManifestRecord {
    /// Serialize the record as `tag(1) | len(u64) | payload`.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            ManifestRecord::AddFile { file_meta } => {
                let mut buf = Vec::new();
                buf.put_u8(TAG_ADD_FILE);
                let json =
                    serde_json::to_vec(file_meta).expect("FileMeta serialization cannot fail");
                buf.put_u64_le(json.len() as u64);
                buf.extend_from_slice(&json);
                buf
            }
            ManifestRecord::SetSchema { schema_json } => {
                let mut buf = Vec::new();
                buf.put_u8(TAG_SET_SCHEMA);
                let bytes = schema_json.as_bytes();
                buf.put_u64_le(bytes.len() as u64);
                buf.extend_from_slice(bytes);
                buf
            }
            ManifestRecord::Compact { add, remove } => {
                let mut buf = Vec::new();
                buf.put_u8(TAG_COMPACT);
                let payload = CompactPayload {
                    add: add.clone(),
                    remove: remove.clone(),
                };
                let json =
                    serde_json::to_vec(&payload).expect("CompactPayload serialization cannot fail");
                buf.put_u64_le(json.len() as u64);
                buf.extend_from_slice(&json);
                buf
            }
        }
    }

    /// Decode a record from a byte slice.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 9 {
            return Err(Error::ManifestWal(format!(
                "record too short: {} bytes",
                bytes.len()
            )));
        }
        let mut cursor = bytes;
        let tag = cursor.get_u8();
        let len = cursor.get_u64_le() as usize;
        if cursor.len() < len {
            return Err(Error::ManifestWal(format!(
                "payload length {len} exceeds remaining {} bytes",
                cursor.len()
            )));
        }
        let payload = &cursor[..len];
        match tag {
            TAG_ADD_FILE => {
                let file_meta = serde_json::from_slice(payload).map_err(|e| {
                    Error::ManifestWal(format!("failed to decode AddFile payload: {e}"))
                })?;
                Ok(ManifestRecord::AddFile { file_meta })
            }
            TAG_SET_SCHEMA => {
                let schema_json = String::from_utf8(payload.to_vec()).map_err(|e| {
                    Error::ManifestWal(format!("failed to decode SetSchema payload: {e}"))
                })?;
                Ok(ManifestRecord::SetSchema { schema_json })
            }
            TAG_COMPACT => {
                let compact: CompactPayload = serde_json::from_slice(payload).map_err(|e| {
                    Error::ManifestWal(format!("failed to decode Compact payload: {e}"))
                })?;
                Ok(ManifestRecord::Compact {
                    add: compact.add,
                    remove: compact.remove,
                })
            }
            other => Err(Error::ManifestWal(format!(
                "unknown manifest record tag {other}"
            ))),
        }
    }
}

/// Apply a decoded manifest record to an in-memory manifest.
pub fn apply_record(manifest: &mut Manifest, record: ManifestRecord) -> Result<()> {
    match record {
        ManifestRecord::AddFile { file_meta } => {
            manifest.files.push(file_meta);
        }
        ManifestRecord::SetSchema { schema_json } => {
            manifest.schema = serde_json::from_str(&schema_json).map_err(|e| {
                Error::ManifestWal(format!("failed to apply SetSchema record: {e}"))
            })?;
        }
        ManifestRecord::Compact { add, remove } => {
            let remove_set: std::collections::HashSet<_> = remove.into_iter().collect();
            manifest.files.retain(|f| !remove_set.contains(&f.path));
            for file_meta in add {
                manifest.files.push(file_meta);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CompactPayload {
    add: Vec<FileMeta>,
    remove: Vec<std::path::PathBuf>,
}
