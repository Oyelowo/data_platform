//! Production storage backend delegating to `std::fs`.

use std::fs::File;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt;

use storage_file::sync_dir;

use crate::io::{OpenOptions, StorageBackend, StorageFile};

/// Production backend that delegates all operations to the operating system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RealBackend;

impl RealBackend {
    /// Create a new production backend.
    pub fn new() -> Self {
        Self
    }
}

impl StorageBackend for RealBackend {
    fn open(&self, path: &Path, opts: OpenOptions) -> IoResult<Box<dyn StorageFile>> {
        let mut std_opts = File::options();
        std_opts
            .read(opts.is_read())
            .write(opts.is_write())
            .create(opts.is_create())
            .truncate(opts.is_truncate());
        let file = std_opts.open(path)?;
        Ok(Box::new(RealFile {
            path: path.to_path_buf(),
            file,
        }))
    }

    fn rename(&self, from: &Path, to: &Path) -> IoResult<()> {
        std::fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> IoResult<()> {
        std::fs::remove_file(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn sync_dir(&self, path: &Path) -> IoResult<()> {
        sync_dir(path)
    }
}

/// A real file backed by `std::fs::File`.
pub struct RealFile {
    #[allow(dead_code)]
    path: PathBuf,
    file: File,
}

impl StorageFile for RealFile {
    #[cfg(unix)]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> IoResult<()> {
        let n = self.file.read_at(buf, offset)?;
        if n < buf.len() {
            return Err(IoError::new(
                ErrorKind::UnexpectedEof,
                format!(
                    "short read at {}: got {}, expected {}",
                    offset,
                    n,
                    buf.len()
                ),
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> IoResult<()> {
        let n = self.file.seek_read(buf, offset)?;
        if n < buf.len() {
            return Err(IoError::new(
                ErrorKind::UnexpectedEof,
                format!(
                    "short read at {}: got {}, expected {}",
                    offset,
                    n,
                    buf.len()
                ),
            ));
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn read_at(&self, buf: &mut [u8], offset: u64) -> IoResult<()> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)
    }

    #[cfg(unix)]
    fn write_at(&self, buf: &[u8], offset: u64) -> IoResult<()> {
        let n = self.file.write_at(buf, offset)?;
        if n < buf.len() {
            return Err(IoError::new(
                ErrorKind::WriteZero,
                format!(
                    "short write at {}: wrote {}, expected {}",
                    offset,
                    n,
                    buf.len()
                ),
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    fn write_at(&self, buf: &[u8], offset: u64) -> IoResult<()> {
        let n = self.file.seek_write(buf, offset)?;
        if n < buf.len() {
            return Err(IoError::new(
                ErrorKind::WriteZero,
                format!(
                    "short write at {}: wrote {}, expected {}",
                    offset,
                    n,
                    buf.len()
                ),
            ));
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn write_at(&self, buf: &[u8], offset: u64) -> IoResult<()> {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(buf)
    }

    fn sync(&self) -> IoResult<()> {
        self.file.sync_all()
    }

    fn set_len(&self, len: u64) -> IoResult<()> {
        self.file.set_len(len)
    }

    fn len(&self) -> IoResult<u64> {
        Ok(self.file.metadata()?.len())
    }
}


