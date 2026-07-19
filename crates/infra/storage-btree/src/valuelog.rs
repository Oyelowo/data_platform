//! Append-only value log for large B+ tree values.
//!
//! Values larger than the configured inline threshold are written to a
//! separate file and only `(offset, len)` references are stored in leaf
//! cells.  The log is append-only; dead values are reclaimed by an explicit
//! stop-the-world GC pass.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex as ParkingMutex;
use storage_format::{read_u32_le, write_u32_le};

use crate::error::{Error, Result};
use crate::io::{Boundary, OpenOptions, RealBackend, StorageBackend, StorageFile};
use crate::sync::Mutex as SyncMutex;

pub type ValueOffset = u64;
pub type ValueLen = u32;

/// Durability mode for value-log appends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Durability {
    /// fsync after every append.
    #[default]
    Immediate,
    /// Rely on explicit `sync`/`close` for durability.
    Buffered,
}

/// A simple append-only value log file.
pub struct ValueLog {
    path: PathBuf,
    file: SyncMutex<Option<Box<dyn StorageFile>>>,
    next_offset: SyncMutex<ValueOffset>,
    /// In-memory reference counts: (offset, len) -> count.
    ref_counts: SyncMutex<HashMap<(ValueOffset, ValueLen), usize>>,
    durability: Durability,
    backend: Arc<dyn StorageBackend>,
    metrics: ParkingMutex<Option<std::sync::Arc<crate::metrics::Metrics>>>,
}

