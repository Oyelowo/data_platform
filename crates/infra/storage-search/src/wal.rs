//! WAL integration for the search engine.

use std::path::Path;

use storage_wal::{Wal as RawWal, WalOptions};

use crate::format::{WalRecord, WAL_DIR};
use crate::options::WalSyncPolicy;

/// Search engine write-ahead log.
pub struct SearchWal {
    wal: RawWal,
    sync_policy: WalSyncPolicy,
}

impl SearchWal {
    /// Open the WAL in `dir` with the given sync policy.
    pub fn open(dir: impl AsRef<Path>, sync_policy: WalSyncPolicy) -> crate::Result<Self> {
        let wal_dir = dir.as_ref().join(WAL_DIR);
        std::fs::create_dir_all(&wal_dir)?;
        let wal = RawWal::open(&wal_dir, WalOptions::default())?;
        Ok(Self { wal, sync_policy })
    }

    /// Append a [`WalRecord`] using the configured sync policy.
    pub fn append(&self, record: WalRecord) -> crate::Result<u64> {
        let payload = record.encode()?;
        let lsn = self.wal.append(&payload, self.sync_policy.durability())?;
        Ok(lsn)
    }

    /// Return an iterator over all WAL records.
    pub fn iter(&self) -> crate::Result<impl Iterator<Item = crate::Result<WalRecord>> + '_> {
        let iter = self.wal.iter(0)?;
        Ok(iter.map(|res| match res {
            Ok(rec) => WalRecord::decode(&rec.payload),
            Err(e) => Err(crate::Error::Io(std::io::Error::other(e.to_string()))),
        }))
    }

    /// Force all buffered records to stable storage.
    pub fn sync(&self) -> crate::Result<()> {
        self.wal.sync()?;
        Ok(())
    }

    /// Append a checkpoint record carrying the current metadata.
    pub fn checkpoint(&self, metadata: &crate::format::Metadata) -> crate::Result<u64> {
        let payload = metadata.encode()?;
        let lsn = self.wal.checkpoint(&payload)?;
        Ok(lsn)
    }

    /// Truncate completed WAL segments.
    pub fn truncate_completed(&self) -> crate::Result<()> {
        self.wal.truncate_completed()?;
        Ok(())
    }

    /// Close the WAL gracefully.
    pub fn close(&self) -> crate::Result<()> {
        self.wal.close()?;
        Ok(())
    }
}
