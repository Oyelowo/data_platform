//! Paged file I/O primitives.
//!
//! The layer below the buffer pool is a simple, page-addressed file.  All
//! reads and writes are whole pages at offsets `page_id * page_size`.  The
//! file grows automatically when a page beyond the current end is written.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::io::{Boundary, OpenOptions, RealBackend, StorageBackend, StorageFile};
use crate::page::PageId;

const MIN_PAGE_SIZE: usize = 512;

/// A page-addressed file.
pub struct PagedFile {
    path: PathBuf,
    page_size: usize,
    file: Box<dyn StorageFile>,
    backend: Arc<dyn StorageBackend>,
}

impl PagedFile {
    /// Open or create a paged file at `path` using the production backend.
    pub fn open(path: impl AsRef<Path>, page_size: usize) -> Result<Self> {
        Self::open_with_backend(path, page_size, Arc::new(RealBackend))
    }

    /// Open or create a paged file at `path` using the supplied backend.
    pub fn open_with_backend(
        path: impl AsRef<Path>,
        page_size: usize,
        backend: Arc<dyn StorageBackend>,
    ) -> Result<Self> {
        if page_size < MIN_PAGE_SIZE {
            return Err(Error::InvalidArgument(format!(
                "page size {page_size} is below minimum {MIN_PAGE_SIZE}"
            )));
        }
        if page_size.count_ones() != 1 {
            return Err(Error::InvalidArgument(
                "page size must be a power of two".into(),
            ));
        }
        let path = path.as_ref().to_path_buf();
        let file = backend.open(
            &path,
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )?;
        Ok(Self {
            path,
            page_size,
            file,
            backend,
        })
    }

    /// Number of whole pages currently stored in the file.
    pub fn page_count(&self) -> Result<u64> {
        let len = self.file.len()?;
        Ok(len / self.page_size as u64)
    }

    /// Read a whole page by id. Returns `Error::Corruption` if the file ends
    /// before the page is complete.
    pub fn read_page(&self, page_id: PageId) -> Result<Vec<u8>> {
        self.backend.pre_op(Boundary::PageRead(page_id))?;
        let offset = page_id.get() * self.page_size as u64;
        let mut buf = vec![0u8; self.page_size];
        match self.file.read_at(&mut buf, offset) {
            Ok(()) => {
                self.backend
                    .corrupt_read(Boundary::PageRead(page_id), &mut buf, offset)?;
                Ok(buf)
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => Err(Error::Corruption(format!(
                "page {page_id} read past end of file"
            ))),
            Err(e) => Err(e.into()),
        }
    }

    /// Write a whole page at `page_id`. The file is extended automatically.
    pub fn write_page(&self, page_id: PageId, data: &[u8]) -> Result<()> {
        if data.len() != self.page_size {
            return Err(Error::InvalidArgument(format!(
                "write_page expected {0} bytes, got {1}",
                self.page_size,
                data.len()
            )));
        }
        self.backend.pre_op(Boundary::PageWrite(page_id))?;
        let offset = page_id.get() * self.page_size as u64;
        let len = self
            .backend
            .truncate_write(Boundary::PageWrite(page_id), data.len())?;
        self.file.write_at(&data[..len], offset)?;
        // Partial writes may leave a torn page; that is the intended fault
        // behaviour and is detected by checksums on reopen.
        Ok(())
    }

    /// Ensure all writes are durably on disk, including the directory entry.
    ///
    /// Callers that implement the fsyncgate rule may choose to abort the
    /// process if this method returns an error; this layer propagates the
    /// error rather than panicking, because abort policy belongs to the
    /// engine.
    pub fn sync(&self) -> Result<()> {
        self.backend.pre_op(Boundary::PageSync(PageId::new(0)))?;
        self.file.sync()?;
        // Sync the parent directory so that file metadata (size, existence) is
        // durable.  This is required for crash-safe rename-free writes.
        self.backend
            .sync_dir(self.path.parent().unwrap_or_else(|| Path::new(".")))?;
        Ok(())
    }

    /// Truncate the file to `len` bytes.
    pub fn set_len(&self, len: u64) -> Result<()> {
        self.file.set_len(len)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_page() {
        let dir = tempfile::tempdir().unwrap();
        let file = PagedFile::open(dir.path().join("pages.dat"), 4096).unwrap();

        let mut page = vec![0u8; 4096];
        page[0..4].copy_from_slice(b"test");
        file.write_page(PageId::new(3), &page).unwrap();

        let read = file.read_page(PageId::new(3)).unwrap();
        assert_eq!(&read[0..4], b"test");
        assert_eq!(file.page_count().unwrap(), 4);
    }

    #[test]
    fn read_missing_page_is_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let file = PagedFile::open(dir.path().join("pages.dat"), 512).unwrap();
        assert!(file.read_page(PageId::new(5)).is_err());
    }

    #[test]
    fn wrong_page_size_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(PagedFile::open(dir.path().join("pages.dat"), 100).is_err());
        assert!(PagedFile::open(dir.path().join("pages.dat"), 3 * 1024).is_err());
    }
}
