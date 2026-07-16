//! Large-object storage trait.

use std::io::{Read, Write};

use crate::error::Result;

/// Storage for values too large to keep inline in a key-value engine.
///
/// Blobs are identified by an opaque byte ID, usually a content hash or a
/// generated object key.
pub trait BlobStore: Send + Sync + 'static {
    /// Error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reader type.
    type Reader: Read + Send;

    /// Writer type.
    type Writer: Write + Send;

    /// Store the contents of `reader` and return the number of bytes written.
    fn put(&self, id: &[u8], reader: &mut dyn Read) -> Result<u64, Self::Error>;

    /// Open an existing blob for reading.
    fn get(&self, id: &[u8]) -> Result<Self::Reader, Self::Error>;

    /// Delete a blob. Deleting a missing blob is not an error.
    fn delete(&self, id: &[u8]) -> Result<(), Self::Error>;

    /// Return the size of a blob, if it exists.
    fn size(&self, id: &[u8]) -> Result<Option<u64>, Self::Error>;
}
