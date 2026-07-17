//! Recovery logic for the Bw-Tree engine.

use std::path::{Path, PathBuf};

use bytes::{Buf, BufMut};
use crc32c::crc32c;

use crate::error::{Error, Result};
use crate::page::Pid;

/// Name of the metadata file.
const META_FILE: &str = "META";

/// Magic number at the start of the metadata file.
const META_MAGIC: u32 = 0x42_57_54_52; // "BWTR"

/// Current metadata format version.
const META_VERSION: u16 = 1;

/// Persistent engine metadata.
#[derive(Clone, Debug)]
pub(crate) struct Meta {
    /// Root PID of the Bw-Tree.
    pub root_pid: Pid,
    /// Next PID to allocate.
    pub next_pid: Pid,
    /// WAL LSN at the time of the checkpoint.
    pub wal_lsn: u64,
}

impl Meta {
    /// Serialize the metadata to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.put_u32_le(META_MAGIC);
        buf.put_u16_le(META_VERSION);
        buf.put_u64_le(self.root_pid);
        buf.put_u64_le(self.next_pid);
        buf.put_u64_le(self.wal_lsn);
        buf.extend_from_slice(&[0u8; 16]); // reserved
        let checksum = crc32c(&buf);
        buf.put_u32_le(checksum);
        buf
    }

    /// Decode metadata from bytes.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 4 + 2 + 8 + 8 + 8 + 16 + 4 {
            return Err(Error::Corruption("meta file truncated".into()));
        }
        let mut view = data;
        let magic = view.get_u32_le();
        if magic != META_MAGIC {
            return Err(Error::Corruption("meta file magic mismatch".into()));
        }
        let version = view.get_u16_le();
        if version != META_VERSION {
            return Err(Error::Corruption(format!(
                "unsupported meta version {version}"
            )));
        }
        let (body, checksum_bytes) = data.split_at(data.len() - 4);
        let expected = crc32c(body);
        let mut cv = checksum_bytes;
        let got = cv.get_u32_le();
        if expected != got {
            return Err(Error::Corruption(format!(
                "meta checksum mismatch: expected {expected:#x}, got {got:#x}"
            )));
        }

        let mut view = &body[4 + 2..];
        let root_pid = view.get_u64_le();
        let next_pid = view.get_u64_le();
        let wal_lsn = view.get_u64_le();
        Ok(Self {
            root_pid,
            next_pid,
            wal_lsn,
        })
    }
}

pub(crate) fn meta_path(dir: &Path) -> PathBuf {
    dir.join(META_FILE)
}

/// Read the metadata file if it exists.
pub(crate) fn read_meta(dir: &Path) -> Result<Option<Meta>> {
    let path = meta_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(&path)?;
    Ok(Some(Meta::decode(&data)?))
}

/// Atomically write the metadata file.
pub(crate) fn write_meta(dir: &Path, meta: &Meta) -> Result<()> {
    let path = meta_path(dir);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, meta.encode())?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_roundtrip() {
        let meta = Meta {
            root_pid: 7,
            next_pid: 42,
            wal_lsn: 123,
        };
        let encoded = meta.encode();
        let decoded = Meta::decode(&encoded).unwrap();
        assert_eq!(meta.root_pid, decoded.root_pid);
        assert_eq!(meta.next_pid, decoded.next_pid);
        assert_eq!(meta.wal_lsn, decoded.wal_lsn);
    }

    #[test]
    fn meta_checksum_failure() {
        let meta = Meta {
            root_pid: 1,
            next_pid: 2,
            wal_lsn: 3,
        };
        let mut encoded = meta.encode();
        encoded[10] ^= 0xFF;
        assert!(Meta::decode(&encoded).is_err());
    }
}
