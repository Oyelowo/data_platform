//! Append-only writer for the feature store.

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// Sequential writer for a single feature store file.
pub struct FeatureWriter {
    file: File,
}

impl FeatureWriter {
    /// Open or create the store file at `path`.
    pub fn open(path: &Path) -> crate::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(false)
            .open(path)?;
        Ok(Self { file })
    }

    /// Append `data` to the file and return the byte offset where it starts.
    pub fn append(&mut self, data: &[u8]) -> crate::Result<u64> {
        let offset = self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(data)?;
        Ok(offset)
    }

    /// Flush the file to stable storage.
    pub fn sync(&mut self) -> crate::Result<()> {
        self.file.sync_all()?;
        Ok(())
    }
}
