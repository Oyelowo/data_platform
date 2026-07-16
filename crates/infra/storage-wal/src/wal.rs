//! Public WAL API.

use std::path::{Path, PathBuf};

use crate::committer::Committer;
use crate::record::{Durability, Record, RecordType};
use crate::reader::{WalIterator, WalReader};
use crate::segment::{list_segments, segment_path};
use crate::{Error, Lsn, Result};

/// Configuration for a WAL.
#[derive(Debug, Clone, Copy)]
pub struct WalOptions {
    /// Maximum size of a single segment file in bytes.
    pub segment_size: u64,
    /// Default durability for `append` calls.
    pub durability: Durability,
}

impl WalOptions {
    /// Default segment size: 64 MiB.
    pub const DEFAULT_SEGMENT_SIZE: u64 = 64 * 1024 * 1024;
}

impl Default for WalOptions {
    fn default() -> Self {
        Self {
            segment_size: Self::DEFAULT_SEGMENT_SIZE,
            durability: Durability::Immediate,
        }
    }
}

/// Append-only write-ahead log.
pub struct Wal {
    dir: PathBuf,
    options: WalOptions,
    committer: Committer,
}

impl std::fmt::Debug for Wal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wal")
            .field("dir", &self.dir)
            .field("options", &self.options)
            .field("committer", &self.committer)
            .finish()
    }
}

impl Wal {
    /// Open or create a WAL at `dir`.
    pub fn open(dir: impl AsRef<Path>, options: WalOptions) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let committer = Committer::start(dir.clone(), options.segment_size)?;
        Ok(Self {
            dir,
            options,
            committer,
        })
    }

    /// Append `payload` with the default record type `Put` and the configured
    /// durability. Returns the assigned LSN.
    pub fn append(&self, payload: impl AsRef<[u8]>, durability: Durability) -> Result<Lsn> {
        self.append_record(Record::new(RecordType::Put, bytes::Bytes::copy_from_slice(payload.as_ref())), durability)
    }

    /// Append an arbitrary record.
    pub fn append_record(&self, record: Record, durability: Durability) -> Result<Lsn> {
        match durability {
            Durability::Buffered => {
                // Buffered durability is not implemented; immediate is required
                // for correctness. Returning an error avoids silently lying.
                Err(Error::InvalidArgument(
                    "buffered durability is not supported".into(),
                ))
            }
            Durability::Immediate => self.committer.append(record),
        }
    }

    /// Append a checkpoint record. This is durable and may be used to truncate
    /// older segments.
    pub fn checkpoint(&self, payload: impl AsRef<[u8]>) -> Result<Lsn> {
        self.append_record(Record::new(RecordType::Checkpoint, bytes::Bytes::copy_from_slice(payload.as_ref())), Durability::Immediate)
    }

    /// Return a random-access reader for this WAL.
    pub fn reader(&self) -> WalReader {
        WalReader::new(&self.dir)
    }

    /// Return an iterator over all records starting from `start_lsn`.
    pub fn iter(&self, start_lsn: Lsn) -> Result<WalIterator> {
        WalIterator::new(&self.dir, start_lsn)
    }

    /// Truncate all segments whose first LSN is strictly less than `before_lsn`.
    /// Returns the number of segments removed.
    pub fn truncate_before(&self, before_lsn: Lsn) -> Result<usize> {
        let segments = list_segments(&self.dir)?;
        let mut removed = 0;
        for first_lsn in segments {
            if first_lsn + self.options.segment_size <= before_lsn {
                std::fs::remove_file(segment_path(&self.dir, first_lsn))?;
                removed += 1;
            } else {
                break;
            }
        }
        Ok(removed)
    }

    /// Gracefully close the WAL, waiting for the commit worker to finish.
    ///
    /// Idempotent: safe to call from a shared reference and safe to call more
    /// than once.
    pub fn close(&self) -> Result<()> {
        self.committer.shutdown()
    }

    /// Directory containing the segment files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Wal {
    fn drop(&mut self) {
        // Ensure the fsync worker is joined even when the caller does not call
        // `close` explicitly. This is required when `Wal` is held inside an
        // `Arc` and dropped from the last reference.
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
        let lsn = wal.append(&b"hello"[..], Durability::Immediate).unwrap();
        let rec = wal.reader().read(lsn).unwrap().unwrap();
        assert_eq!(rec.payload, &b"hello"[..]);
        wal.close().unwrap();
    }

    #[test]
    fn append_many_and_iterate() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
        let lsns: Vec<_> = (0..10)
            .map(|i| wal.append(format!("record-{i}").into_bytes(), Durability::Immediate).unwrap())
            .collect();

        for (i, lsn) in lsns.iter().enumerate() {
            let rec = wal.reader().read(*lsn).unwrap().unwrap();
            assert_eq!(rec.payload, format!("record-{i}").into_bytes());
        }

        let values: Vec<_> = wal
            .iter(0)
            .unwrap()
            .map(|r| String::from_utf8(r.unwrap().payload.to_vec()).unwrap())
            .collect();
        assert_eq!(values.len(), 10);
        wal.close().unwrap();
    }

    #[test]
    fn checkpoint_record() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
        let lsn = wal.checkpoint(b"cp").unwrap();
        let rec = wal.reader().read(lsn).unwrap().unwrap();
        assert_eq!(rec.ty, RecordType::Checkpoint);
        wal.close().unwrap();
    }
}
