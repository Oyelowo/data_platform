//! Content-addressed object store implementing `storage_traits::BlobStore`.
//!
//! `storage-blob` provides durable, streaming storage for opaque byte IDs.
//! Objects are packed into append-only volume files and indexed by an in-memory
//! map that is recovered from a `storage-wal` backed index log.
//!
//! See `.doc/storage-blob-design.md` for the full architecture and checklist.

pub mod error;
pub mod format;
pub mod gc;
pub mod index;
pub mod index_wal;
pub mod options;
pub mod recovery;
pub mod store;
pub mod volume;
pub mod volume_manager;

pub use error::{Error, Result};
pub use options::BlobStoreOptions;
pub use store::{BlobStoreImpl, BlobWriter};
pub use volume::{BlobPayloadReader, RecordLocation, VolumeReader, VolumeWriter};
