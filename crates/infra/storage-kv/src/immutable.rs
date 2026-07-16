//! Placeholder for immutable MemTable background flush; currently flush is
//! synchronous. This module only exports SSTable path helpers.

use std::path::{Path, PathBuf};

use crate::FileNumber;

/// Path for an SSTable file.
pub fn sstable_path(db_path: &Path, number: FileNumber) -> PathBuf {
    db_path.join(format!("{:06}.sst", number))
}
