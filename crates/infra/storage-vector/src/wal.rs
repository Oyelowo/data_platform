//! Vector-engine WAL wrapper around `storage-wal`.

use std::path::Path;

use storage_wal::{Durability, Wal as RawWal, WalOptions};

use crate::format::{WalRecord, WAL_DIR};

/// Vector-engine write-ahead log.
pub struct VectorWal {
    wal: RawWal,
}

impl VectorWal {
    /// Open the WAL in `dir`.
    pub fn open(dir: impl AsRef<Path>) -> crate::Result<Self> {
        let wal_dir = dir.as_ref().join(WAL_DIR);
        std::fs::create_dir_all(&wal_dir)?;
        let wal = RawWal::open(&wal_dir, WalOptions::default())?;
        Ok(Self { wal })
    }

    /// Append a [`WalRecord`] with immediate durability.
    pub fn append(&self, record: WalRecord) -> crate::Result<u64> {
        let payload = record.encode()?;
        let lsn = self.wal.append(&payload, Durability::Immediate)?;
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

    /// Flush all buffered records to stable storage.
    pub fn sync(&self) -> crate::Result<()> {
        self.wal.sync()?;
        Ok(())
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
