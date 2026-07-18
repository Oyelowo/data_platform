//! Paged file I/O primitives.
//!
//! The layer below the buffer pool is a simple, page-addressed file.  All
//! reads and writes are whole pages at offsets `page_id * page_size`.  The
//! file grows automatically when a page beyond the current end is written.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::v2::page::PageId;

const MIN_PAGE_SIZE: usize = 512;

/// A page-addressed file.
pub struct PagedFile {
    path: PathBuf,
    page_size: usize,
    file: File,
}

impl PagedFile {
    /// Open or create a paged file at `path`.
    pub fn open(path: impl AsRef<Path>, page_size: usize) -> Result<Self> {
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
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        Ok(Self {
            path,
            page_size,
            file,
        })
    }

    /// Number of whole pages currently stored in the file.
    pub fn page_count(&self) -> Result<u64> {
        let len = self.file.metadata()?.len();
        Ok(len / self.page_size as u64)
    }

    /// Read a whole page by id. Returns `Error::Corruption` if the file ends
    /// before the page is complete.
    pub fn read_page(&self, page_id: PageId) -> Result<Vec<u8>> {
        let offset = page_id * self.page_size as u64;
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; self.page_size];
        match file.read_exact(&mut buf) {
            Ok(()) => Ok(buf),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Err(Error::Corruption(
                format!("page {page_id} read past end of file"),
            )),
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
        let offset = page_id * self.page_size as u64;
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        file.flush()?;
        Ok(())
    }

    /// Ensure all writes are durably on disk, including the directory entry.
    ///
    /// Callers that implement the fsyncgate rule may choose to abort the
    /// process if this method returns an error; this layer propagates the
    /// error rather than panicking, because abort policy belongs to the
    /// engine.
    pub fn sync(&self) -> Result<()> {
        self.file.sync_all()?;
        // Sync the parent directory so that file metadata (size, existence) is
        // durable.  This is required for crash-safe rename-free writes.
        let dir = File::open(self.path.parent().unwrap_or_else(|| Path::new(".")))?;
        dir.sync_all()?;
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
        file.write_page(3, &page).unwrap();

        let read = file.read_page(3).unwrap();
        assert_eq!(&read[0..4], b"test");
        assert_eq!(file.page_count().unwrap(), 4);
    }

    #[test]
    fn read_missing_page_is_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let file = PagedFile::open(dir.path().join("pages.dat"), 512).unwrap();
        assert!(file.read_page(5).is_err());
    }

    #[test]
    fn wrong_page_size_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(PagedFile::open(dir.path().join("pages.dat"), 100).is_err());
        assert!(PagedFile::open(dir.path().join("pages.dat"), 3 * 1024).is_err());
    }
}
