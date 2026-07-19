//! Directory and file fsync helpers.

use std::fs::File;
use std::io;
use std::path::Path;

/// Open a directory for the purpose of fsyncing it.
///
/// On Unix this opens the directory with `O_RDONLY`; on Windows it returns
/// `None` because directory fsync semantics differ.
pub fn open_dir_for_sync(path: &Path) -> io::Result<Option<File>> {
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

/// Fsync a directory so that file creation/deletion/rename operations within
/// it are durable.
///
/// This is a no-op on platforms where directory fsync is not supported.
pub fn sync_dir(path: &Path) -> io::Result<()> {
    if let Some(dir) = open_dir_for_sync(path)? {
        dir.sync_all()?;
    }
    Ok(())
}

/// Fsync a single file.
pub fn sync_file(file: &File) -> io::Result<()> {
    file.sync_all()?;
    Ok(())
}
