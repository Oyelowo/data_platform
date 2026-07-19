//! Public WAL API.

use std::fs::File;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::committer::Committer;
use storage_file::sync_dir;
use crate::reader::{WalIterator, WalReader};
use crate::record::{Durability, Record, RecordType, RECORD_HEADER_SIZE};
use crate::segment::{Segment, list_segments, segment_path};
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

    /// Minimum sensible segment size: must fit several records plus headers.
    pub const MIN_SEGMENT_SIZE: u64 = RECORD_HEADER_SIZE as u64 * 4;

    /// Validate options and return an error if they are unusable.
    pub fn validate(&self) -> Result<()> {
        if self.segment_size < Self::MIN_SEGMENT_SIZE {
            return Err(Error::InvalidArgument(format!(
                "segment_size {} is below minimum {}",
                self.segment_size,
                Self::MIN_SEGMENT_SIZE
            )));
        }
        Ok(())
    }
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
    /// Advisory lock file ensuring only one `Wal` instance owns the directory.
    /// Stored in a mutex so `close` can release the lock from a shared reference.
    lock: std::sync::Mutex<Option<File>>,
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
        Self::open_with_fault_config(dir, options, None)
    }

    /// Open or create a WAL at `dir` with an optional fault-injection config.
    ///
    /// The fault config is intended for deterministic testing of durability
    /// boundaries.
    ///
    /// # Locking
    ///
    /// This method acquires an advisory lock on `dir/wal.lock`. A second open
    /// on the same directory returns [`Error::Locked`].
    pub fn open_with_fault_config(
        dir: impl AsRef<Path>,
        options: WalOptions,
        fault_config: Option<crate::FaultConfig>,
    ) -> Result<Self> {
        options.validate()?;
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        sync_dir(&dir)?;

        let lock_path = dir.join("wal.lock");
        let lock_file = File::create(&lock_path)?;
        if lock_file.try_lock_exclusive().is_err() {
            return Err(Error::Locked);
        }

        let committer = match fault_config {
            Some(cfg) => {
                Committer::start_with_fault_config(dir.clone(), options.segment_size, cfg)?
            }
            None => Committer::start(dir.clone(), options.segment_size)?,
        };
        Ok(Self {
            dir,
            options,
            committer,
            lock: std::sync::Mutex::new(Some(lock_file)),
        })
    }

    /// Append `payload` with the default record type `Put` and the configured
    /// durability. Returns the assigned LSN.
    pub fn append(&self, payload: impl AsRef<[u8]>, durability: Durability) -> Result<Lsn> {
        self.append_record(
            Record::new(
                RecordType::Put,
                bytes::Bytes::copy_from_slice(payload.as_ref()),
            ),
            durability,
        )
    }

    /// Append an arbitrary record.
    pub fn append_record(&self, record: Record, durability: Durability) -> Result<Lsn> {
        match durability {
            Durability::Buffered => self.committer.append_buffered(record),
            Durability::Immediate => self.committer.append(record),
        }
    }

    /// Force a flush of all buffered records. Blocks until durable.
    pub fn sync(&self) -> Result<()> {
        self.committer.sync()
    }

    /// Append a checkpoint record. This is durable and may be used to truncate
    /// older segments.
    pub fn checkpoint(&self, payload: impl AsRef<[u8]>) -> Result<Lsn> {
        self.append_record(
            Record::new(
                RecordType::Checkpoint,
                bytes::Bytes::copy_from_slice(payload.as_ref()),
            ),
            Durability::Immediate,
        )
    }

    /// Return a random-access reader for this WAL.
    pub fn reader(&self) -> WalReader {
        WalReader::new(&self.dir)
    }

    /// Return an iterator over all records starting from `start_lsn`.
    pub fn iter(&self, start_lsn: Lsn) -> Result<WalIterator> {
        WalIterator::new(&self.dir, start_lsn)
    }

    /// Truncate all completed segments whose first LSN is strictly less than
    /// `before_lsn`. The active (last) segment is never removed, even if it is
    /// fully before `before_lsn`, because the group-commit worker may still be
    /// appending to it.
    pub fn truncate_before(&self, before_lsn: Lsn) -> Result<usize> {
        let segments = list_segments(&self.dir)?;
        if segments.is_empty() {
            return Ok(0);
        }
        let mut removed = 0;
        // The last segment is the active segment and must be preserved.
        for first_lsn in segments.iter().take(segments.len().saturating_sub(1)) {
            // A segment is "completed" if its entire byte range is before
            // `before_lsn`. We use a strict <= so that a record at exactly
            // `before_lsn` remains readable.
            if first_lsn + self.options.segment_size <= before_lsn {
                std::fs::remove_file(segment_path(&self.dir, *first_lsn))?;
                removed += 1;
            } else {
                break;
            }
        }
        if removed > 0 {
            sync_dir(&self.dir)?;
        }
        Ok(removed)
    }

    /// Truncate all WAL segments that are fully before the current active
    /// segment.
    ///
    /// The active (last) segment is preserved because the group-commit worker
    /// keeps it open for appends. Truncating it would cause subsequent writes to
    /// go to an unlinked inode and be lost on the next open. Callers that need
    /// to reclaim the active segment must first rotate or close the WAL.
    pub fn truncate_completed(&self) -> Result<usize> {
        let segments = list_segments(&self.dir)?;
        if segments.len() <= 1 {
            return Ok(0);
        }
        let active_first = segments[segments.len() - 1];
        self.truncate_before(active_first)
    }

    /// Simulate a power-loss crash by truncating the active WAL segment to the
    /// byte length that has actually been fsynced. Records that were written to
    /// the OS page cache but not yet durable are dropped.
    pub fn crash(&self) -> Result<()> {
        let last_synced_len = self.committer.crash()?;
        let segments = list_segments(&self.dir)?;
        let active_first = segments.last().copied().unwrap_or(0);
        let mut segment = Segment::open(&self.dir, active_first, self.options.segment_size)?;
        if segment.written() > last_synced_len {
            segment.truncate(last_synced_len)?;
        }
        Ok(())
    }

    /// Gracefully close the WAL, waiting for the commit worker to finish.
    ///
    /// Idempotent: safe to call from a shared reference and safe to call more
    /// than once. The advisory directory lock is released so the directory can
    /// be reopened by another `Wal` instance.
    pub fn close(&self) -> Result<()> {
        let result = self.committer.shutdown();
        // Release the advisory lock so another process/instance can open the WAL.
        let _ = self.lock.lock().unwrap().take();
        result
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
        // `Arc` and dropped from the last reference. The advisory lock file is
        // dropped as part of the struct drop, releasing the lock.
        let _ = self.committer.shutdown();
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
            .map(|i| {
                wal.append(format!("record-{i}").into_bytes(), Durability::Immediate)
                    .unwrap()
            })
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
