//! WAL integration for the geospatial engine.

use std::path::Path;

use storage_wal::{Wal as RawWal, WalOptions};

use crate::format::{WalRecord, WAL_DIR};
use crate::options::WalSyncPolicy;

/// Geospatial engine write-ahead log.
pub struct GeoWal {
    wal: RawWal,
    sync_policy: WalSyncPolicy,
}

impl GeoWal {
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

    /// Return an iterator over all WAL records with their LSNs.
    ///
    /// Checkpoint and blank records are skipped; only application data records
    /// are yielded.
    pub fn iter(&self) -> crate::Result<impl Iterator<Item = crate::Result<(u64, WalRecord)>> + '_> {
        let iter = self.wal.iter(0)?;
        Ok(iter.filter_map(|res| match res {
            Ok(rec) => {
                if rec.ty != storage_wal::RecordType::Put {
                    return None;
                }
                let lsn = rec.lsn;
                Some(WalRecord::decode(&rec.payload).map(|r| (lsn, r)))
            }
            Err(e) => Some(Err(crate::Error::Wal(e))),
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