impl ValueLog {
    /// Open or create `<dir>/values.log` with immediate durability.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_durability(dir, Durability::Immediate)
    }

    /// Open or create `<dir>/values.log` with the requested durability.
    pub fn open_with_durability(dir: impl AsRef<Path>, durability: Durability) -> Result<Self> {
        Self::open_with_backend(dir, durability, Arc::new(RealBackend))
    }

    /// Open or create `<dir>/values.log` with the requested durability and
    /// backend.
    pub fn open_with_backend(
        dir: impl AsRef<Path>,
        durability: Durability,
        backend: Arc<dyn StorageBackend>,
    ) -> Result<Self> {
        let path = dir.as_ref().join("values.log");
        let file = backend.open(
            &path,
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )?;
        let end = file.len().map_err(Error::Io)?;
        Ok(Self {
            path,
            file: SyncMutex::new(Some(file)),
            next_offset: SyncMutex::new(end),
            ref_counts: SyncMutex::new(HashMap::new()),
            durability,
            backend,
            metrics: ParkingMutex::new(None),
        })
    }

    /// Attach a metrics collector to this value log.
    pub fn set_metrics(&self, metrics: std::sync::Arc<crate::metrics::Metrics>) {
        *self.metrics.lock() = Some(metrics);
    }

    fn with_file<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Box<dyn StorageFile>) -> Result<T>,
    {
        self.file.with_mut(|guard| {
            let file = guard
                .as_mut()
                .ok_or(Error::Unsupported("value log is closed"))?;
            f(file)
        })
    }

    /// Append `value` to the file and return its `(offset, len)` reference.
    ///
    /// The on-disk format is `[len: u32 LE][value bytes]`.  The returned
    /// `offset` points to the start of the record (the length prefix).
    pub fn append(&self, value: &[u8]) -> Result<(ValueOffset, ValueLen)> {
        if value.len() > u32::MAX as usize {
            return Err(Error::OutOfBounds {
                kind: crate::error::BoundKind::Value,
                limit: u32::MAX as usize,
                got: value.len(),
            });
        }
        let len = value.len() as ValueLen;
        let offset = self.next_offset.with_mut(|offset_guard| {
            let offset = *offset_guard;
            let record_len = 4 + value.len() as u64;
            *offset_guard = offset + record_len;
            offset
        });

        self.backend.pre_op(Boundary::ValueLogAppend)?;
        if self.backend.drop_append(Boundary::ValueLogAppend) {
            // Simulate a lost buffered append: the caller believes the append
            // succeeded, but the bytes never reached stable storage.
            return Ok((offset, len));
        }

        let mut record = Vec::with_capacity(4 + value.len());
        record.extend_from_slice(&len.to_le_bytes());
        record.extend_from_slice(value);
        let write_len = self
            .backend
            .truncate_write(Boundary::ValueLogAppend, record.len())?;

        self.with_file(|file| {
            file.write_at(&record[..write_len], offset)
                .map_err(Error::Io)?;
            if self.durability == Durability::Immediate {
                self.backend.pre_op(Boundary::ValueLogSync)?;
                file.sync().map_err(Error::Io)?;
                if let Some(m) = self.metrics.lock().as_ref() {
                    m.inc_value_log_syncs();
                }
            }
            Ok(())
        })?;

        if let Some(m) = self.metrics.lock().as_ref() {
            m.inc_value_log_bytes(write_len as u64);
        }

        Ok((offset, len))
    }

    /// Read the value stored at `(offset, len)`.
    pub fn read(&self, offset: ValueOffset, len: ValueLen) -> Result<Vec<u8>> {
        self.backend.pre_op(Boundary::ValueLogRead)?;
        self.with_file(|file| {
            let mut len_buf = [0u8; 4];
            file.read_at(&mut len_buf, offset).map_err(Error::Io)?;
            self.backend
                .corrupt_read(Boundary::ValueLogRead, &mut len_buf, offset)?;
            let stored_len = read_u32_le(&len_buf);
            if stored_len != len {
                return Err(Error::Corruption(format!(
                    "value-log length mismatch at offset {offset}: expected {len}, got {stored_len}"
                )));
            }
            let mut value = vec![0u8; len as usize];
            file.read_at(&mut value, offset + 4).map_err(Error::Io)?;
            self.backend
                .corrupt_read(Boundary::ValueLogRead, &mut value, offset + 4)?;
            Ok(value)
        })
    }

    /// fsync the log file.
    pub fn sync(&self) -> Result<()> {
        self.backend.pre_op(Boundary::ValueLogSync)?;
        self.with_file(|file| file.sync().map_err(Error::Io))?;
        if let Some(m) = self.metrics.lock().as_ref() {
            m.inc_value_log_syncs();
        }
        Ok(())
    }

    /// Sync and close the log file.
    pub fn close(&self) -> Result<()> {
        self.file.with_mut(|guard| {
            if let Some(file) = guard.take() {
                file.sync().map_err(Error::Io)?;
                // file is dropped on take
            }
            Ok(())
        })
    }

    /// Increment the reference count for a value-log entry.
    pub fn add_ref(&self, offset: ValueOffset, len: ValueLen) {
        self.ref_counts.with_mut(|counts| {
            *counts.entry((offset, len)).or_insert(0) += 1;
        });
    }

    /// Decrement the reference count; remove the entry when it reaches zero.
    pub fn release(&self, offset: ValueOffset, len: ValueLen) {
        self.ref_counts.with_mut(|counts| {
            let entry = counts.entry((offset, len)).or_insert(0);
            if *entry > 0 {
                *entry -= 1;
                if *entry == 0 {
                    counts.remove(&(offset, len));
                }
            }
        });
    }

    /// Return all value-log references with a positive reference count.
    pub fn live_refs(&self) -> Vec<(ValueOffset, ValueLen)> {
        self.ref_counts.with_mut(|counts| {
            counts
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(k, _)| *k)
                .collect()
        })
    }

    /// Validate that `observed` reference counts match the value log and that
    /// every live reference is reachable.
    ///
    /// This is intended for integrity checks: it compares the counts derived
    /// from walking the B+ tree against the in-memory reference counters.  It
    /// returns `Error::Corruption` on any mismatch.
    pub fn validate_refs(&self, observed: &HashMap<(ValueOffset, ValueLen), usize>) -> Result<()> {
        self.ref_counts.with_mut(|counts| {
            for ((offset, len), observed_count) in observed {
                let stored = counts.get(&(*offset, *len)).copied().unwrap_or(0);
                if stored != *observed_count {
                    return Err(Error::Corruption(format!(
                        "value-log ref count mismatch for ({offset},{len}): \
                         stored {stored}, observed {observed_count}"
                    )));
                }
            }

            for ((offset, len), stored) in counts.iter().filter(|(_, c)| **c > 0) {
                if observed.get(&(*offset, *len)) != Some(stored) {
                    return Err(Error::Corruption(format!(
                        "value-log entry ({offset},{len}) with refcount {stored} \
                         is not reachable from the tree"
                    )));
                }
            }

            Ok(())
        })
    }

    /// Stop-the-world compaction: rewrite the log keeping only live values
    /// and update stored offsets.
    ///
    /// The returned vector maps each old `(offset, len)` to its new offset.
    /// Callers are responsible for updating leaf-page references.
    pub fn gc(&self) -> Result<Vec<((ValueOffset, ValueLen), ValueOffset)>> {
        let live = self.live_refs();
        if live.is_empty() {
            // Nothing is live; truncate the log.
            self.with_file(|file| {
                file.set_len(0).map_err(Error::Io)?;
                Ok(())
            })?;
            self.next_offset.with_mut(|offset_guard| {
                *offset_guard = 0;
            });
            self.ref_counts.with_mut(|counts| {
                counts.clear();
            });
            return Ok(Vec::new());
        }

        // Copy live values in deterministic offset order.
        let mut sorted: Vec<(ValueOffset, ValueLen)> = live;
        sorted.sort_by_key(|(off, _)| *off);

        let new_path = self.path.with_extension("log.new");
        {
            let new_file = self.backend.open(
                &new_path,
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true),
            )?;
            let mut write_offset: ValueOffset = 0;
            for (old_offset, len) in &sorted {
                let value = self.read(*old_offset, *len)?;
                let mut len_buf = [0u8; 4];
                write_u32_le(&mut len_buf, value.len() as u32);
                new_file
                    .write_at(&len_buf, write_offset)
                    .map_err(Error::Io)?;
                write_offset += 4;
                new_file.write_at(&value, write_offset).map_err(Error::Io)?;
                write_offset += value.len() as u64;
            }
            new_file.sync().map_err(Error::Io)?;
        }

        // Atomically install the compacted file.
        self.backend
            .rename(&new_path, &self.path)
            .map_err(Error::Io)?;

        // Reopen the file and rebuild offsets/counts.
        let new_file = self.backend.open(
            &self.path,
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(false)
                .truncate(false),
        )?;
        let new_len = new_file.len().map_err(Error::Io)?;
        self.file.with_mut(|file_guard| {
            *file_guard = Some(new_file);
        });
        self.next_offset.with_mut(|offset_guard| {
            *offset_guard = new_len;
        });

        // Compute new offsets while preserving counts.
        self.ref_counts.with_mut(|counts| {
            let mut new_offsets: HashMap<(ValueOffset, ValueLen), ValueOffset> = HashMap::new();
            let mut new_counts: HashMap<(ValueOffset, ValueLen), usize> = HashMap::new();
            let mut current_offset: ValueOffset = 0;
            for (old_offset, len) in sorted {
                let count = counts.get(&(old_offset, len)).copied().unwrap_or(1);
                new_offsets.insert((old_offset, len), current_offset);
                new_counts.insert((current_offset, len), count);
                current_offset += 4 + len as u64;
            }
            *counts = new_counts;
            Ok(new_offsets.into_iter().collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let log = ValueLog::open(dir.path()).unwrap();
        let value = vec![b'x'; 1024];
        let (offset, len) = log.append(&value).unwrap();
        assert_eq!(len, value.len() as u32);
        let read = log.read(offset, len).unwrap();
        assert_eq!(read, value);
    }

    #[test]
    fn ref_counts_track_liveness() {
        let dir = tempfile::tempdir().unwrap();
        let log = ValueLog::open(dir.path()).unwrap();
        let (offset, len) = log.append(b"hello").unwrap();
        log.add_ref(offset, len);
        assert_eq!(log.live_refs(), vec![(offset, len)]);
        log.release(offset, len);
        assert!(log.live_refs().is_empty());
    }

    #[test]
    fn gc_copies_live_values_and_updates_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let log = ValueLog::open(dir.path()).unwrap();
        let (off1, len1) = log.append(b"live").unwrap();
        let (off2, _len2) = log.append(b"dead").unwrap();
        log.add_ref(off1, len1);
        let mapping = log.gc().unwrap();
        assert_eq!(log.live_refs(), vec![(0, len1)]);
        assert_eq!(log.read(0, len1).unwrap(), b"live");
        // The dead record should have been discarded.
        assert!(!mapping.iter().any(|((off, _), _)| *off == off2));
        // The live record must be remapped to offset 0.
        assert!(
            mapping
                .iter()
                .any(|((off, len), new_off)| { *off == off1 && *len == len1 && *new_off == 0 })
        );
    }

    #[test]
    fn sync_makes_value_durable() {
        let dir = tempfile::tempdir().unwrap();
        let log = ValueLog::open_with_durability(dir.path(), Durability::Buffered).unwrap();
        let value = b"persist me";
        let (offset, len) = log.append(value).unwrap();
        log.sync().unwrap();
        drop(log);

        let log2 = ValueLog::open(dir.path()).unwrap();
        assert_eq!(log2.read(offset, len).unwrap(), value);
    }
}
