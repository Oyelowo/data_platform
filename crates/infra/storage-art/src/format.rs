//! On-disk format for the durable `ArtEngine`.
//!
//! This module defines the metadata file (`art.meta`) and the WAL record format
//! used by `ArtEngine`. The actual tree snapshot is stored in `snapshot.bin`
//! using the format in [`crate::snapshot`]; this module only stores the
//! snapshot's CRC and LSN in metadata so that recovery can verify and locate it.

use std::path::{Path, PathBuf};

use bytes::{Buf, BufMut, Bytes};
use storage_format::{crc32c, decode_uvarint, write_uvarint};

use crate::options::{ArtEngineOptions, WalSyncPolicy};
use crate::{Error, Result};

/// Magic number at the start of `art.meta`.
///
/// Reads as "ART\x01" in little-endian (the final byte distinguishes the
/// durable engine format from the in-memory snapshot magic).
pub const META_MAGIC: u32 = 0x4152_5401;

/// Current metadata format version.
pub const META_VERSION: u32 = 1;

/// On-disk metadata for `ArtEngine`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    /// Engine options persisted at creation time.
    pub options: ArtEngineOptions,
    /// LSN of the WAL record that corresponds to the snapshot.
    pub last_snapshot_lsn: u64,
    /// CRC32C of `snapshot.bin`.
    pub snapshot_crc: u32,
}

impl Metadata {
    /// Create metadata for a freshly created engine.
    pub fn new(options: ArtEngineOptions) -> Self {
        Self {
            options,
            last_snapshot_lsn: 0,
            snapshot_crc: 0,
        }
    }

    /// Encode metadata to a byte vector with a trailing CRC32C.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.put_u32_le(META_MAGIC);
        body.put_u32_le(META_VERSION);

        // ArtMapOptions
        write_uvarint(&mut body, self.options.map.max_key_len as u64).unwrap();
        write_uvarint(&mut body, self.options.map.max_value_len as u64).unwrap();
        write_uvarint(&mut body, self.options.map.max_entries.unwrap_or(0) as u64).unwrap();

        // Engine-specific options
        body.push(match self.options.wal_sync_policy {
            WalSyncPolicy::Immediate => 0,
            WalSyncPolicy::Buffered => 1,
        });
        write_uvarint(&mut body, self.options.wal_segment_size).unwrap();
        body.push(u8::from(self.options.snapshot_on_sync));

        // Snapshot location / checksum
        body.put_u64_le(self.last_snapshot_lsn);
        body.put_u32_le(self.snapshot_crc);

        let crc = crc32c(&body);
        body.put_u32_le(crc);
        body
    }

    /// Decode metadata from bytes and verify magic, version, and CRC.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 + 4 {
            return Err(Error::Corruption("metadata too short".into()));
        }
        let body_len = bytes.len() - 4;
        let body = &bytes[..body_len];
        let stored_crc = crc32c(body);
        let computed_crc = u32::from_le_bytes([
            bytes[body_len],
            bytes[body_len + 1],
            bytes[body_len + 2],
            bytes[body_len + 3],
        ]);
        if stored_crc != computed_crc {
            return Err(Error::Corruption(format!(
                "metadata crc mismatch: expected {stored_crc:#x}, got {computed_crc:#x}"
            )));
        }

        let mut cursor = body;
        let magic = cursor.get_u32_le();
        if magic != META_MAGIC {
            return Err(Error::Corruption(format!(
                "bad metadata magic: expected {:#x}, got {:#x}",
                META_MAGIC, magic
            )));
        }
        let version = cursor.get_u32_le();
        if version != META_VERSION {
            return Err(Error::Corruption(format!(
                "unsupported metadata version {version}"
            )));
        }

        let (max_key_len, n) = decode_uvarint(cursor)
            .map_err(|e| Error::Corruption(format!("metadata max_key_len: {e}")))?;
        cursor.advance(n);
        let (max_value_len, n) = decode_uvarint(cursor)
            .map_err(|e| Error::Corruption(format!("metadata max_value_len: {e}")))?;
        cursor.advance(n);
        let (max_entries_raw, n) = decode_uvarint(cursor)
            .map_err(|e| Error::Corruption(format!("metadata max_entries: {e}")))?;
        cursor.advance(n);
        let max_entries = if max_entries_raw == 0 {
            None
        } else {
            Some(max_entries_raw as usize)
        };

        if cursor.is_empty() {
            return Err(Error::Corruption("metadata truncated before policy".into()));
        }
        let wal_sync_policy = match cursor.get_u8() {
            0 => WalSyncPolicy::Immediate,
            1 => WalSyncPolicy::Buffered,
            other => {
                return Err(Error::Corruption(format!(
                    "unknown wal_sync_policy {other}"
                )));
            }
        };
        let (wal_segment_size, n) = decode_uvarint(cursor)
            .map_err(|e| Error::Corruption(format!("metadata wal_segment_size: {e}")))?;
        cursor.advance(n);
        if cursor.is_empty() {
            return Err(Error::Corruption(
                "metadata truncated before snapshot_on_sync".into(),
            ));
        }
        let snapshot_on_sync = cursor.get_u8() != 0;

        if cursor.len() < 12 {
            return Err(Error::Corruption(
                "metadata truncated before lsn/crc".into(),
            ));
        }
        let last_snapshot_lsn = cursor.get_u64_le();
        let snapshot_crc = cursor.get_u32_le();

        Ok(Self {
            options: ArtEngineOptions {
                map: crate::options::ArtMapOptions {
                    max_key_len: max_key_len as usize,
                    max_value_len: max_value_len as usize,
                    max_entries,
                },
                wal_sync_policy,
                wal_segment_size,
                snapshot_on_sync,
            },
            last_snapshot_lsn,
            snapshot_crc,
        })
    }
}

