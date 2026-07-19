//! Filesystem helpers for durable metadata operations.

use std::fs::File;
use std::path::Path;

use crate::Result;

/// Open a directory for the purpose of fsyncing it.
///
/// On Unix this opens the directory with `O_RDONLY`; on Windows it is a no-op
/// because directory fsync semantics differ.
pub fn open_dir_for_sync(path: &Path) -> Result<Option<File>> {
    #[cfg(unix)]
    {
        Ok(Some(File::open(path)?))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(None)
    }
}

/// Fsync a directory so that file creation/deletion/rename operations within it
/// are durable.
///
/// This is a no-op on platforms where directory fsync is not supported.
pub fn sync_dir(path: &Path) -> Result<()> {
    if let Some(dir) = open_dir_for_sync(path)? {
        dir.sync_all()?;
    }
    Ok(())
}

/// Write `data` to a temporary file next to `dest`, fsync it, rename it over
/// `dest`, then fsync the parent directory.
///
/// This is the canonical atomic-update pattern used for metadata files.
#[allow(dead_code)]
pub fn atomic_write(dest: &Path, data: impl AsRef<[u8]>) -> Result<()> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    let file = File::open(&tmp)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, dest)?;
    sync_dir(parent)?;
    Ok(())
}
