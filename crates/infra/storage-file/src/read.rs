//! Positional read/write helpers.

use std::fs::File;
use std::io;

#[cfg(not(unix))]
use std::io::{Read, Seek, SeekFrom, Write};

/// Read exactly `buf.len()` bytes at `offset` from `file`.
///
/// Uses `read_exact_at` on Unix; falls back to seek + read on other platforms.
pub fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(buf, offset)?;
    }
    #[cfg(not(unix))]
    {
        let mut file = file.try_clone()?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)?;
    }
    Ok(())
}

/// Write all of `buf` at `offset` in `file`.
///
/// Uses `write_all_at` on Unix; falls back to seek + write on other platforms.
pub fn write_all_at(file: &mut File, offset: u64, buf: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(buf, offset)?;
    }
    #[cfg(not(unix))]
    {
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(buf)?;
    }
    Ok(())
}
