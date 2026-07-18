//! Pluggable storage I/O abstraction.
//!
//! The `storage-btree` engine normally uses the production `RealBackend`, which
//! delegates directly to `std::fs`. Tests can substitute a `FaultyBackend` to
//! inject deterministic faults at every semantic I/O boundary.

use std::io::Result as IoResult;
use std::path::Path;

use crate::page::PageId;

/// Options controlling how a file is opened.
///
/// This mirrors the subset of `std::fs::OpenOptions` that the engine needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    create: bool,
    truncate: bool,
}

impl OpenOptions {
    /// Create a new option set with all flags disabled.
    pub fn new() -> Self {
        Self {
            read: false,
            write: false,
            create: false,
            truncate: false,
        }
    }

    /// Set the read flag.
    pub fn read(mut self, read: bool) -> Self {
        self.read = read;
        self
    }

    /// Set the write flag.
    pub fn write(mut self, write: bool) -> Self {
        self.write = write;
        self
    }

    /// Set the create flag.
    pub fn create(mut self, create: bool) -> Self {
        self.create = create;
        self
    }

    /// Set the truncate flag.
    pub fn truncate(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    /// Return whether read access was requested.
    pub(crate) fn is_read(&self) -> bool {
        self.read
    }

    /// Return whether write access was requested.
    pub(crate) fn is_write(&self) -> bool {
        self.write
    }

    /// Return whether file creation was requested.
    pub(crate) fn is_create(&self) -> bool {
        self.create
    }

    /// Return whether truncation was requested.
    pub(crate) fn is_truncate(&self) -> bool {
        self.truncate
    }
}

/// A single persistent file accessed by the engine.
///
/// All methods are `&self` so the file can be shared between threads through an
/// `Arc`.
pub trait StorageFile: Send + Sync {
    /// Read exactly `buf.len()` bytes at `offset`.
    ///
    /// A short read is reported as `std::io::ErrorKind::UnexpectedEof`.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> IoResult<()>;

    /// Write `buf` at `offset`.
    fn write_at(&self, buf: &[u8], offset: u64) -> IoResult<()>;

    /// Flush all buffers to stable storage.
    fn sync(&self) -> IoResult<()>;

    /// Change the length of the file.
    fn set_len(&self, len: u64) -> IoResult<()>;

    /// Return the current length of the file.
    fn len(&self) -> IoResult<u64>;

    /// Return true if the file is empty.
    fn is_empty(&self) -> IoResult<bool> {
        Ok(self.len()? == 0)
    }
}

impl StorageFile for Box<dyn StorageFile> {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> IoResult<()> {
        (**self).read_at(buf, offset)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> IoResult<()> {
        (**self).write_at(buf, offset)
    }

    fn sync(&self) -> IoResult<()> {
        (**self).sync()
    }

    fn set_len(&self, len: u64) -> IoResult<()> {
        (**self).set_len(len)
    }

    fn len(&self) -> IoResult<u64> {
        (**self).len()
    }
}

/// A factory for opening, renaming, and removing persistent files.
///
/// The engine uses this trait for all disk access so that tests can swap in a
/// fault-injecting implementation without changing engine logic.
pub trait StorageBackend: Send + Sync + 'static {
    /// Open `path` with the supplied options and return a boxed file handle.
    fn open(&self, path: &Path, opts: OpenOptions) -> IoResult<Box<dyn StorageFile>>;

    /// Atomically rename `from` to `to`.
    fn rename(&self, from: &Path, to: &Path) -> IoResult<()>;

    /// Remove `path`.
    fn remove(&self, path: &Path) -> IoResult<()>;

    /// Return true if `path` exists.
    fn exists(&self, path: &Path) -> bool;

    /// Sync the directory at `path` so that metadata changes are durable.
    fn sync_dir(&self, path: &Path) -> IoResult<()>;

    /// Hook called before a tagged operation.
    ///
    /// `FaultyBackend` overrides this to count operations, log the sequence, and
    /// inject `FailNth` / `FailEvery` faults. Production backends leave this as
    /// a no-op.
    fn pre_op(&self, _boundary: Boundary) -> IoResult<()> {
        Ok(())
    }

