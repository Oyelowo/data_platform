//! Atomic file-write helpers.

use std::fs::{OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::Path;

use crate::sync::sync_dir;

/// Write `data` to a temporary file next to `dest`, fsync it, rename it over
/// `dest`, then fsync the parent directory.
///
/// This is the canonical atomic-update pattern used for metadata files. The
/// destination is either updated completely or not at all.
pub fn atomic_write(dest: &Path, data: impl AsRef<[u8]>) -> io::Result<()> {
    atomic_write_with_permissions(dest, data, None)
}

/// Like [`atomic_write`], but also sets the permissions of the temporary file
/// before renaming it.
pub fn atomic_write_with_permissions(
    dest: &Path,
    data: impl AsRef<[u8]>,
    permissions: Option<Permissions>,
) -> io::Result<()> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dest.with_extension("tmp");

    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(data.as_ref())?;
        file.sync_all()?;
        if let Some(perms) = permissions {
            file.set_permissions(perms)?;
        }
    }

    std::fs::rename(&tmp, dest)?;
    sync_dir(parent)?;
    Ok(())
}
