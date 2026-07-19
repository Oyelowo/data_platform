//! Shared binary-format primitives for storage engines.
//!
//! This crate provides checksums, variable-length integer encoding, and
//! little-endian integer helpers that are used by multiple engines. It does
//! not impose a single on-disk format; each engine still defines its own
//! record/header layout, but it can reuse the primitives here instead of
//! duplicating byte manipulation.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]

pub mod checksum;
pub mod primitive;
pub mod varint;

pub use checksum::{crc32c, Crc32c};
pub use primitive::{read_u16_le, read_u32_le, read_u64_le, write_u16_le, write_u32_le, write_u64_le};
pub use varint::{decode_uvarint, encode_uvarint, encoded_uvarint_len, read_uvarint, write_uvarint};

/// Re-export `bytes` so consumers do not need to add it separately for the
/// helper traits used by this crate.
pub use bytes;