    /// Hook called after a successful read.
    ///
    /// `FaultyBackend` overrides this to implement `CorruptReadNth`.
    fn corrupt_read(&self, _boundary: Boundary, _buf: &mut [u8], _offset: u64) -> IoResult<()> {
        Ok(())
    }

    /// Hook called before a write to determine how many bytes to write.
    ///
    /// `FaultyBackend` overrides this to implement `PartialWriteNth` by returning
    /// a length smaller than `buf_len`. Production backends return `buf_len`.
    fn truncate_write(&self, _boundary: Boundary, buf_len: usize) -> IoResult<usize> {
        Ok(buf_len)
    }

    /// Hook called before an append to decide whether to drop it.
    ///
    /// `FaultyBackend` overrides this to implement `DropAppends`. If this
    /// returns `true`, the caller should skip the write and behave as if the
    /// buffered bytes were lost in a power failure.
    fn drop_append(&self, _boundary: Boundary) -> bool {
        false
    }

    /// Return the recorded operation sequence.
    ///
    /// `FaultyBackend` returns every `(boundary, family)` pair observed so far;
    /// other backends return an empty vector.
    fn operation_log(&self) -> Vec<(Boundary, OpFamily)> {
        Vec::new()
    }

    /// Simulate a power loss by truncating files to their last-synced length.
    ///
    /// `FaultyBackend` implements this; other backends do nothing.
    fn crash(&self) {}
}

/// Semantic boundary at which a fault can be injected.
///
/// The engine tags each I/O operation with a boundary so that tests can target
/// specific moments ("fail the first `PageSync` after a `WalSync`") rather than
/// raw positional counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Boundary {
    /// Append of a record to the physiological WAL.
    WalAppendRecord,
    /// Flush of a completed WAL segment.
    WalFlushSegment,
    /// fsync of the WAL.
    WalSync,
    /// Read of a page from `pages.dat`.
    PageRead(PageId),
    /// Write of a page to `pages.dat`.
    PageWrite(PageId),
    /// fsync of `pages.dat` or its containing directory.
    PageSync(PageId),
    /// Read of a value from `values.log`.
    ValueLogRead,
    /// Append of a value to `values.log`.
    ValueLogAppend,
    /// fsync of `values.log`.
    ValueLogSync,
    /// Write of the temporary `META` file.
    MetaWriteTemp,
    /// Rename installing a new `META` file.
    MetaRename,
    /// fsync of the directory containing `META`.
    MetaDirSync,
}

/// Operation family used for counting and matching rules.
///
/// A family collapses several boundaries into a single countable stream. For
/// example, all `PageWrite` boundaries belong to `OpFamily::PageWrite`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpFamily {
    /// WAL record append.
    WalAppend,
    /// WAL segment flush.
    WalFlush,
    /// WAL fsync.
    WalSync,
    /// Page read.
    PageRead,
    /// Page write.
    PageWrite,
    /// Page fsync.
    PageSync,
    /// Value-log read.
    ValueLogRead,
    /// Value-log append.
    ValueLogAppend,
    /// Value-log fsync.
    ValueLogSync,
    /// META file write.
    MetaWrite,
    /// File rename.
    FileRename,
    /// Directory fsync.
    DirSync,
}

impl Boundary {
    /// Return the operation family this boundary belongs to.
    pub fn family(&self) -> OpFamily {
        match self {
            Boundary::WalAppendRecord => OpFamily::WalAppend,
            Boundary::WalFlushSegment => OpFamily::WalFlush,
            Boundary::WalSync => OpFamily::WalSync,
            Boundary::PageRead(_) => OpFamily::PageRead,
            Boundary::PageWrite(_) => OpFamily::PageWrite,
            Boundary::PageSync(_) => OpFamily::PageSync,
            Boundary::ValueLogRead => OpFamily::ValueLogRead,
            Boundary::ValueLogAppend => OpFamily::ValueLogAppend,
            Boundary::ValueLogSync => OpFamily::ValueLogSync,
            Boundary::MetaWriteTemp => OpFamily::MetaWrite,
            Boundary::MetaRename => OpFamily::FileRename,
            Boundary::MetaDirSync => OpFamily::DirSync,
        }
    }
}

pub mod fault;
pub mod real;

pub use fault::{FaultRule, FaultSchedule, FaultyBackend};
pub use real::{RealBackend, RealFile};
