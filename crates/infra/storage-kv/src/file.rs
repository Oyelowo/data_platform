//! Positioned-read file abstraction for SSTable readers.
//!
//! `SSTableReader` used to hold an `Arc<Mutex<File>>` and seek+read under the
//! mutex, serializing all block reads of a table.  [`RandomAccessFile`]
//! instead exposes positioned reads (`pread` on Unix), which are thread-safe
//! by construction: the shared file offset never moves, so any number of
//! readers can read disjoint blocks of the same file concurrently.
//!
//! `fadvise` hints let the engine tell the OS about upcoming access patterns
//! (random point lookups vs sequential compaction scans) so the page-cache
//! heuristics match reality.  Hints are advisory: they never change results
//! and their errors are swallowed by callers.

use std::fs::File;
use std::path::Path;

use bytes::Bytes;

use crate::Result;

/// Advisory access-pattern hint for the OS page cache.
///
/// The full POSIX hint set is exposed even though only some variants have
/// callers today (point reads use `Random`, scans/compaction `Sequential`);
/// the remaining hints are used by the compaction admission policy.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadviseAdvice {
    /// No particular pattern (the default).
    Normal,
    /// Reads will be at random offsets; disables readahead.
    Random,
    /// Reads will be mostly sequential; aggressive readahead.
    Sequential,
    /// The range will be needed soon; start readahead now.
    WillNeed,
    /// The range will not be needed soon; may be evicted from the page cache.
    DontNeed,
    /// The range will be read exactly once.
    NoReuse,
}

/// A file that supports thread-safe positioned reads.
pub trait RandomAccessFile: Send + Sync {
    /// Read exactly `len` bytes starting at `offset`.  Returns an error on
    /// short reads (e.g. `offset + len` beyond end of file).
    fn read_exact_at(&self, offset: u64, len: usize) -> Result<Bytes>;

    /// File length in bytes.
    fn len(&self) -> Result<u64>;

    /// Hint the OS about an upcoming access pattern.  Advisory only.
    fn fadvise(&self, offset: u64, len: u64, advice: FadviseAdvice) -> Result<()>;
}

/// A [`RandomAccessFile`] backed by a plain [`std::fs::File`].
///
/// On Unix this uses `pread`, which does not touch the shared file offset.
/// On other platforms it falls back to a mutex around seek+read; the trait
/// contract is identical either way.
pub struct StdRandomAccessFile {
    #[cfg(unix)]
    file: File,
    #[cfg(not(unix))]
    file: std::sync::Mutex<File>,
}

impl StdRandomAccessFile {
    /// Open an existing file for positioned reads.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        Ok(Self::from_file(file))
    }

    /// Wrap an already-open file.
    pub fn from_file(file: File) -> Self {
        #[cfg(unix)]
        {
            Self { file }
        }
        #[cfg(not(unix))]
        {
            Self {
                file: std::sync::Mutex::new(file),
            }
        }
    }
}

#[cfg(unix)]
impl RandomAccessFile for StdRandomAccessFile {
    fn read_exact_at(&self, offset: u64, len: usize) -> Result<Bytes> {
        use std::os::unix::fs::FileExt;
        let mut buf = vec![0u8; len];
        self.file.read_exact_at(&mut buf, offset)?;
        Ok(Bytes::from(buf))
    }

    fn len(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    fn fadvise(&self, offset: u64, len: u64, advice: FadviseAdvice) -> Result<()> {
        fadvise_impl(&self.file, offset, len, advice)
    }
}

#[cfg(not(unix))]
impl RandomAccessFile for StdRandomAccessFile {
    fn read_exact_at(&self, offset: u64, len: usize) -> Result<Bytes> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf)?;
        Ok(Bytes::from(buf))
    }

    fn len(&self) -> Result<u64> {
        Ok(self.file.lock().unwrap().metadata()?.len())
    }

    fn fadvise(&self, _offset: u64, _len: u64, _advice: FadviseAdvice) -> Result<()> {
        Ok(())
    }
}