/// A single operation inside a batch WAL record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchOp {
    /// Insert or overwrite a key.
    Put {
        /// Key bytes.
        key: Vec<u8>,
        /// Value bytes.
        value: Vec<u8>,
    },
    /// Delete a key.
    Delete {
        /// Key bytes.
        key: Vec<u8>,
    },
}

/// A logical record stored in the WAL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalRecord {
    /// Insert or overwrite a single key.
    Put {
        /// Key bytes.
        key: Vec<u8>,
        /// Value bytes.
        value: Vec<u8>,
    },
    /// Delete a single key.
    Delete {
        /// Key bytes.
        key: Vec<u8>,
    },
    /// A batch of operations applied atomically in memory.
    Batch(Vec<BatchOp>),
}

impl WalRecord {
    /// Encode the record into a byte vector.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            WalRecord::Put { key, value } => {
                out.push(1);
                encode_bytes(&mut out, key);
                encode_bytes(&mut out, value);
            }
            WalRecord::Delete { key } => {
                out.push(2);
                encode_bytes(&mut out, key);
            }
            WalRecord::Batch(ops) => {
                out.push(3);
                write_uvarint(&mut out, ops.len() as u64).unwrap();
                for op in ops {
                    match op {
                        BatchOp::Put { key, value } => {
                            out.push(1);
                            encode_bytes(&mut out, key);
                            encode_bytes(&mut out, value);
                        }
                        BatchOp::Delete { key } => {
                            out.push(2);
                            encode_bytes(&mut out, key);
                        }
                    }
                }
            }
        }
        out
    }

    /// Decode a record from bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::Corruption("empty wal record".into()));
        }
        let mut cursor = bytes;
        let ty = cursor.get_u8();
        match ty {
            1 => {
                let key = decode_bytes(&mut cursor)?;
                let value = decode_bytes(&mut cursor)?;
                Ok(WalRecord::Put { key, value })
            }
            2 => {
                let key = decode_bytes(&mut cursor)?;
                Ok(WalRecord::Delete { key })
            }
            3 => {
                let (count, n) = decode_uvarint(cursor)
                    .map_err(|e| Error::Corruption(format!("batch count: {e}")))?;
                cursor.advance(n);
                let mut ops = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    if cursor.is_empty() {
                        return Err(Error::Corruption("truncated batch op header".into()));
                    }
                    let op_ty = cursor.get_u8();
                    let op = match op_ty {
                        1 => {
                            let key = decode_bytes(&mut cursor)?;
                            let value = decode_bytes(&mut cursor)?;
                            BatchOp::Put { key, value }
                        }
                        2 => {
                            let key = decode_bytes(&mut cursor)?;
                            BatchOp::Delete { key }
                        }
                        other => {
                            return Err(Error::Corruption(format!(
                                "unknown batch op type {other}"
                            )));
                        }
                    };
                    ops.push(op);
                }
                Ok(WalRecord::Batch(ops))
            }
            other => Err(Error::Corruption(format!(
                "unknown wal record type {other}"
            ))),
        }
    }

    /// Decode a record from a `storage_wal::Record` payload.
    pub fn from_wal(record: &storage_wal::Record) -> Result<Self> {
        Self::decode(&record.payload)
    }

    /// Convert this record into a `storage_wal::Record` payload.
    pub fn into_wal(self) -> storage_wal::Record {
        storage_wal::Record::new(storage_wal::RecordType::Put, Bytes::from(self.encode()))
    }
}

