//! Range and prefix cursors for `ArtMap`.

use bytes::Bytes;

use crate::error::Result;

/// An iterator over a sorted range or prefix of keys in an `ArtMap`.
#[derive(Debug)]
pub struct ArtCursor {
    // TODO: implement cursor state.
}

impl ArtCursor {
    /// Advance to the next key/value pair.
    pub fn next(&mut self) -> Result<Option<(Bytes, Bytes)>> {
        // TODO: implement traversal.
        Ok(None)
    }
}
