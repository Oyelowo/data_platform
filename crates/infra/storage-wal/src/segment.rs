//! Segment files and metadata.

use std::fs::{File, OpenOptions};
use std::io::{Read, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::{Error, Lsn, Result};

/// File extension for WAL segment files.
pub const SEGMENT_EXT: &str = "log";

/// A physical WAL segment file.
pub struct Segment {
    // Retained for diagnostics and future APIs.
    #[allow(dead_code)]
    path: PathBuf,
    file: File,
    #[allow(dead_code)]
    first_lsn: Lsn,
    written: u64,
    capacity: u64,
}

impl Segment {
    /// Create or open a segment file starting at `first_lsn`.
    pub fn open(dir: &Path, first_lsn: Lsn, capacity: u64) -> Result<Self> {
        let path = segment_path(dir, first_lsn);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            path,
            file,
            first_lsn,
            written,
            capacity,
        })
    }

    #[allow(dead_code)]
    pub fn first_lsn(&self) -> Lsn {
        self.first_lsn
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn remaining(&self) -> u64 {
        self.capacity.saturating_sub(self.written)
    }

    #[allow(dead_code)]
    pub fn is_full(&self) -> bool {
        self.remaining() == 0
    }

    /// Append raw bytes to this segment and return the LSN offset.
    pub fn append(&mut self, bytes: &[u8]) -> Result<u64> {
        if self.written + bytes.len() as u64 > self.capacity {
            return Err(Error::InvalidArgument(
                "record does not fit in segment".into(),
            ));
        }
        let offset = self.written;
        self.file.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(offset)
    }

    /// Flush the OS page cache for this segment.
    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        Ok(())
    }

    /// Persist the segment to durable storage.
    pub fn sync(&mut self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    /// Truncate the segment file to `new_len` bytes.
    pub fn truncate(&mut self, new_len: u64) -> Result<()> {
        self.file.set_len(new_len)?;
        self.written = new_len;
        Ok(())
    }

    /// Return a new file handle opened for reading this segment.
    #[allow(dead_code)]
    pub fn open_read(&self) -> Result<File> {
        Ok(File::open(&self.path)?)
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the full contents of this segment file into memory.
    pub fn read_all(&mut self) -> Result<Vec<u8>> {
        use std::io::Seek;
        let mut buf = Vec::new();
        self.file.seek(SeekFrom::Start(0))?;
        self.file.read_to_end(&mut buf)?;
        Ok(buf)
    }
}

/// Parse a segment filename of the form `wal-{lsn:020}.log`.
pub fn parse_segment_filename(name: &str) -> Option<Lsn> {
    let name = name.strip_prefix("wal-")?;
    let name = name.strip_suffix(SEGMENT_EXT)?;
    let name = name.strip_suffix(".")?;
    name.parse::<Lsn>().ok()
}

/// Build the canonical path for a segment.
pub fn segment_path(dir: &Path, first_lsn: Lsn) -> PathBuf {
    dir.join(format!("wal-{first_lsn:020}.{SEGMENT_EXT}"))
}

/// Collect segment first-LSNs from a directory, sorted ascending.
pub fn list_segments(dir: &Path) -> Result<Vec<Lsn>> {
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(lsn) = parse_segment_filename(&name) {
            ids.push(lsn);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

/// Read the full contents of a segment file into memory.
pub fn read_segment(dir: &Path, first_lsn: Lsn) -> Result<Vec<u8>> {
    let path = segment_path(dir, first_lsn);
    let mut file = File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_path_and_parse() {
        let dir = Path::new("/tmp/wal");
        let path = segment_path(dir, 123);
        assert_eq!(path, PathBuf::from("/tmp/wal/wal-00000000000000000123.log"));
        let name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(parse_segment_filename(&name), Some(123));
    }
}
