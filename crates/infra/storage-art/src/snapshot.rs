//! Optional in-memory snapshot encode/decode for `ArtMap`.
//!
//! Snapshots are *not* durable on their own; they are intended for migration,
//! testing, or as an input to a wrapping engine's checkpoint mechanism.

use crate::error::Result;

/// Encode the current map state into a byte vector.
pub fn encode() -> Result<Vec<u8>> {
    // TODO: implement serialization.
    Ok(Vec::new())
}

/// Decode a map state from a byte vector.
pub fn decode(_bytes: &[u8]) -> Result<()> {
    // TODO: implement deserialization.
    Ok(())
}