fn encode_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    write_uvarint(out, bytes.len() as u64).unwrap();
    out.extend_from_slice(bytes);
}

fn decode_bytes(cursor: &mut &[u8]) -> Result<Vec<u8>> {
    let (len, n) = decode_uvarint(cursor)
        .map_err(|e| Error::Corruption(format!("length-prefixed bytes: {e}")))?;
    cursor.advance(n);
    if cursor.len() < len as usize {
        return Err(Error::Corruption("truncated length-prefixed bytes".into()));
    }
    let bytes = cursor[..len as usize].to_vec();
    cursor.advance(len as usize);
    Ok(bytes)
}

/// Standard file names inside an `ArtEngine` directory.
pub const META_FILE: &str = "art.meta";
/// Name of the snapshot file inside an `ArtEngine` directory.
pub const SNAPSHOT_FILE: &str = "snapshot.bin";
/// Name of the WAL subdirectory inside an `ArtEngine` directory.
pub const WAL_DIR: &str = "wal";

/// Return the path to the metadata file.
pub fn meta_path(dir: impl AsRef<Path>) -> PathBuf {
    dir.as_ref().join(META_FILE)
}

/// Return the path to the snapshot file.
pub fn snapshot_path(dir: impl AsRef<Path>) -> PathBuf {
    dir.as_ref().join(SNAPSHOT_FILE)
}

/// Return the path to the WAL directory.
pub fn wal_dir(dir: impl AsRef<Path>) -> PathBuf {
    dir.as_ref().join(WAL_DIR)
}

/// Atomically write `bytes` to `path` by writing to a temp file, fsyncing,
/// renaming over the target, and fsyncing the parent directory.
pub fn write_atomic(path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        storage_file::sync_dir(parent)?;
    }
    Ok(())
}

/// Compute the CRC32C of the file at `path`, if it exists.
pub fn file_crc(path: impl AsRef<Path>) -> Result<u32> {
    let bytes = std::fs::read(path)?;
    Ok(crc32c(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let meta = Metadata {
            options: ArtEngineOptions::default(),
            last_snapshot_lsn: 42,
            snapshot_crc: 0xdead_beef,
        };
        let bytes = meta.encode();
        let decoded = Metadata::decode(&bytes).unwrap();
        assert_eq!(decoded, meta);
    }

    #[test]
    fn metadata_corruption_detected() {
        let meta = Metadata::new(ArtEngineOptions::default());
        let mut bytes = meta.encode();
        bytes[10] ^= 0xff;
        assert!(Metadata::decode(&bytes).is_err());
    }

    #[test]
    fn wal_record_roundtrip() {
        let records = vec![
            WalRecord::Put {
                key: b"hello".to_vec(),
                value: b"world".to_vec(),
            },
            WalRecord::Delete { key: b"x".to_vec() },
            WalRecord::Batch(vec![
                BatchOp::Put {
                    key: b"a".to_vec(),
                    value: b"1".to_vec(),
                },
                BatchOp::Delete { key: b"b".to_vec() },
            ]),
        ];
        for rec in records {
            let bytes = rec.clone().encode();
            let decoded = WalRecord::decode(&bytes).unwrap();
            assert_eq!(decoded, rec);
        }
    }

    #[test]
    fn wal_record_rejects_unknown_type() {
        assert!(WalRecord::decode(&[99]).is_err());
    }
}