/// Map the hint to `posix_fadvise` on Linux.  Note that `posix_fadvise`
/// returns the error number directly instead of setting `errno`.
#[cfg(all(unix, target_os = "linux"))]
fn fadvise_impl(file: &File, offset: u64, len: u64, advice: FadviseAdvice) -> Result<()> {
    use std::os::unix::io::AsRawFd;
    let adv = match advice {
        FadviseAdvice::Normal => libc::POSIX_FADV_NORMAL,
        FadviseAdvice::Random => libc::POSIX_FADV_RANDOM,
        FadviseAdvice::Sequential => libc::POSIX_FADV_SEQUENTIAL,
        FadviseAdvice::WillNeed => libc::POSIX_FADV_WILLNEED,
        FadviseAdvice::DontNeed => libc::POSIX_FADV_DONTNEED,
        FadviseAdvice::NoReuse => libc::POSIX_FADV_NOREUSE,
    };
    let ret = unsafe { libc::posix_fadvise(file.as_raw_fd(), offset as _, len as _, adv) };
    if ret != 0 {
        return Err(crate::Error::Io(std::io::Error::from_raw_os_error(ret)));
    }
    Ok(())
}

/// `posix_fadvise` does not exist on macOS or the BSDs; the hint is advisory
/// so this is a no-op there.
#[cfg(all(unix, not(target_os = "linux")))]
fn fadvise_impl(_file: &File, _offset: u64, _len: u64, _advice: FadviseAdvice) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pattern(dir: &tempfile::TempDir, len: usize) -> std::path::PathBuf {
        let path = dir.path().join("data.bin");
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn read_exact_at_reads_the_right_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pattern(&dir, 10_000);
        let file = StdRandomAccessFile::open(&path).unwrap();

        assert_eq!(file.len().unwrap(), 10_000);
        let head = file.read_exact_at(0, 4).unwrap();
        assert_eq!(head.as_ref(), &[0, 1, 2, 3]);
        let mid = file.read_exact_at(251, 4).unwrap();
        assert_eq!(mid.as_ref(), &[0, 1, 2, 3], "offset 251 wraps the pattern");
        let tail = file.read_exact_at(9_999, 1).unwrap();
        assert_eq!(tail.as_ref(), &[(9_999 % 251) as u8]);
    }

    #[test]
    fn read_past_eof_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pattern(&dir, 100);
        let file = StdRandomAccessFile::open(&path).unwrap();
        assert!(file.read_exact_at(100, 1).is_err());
        assert!(file.read_exact_at(50, 100).is_err());
        // An empty read at EOF is fine.
        assert_eq!(file.read_exact_at(100, 0).unwrap().len(), 0);
    }

    #[test]
    fn concurrent_reads_do_not_interfere() {
        // With a shared seek offset, interleaved seek+read pairs would corrupt
        // each other's reads.  `pread` has no shared offset, so every thread
        // must observe exactly its own region.
        let dir = tempfile::tempdir().unwrap();
        let path = write_pattern(&dir, 1 << 20);
        let file = std::sync::Arc::new(StdRandomAccessFile::open(&path).unwrap());

        let mut handles = Vec::new();
        for t in 0..8u64 {
            let file = std::sync::Arc::clone(&file);
            handles.push(std::thread::spawn(move || {
                for i in 0..500u64 {
                    let offset = ((t * 977 + i * 131) % (1 << 19)) * 2;
                    let got = file.read_exact_at(offset, 2).unwrap();
                    assert_eq!(
                        got.as_ref(),
                        &[(offset % 251) as u8, ((offset + 1) % 251) as u8],
                        "thread {t} read wrong bytes at {offset}"
                    );
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn fadvise_is_accepted_and_advisory() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pattern(&dir, 4096);
        let file = StdRandomAccessFile::open(&path).unwrap();
        for advice in [
            FadviseAdvice::Normal,
            FadviseAdvice::Random,
            FadviseAdvice::Sequential,
            FadviseAdvice::WillNeed,
            FadviseAdvice::DontNeed,
            FadviseAdvice::NoReuse,
        ] {
            // Hints may fail on some filesystems (advisory), but must never
            // change what a subsequent read returns.
            let _ = file.fadvise(0, 4096, advice);
            let got = file.read_exact_at(0, 4).unwrap();
            assert_eq!(got.as_ref(), &[0, 1, 2, 3]);
        }
    }
}
