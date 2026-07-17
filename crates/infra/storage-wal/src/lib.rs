//! Append-only write-ahead log with group commit.
//!
//! This crate provides a segment-based WAL with binary records, CRC32C
//! checksums, and a background fsync worker that batches concurrent commit
//! requests into a single disk flush (group commit). The public API is
//! synchronous and `tokio`-free, but internally uses a dedicated background
//! thread and channel-based coordination.
//!
//! # Design goals
//!
//! * Durability with bounded latency: callers block until their records are
//!   durable, but concurrent commits are merged into one `fsync`.
//! * Crash safety: every record is checksummed and length-prefixed so torn
//!   writes and truncation are detectable.
//! * Simplicity: synchronous public API; the engine owns one writer thread.
//!
//! # Quick example
//!
//! ```rust,no_run
//! use storage_wal::{Wal, WalOptions, Durability};
//!
//! let dir = tempfile::tempdir().unwrap();
//! let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
//! let lsn = wal.append(b"hello", Durability::Immediate).unwrap();
//! let reader = wal.reader();
//! let record = reader.read(lsn).unwrap().unwrap();
//! assert_eq!(record.payload, &b"hello"[..]);
//! ```

mod committer;
mod reader;
mod record;
mod segment;
mod wal;

pub use record::{Durability, RECORD_HEADER_SIZE, Record, RecordType};
pub use wal::{Wal, WalOptions};

/// Logical sequence number. Monotonically increasing within a WAL.
pub type Lsn = u64;

/// Result alias used throughout this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the WAL.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("corrupt record at lsn {lsn}: {reason}")]
    CorruptRecord { lsn: Lsn, reason: String },

    #[error("checksum mismatch at lsn {lsn}: expected {expected:#x}, got {got:#x}")]
    ChecksumMismatch { lsn: Lsn, expected: u32, got: u32 },

    #[error("record not found at lsn {lsn}")]
    RecordNotFound { lsn: Lsn },

    #[error("wal is closed")]
    Closed,

    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}
