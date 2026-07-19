//! Small utility helpers used across `storage-blob`.

use std::path::Path;

/// fsync a directory so that newly created or removed entries are durable.
///
/// On Unix this opens the directory and calls `fsync`.  Non-Unix platforms do
/// not expose a portable directory fsync, so this is a best-effort no-op
/// there.
pub fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let file = std::fs::File::open(path)?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
