//! Overflow value storage.
//!
//! Large values are stored as a chain of fixed-size blocks in an append-only
//! overflow file. This is a simplified substitute for a full log-structured
//! store in the first version.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::{Buf, BufMut, Bytes};
use parking_lot::Mutex;

use crate::error::{Error, Result};

const OVERFLOW_FILE: &str = "overflow.dat";
const OVERFLOW_MAGIC: u32 = 0x4F_56_46_4C; // "OVFL"
const OVERFLOW_BLOCK_SIZE: usize = 64 * 1024;
const NULL_OFFSET: u64 = u64::MAX;

/// Append-only overflow value store.
pub(crate) struct OverflowStore {
    path: PathBuf,
    file: Mutex<File>,
    next_offset: AtomicU64,
}

impl OverflowStore {
    /// Open or create the overflow store in `dir`.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(OVERFLOW_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            path,
            file: Mutex::new(file),
            next_offset: AtomicU64::new(len),
        })
    }

    /// Write a value and return the head block offset.
    pub fn write(&self, value: &[u8]) -> Result<u64> {
        let data_capacity = OVERFLOW_BLOCK_SIZE - 16;
        let num_blocks = value.len().div_ceil(data_capacity).max(1);
        let offsets: Vec<u64> = (0..num_blocks)
            .map(|_| self.next_offset.fetch_add(OVERFLOW_BLOCK_SIZE as u64, Ordering::SeqCst))
            .collect();

        let mut file = self.file.lock();
        for (i, offset) in offsets.iter().copied().enumerate() {
            let start = i * data_capacity;
            let end = ((i + 1) * data_capacity).min(value.len());
            let next = if i + 1 < offsets.len() {
                offsets[i + 1]
            } else {
                NULL_OFFSET
            };
            Self::write_block(&mut file, offset, next, &value[start..end])?;
        }
        file.flush()?;
        Ok(offsets[0])
    }

    /// Read a value from its head block offset.
    pub fn read(&self, head: u64) -> Result<Bytes> {
        let mut out = Vec::new();
        let mut current = head;
        let mut visited = std::collections::HashSet::new();
        let mut file = self.file.lock();
        while current != NULL_OFFSET {
            if !visited.insert(current) {
                return Err(Error::Corruption("overflow cycle detected".into()));
            }
            let (next, data) = Self::read_block(&mut file, current)?;
            out.extend_from_slice(&data);
            current = next;
        }
        Ok(Bytes::from(out))
    }

    /// Ensure all writes are durably persisted.
    pub fn sync(&self) -> Result<()> {
        let file = self.file.lock();
        file.sync_all()?;
        drop(file);
        if let Ok(dir) = File::open(&self.path.parent().unwrap_or(Path::new(""))) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    fn write_block(file: &mut File, offset: u64, next: u64, data: &[u8]) -> Result<()> {
        let mut buf = Vec::with_capacity(OVERFLOW_BLOCK_SIZE);
        buf.put_u32_le(OVERFLOW_MAGIC);
        buf.put_u64_le(next);
        buf.put_u32_le(data.len() as u32);
        buf.extend_from_slice(data);
        buf.resize(OVERFLOW_BLOCK_SIZE, 0);
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&buf)?;
        Ok(())
    }

    fn read_block(file: &mut File, offset: u64) -> Result<(u64, Bytes)> {
        let mut buf = vec![0u8; OVERFLOW_BLOCK_SIZE];
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut buf)?;
        let mut cursor = &buf[..];
        let magic = cursor.get_u32_le();
        if magic != OVERFLOW_MAGIC {
            return Err(Error::Corruption(format!(
                "overflow block magic mismatch at offset {offset}: expected {OVERFLOW_MAGIC:#x}, got {magic:#x}"
            )));
        }
        let next = cursor.get_u64_le();
        let data_len = cursor.get_u32_le() as usize;
        let data = Bytes::copy_from_slice(&buf[16..16 + data_len]);
        Ok((next, data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_value_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = OverflowStore::open(dir.path()).unwrap();
        let head = store.write(&[]).unwrap();
        assert_eq!(store.read(head).unwrap().as_ref(), &[]);
    }

    #[test]
    fn small_value_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = OverflowStore::open(dir.path()).unwrap();
        let value = b"hello world";
        let head = store.write(value).unwrap();
        assert_eq!(store.read(head).unwrap().as_ref(), value.as_slice());
    }

    #[test]
    fn large_value_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = OverflowStore::open(dir.path()).unwrap();
        let value = vec![0xABu8; 256 * 1024];
        let head = store.write(&value).unwrap();
        assert_eq!(store.read(head).unwrap().as_ref(), value.as_slice());
    }
}
