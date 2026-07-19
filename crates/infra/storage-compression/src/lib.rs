//! Shared compression codec registry for storage engines.
//!
//! This crate provides a uniform interface for block compression codecs. Each
//! codec stores a small header so the decoder can bound its output allocation
//! before decompressing.

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]

use std::fmt;

use bytes::Bytes;

pub mod codec;

pub use codec::{CompressionCodec, CompressionCodecType};

/// Result type alias for compression operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by compression/decompression.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Input is too large to encode.
    #[error("input too large: {0} bytes")]
    InputTooLarge(usize),

    /// Stored payload is too short to contain the length prefix.
    #[error("compressed payload too short")]
    TooShort,

    /// Decompressed length prefix exceeds the configured maximum.
    #[error("decompressed length {0} exceeds maximum {1}")]
    DecompressedTooLarge(usize, usize),

    /// Decompressed length does not match the stored prefix.
    #[error("decompressed length {0} does not match prefix {1}")]
    LengthMismatch(usize, usize),

    /// LZ4 error.
    #[error("lz4 error: {0}")]
    Lz4(String),

    /// Zstd error.
    #[error("zstd error: {0}")]
    Zstd(String),

    /// Snappy error.
    #[error("snappy error: {0}")]
    Snappy(String),
}

impl fmt::Debug for dyn CompressionCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CompressionCodec({:?})", self.ty())
    }
}

/// Decompress `stored` using `ty`.
///
/// The `None` path returns `stored` without copying.
pub fn decompress(ty: CompressionCodecType, stored: Bytes) -> Result<Bytes> {
    match ty {
        CompressionCodecType::None => Ok(stored),
        CompressionCodecType::Lz4 => codec::Lz4Codec.decode(&stored),
        CompressionCodecType::Zstd => codec::ZstdCodec::default().decode(&stored),
        CompressionCodecType::Snappy => codec::SnappyCodec.decode(&stored),
    }
}

/// Return the codec implementation for `ty`, or `None` for `None`.
pub fn codec_for(ty: CompressionCodecType) -> Option<Box<dyn CompressionCodec>> {
    match ty {
        CompressionCodecType::None => None,
        CompressionCodecType::Lz4 => Some(Box::new(codec::Lz4Codec)),
        CompressionCodecType::Zstd => Some(Box::new(codec::ZstdCodec::default())),
        CompressionCodecType::Snappy => Some(Box::new(codec::SnappyCodec)),
    }
}
