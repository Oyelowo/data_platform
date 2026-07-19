//! Compression codec implementations.

use bytes::Bytes;

use crate::{Error, Result};

/// Maximum decompressed block size. Engines may override this, but a global
/// default prevents runaway allocation on corrupt length prefixes.
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 16 * 1024 * 1024; // 16 MiB

/// Size of the little-endian uncompressed-length prefix used by LZ4 and ZSTD.
const LEN_PREFIX_SIZE: usize = 4;

/// Right-shift used for the 1/8 minimum-savings threshold.
const MIN_SAVINGS_SHIFT: u32 = 3;

/// Compression codec selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionCodecType {
    /// No compression.
    #[default]
    None,
    /// LZ4 block compression.
    Lz4,
    /// Zstd compression.
    Zstd,
    /// Snappy block compression.
    Snappy,
}

/// A compression codec.
pub trait CompressionCodec: Send + Sync {
    /// Codec type.
    fn ty(&self) -> CompressionCodecType;

    /// Compress `input`. Returns the stored bytes and the type to record.
    ///
    /// The type may be `None` if compression did not save enough space.
    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionCodecType)>;

    /// Decompress stored bytes back to the original block.
    fn decode(&self, input: &[u8]) -> Result<Bytes>;
}

/// Whether storing `stored` bytes instead of `original` bytes is worthwhile.
fn worth_storing_compressed(original: usize, stored: usize) -> bool {
    stored + (original >> MIN_SAVINGS_SHIFT) < original
}

/// Prepend the 4-byte little-endian uncompressed length to a codec payload.
fn prepend_len(input_len: usize, mut payload: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(LEN_PREFIX_SIZE + payload.len());
    out.extend_from_slice(&(input_len as u32).to_le_bytes());
    out.append(&mut payload);
    out
}

/// Split a length-prefixed payload into (uncompressed length, codec payload).
fn read_len_prefix(input: &[u8]) -> Result<(usize, &[u8])> {
    if input.len() < LEN_PREFIX_SIZE {
        return Err(Error::TooShort);
    }
    let len = u32::from_le_bytes(
        input[..LEN_PREFIX_SIZE]
            .try_into()
            .expect("length prefix is exactly 4 bytes"),
    ) as usize;
    if len > DEFAULT_MAX_DECOMPRESSED_SIZE {
        return Err(Error::DecompressedTooLarge(len, DEFAULT_MAX_DECOMPRESSED_SIZE));
    }
    Ok((len, &input[LEN_PREFIX_SIZE..]))
}

fn reject_oversized(input: &[u8]) -> Result<()> {
    if input.len() > u32::MAX as usize {
        return Err(Error::InputTooLarge(input.len()));
    }
    Ok(())
}

/// LZ4 block codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct Lz4Codec;

impl CompressionCodec for Lz4Codec {
    fn ty(&self) -> CompressionCodecType {
        CompressionCodecType::Lz4
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionCodecType)> {
        reject_oversized(input)?;
        let payload = lz4::block::compress(input, None, false)
            .map_err(|e| Error::Lz4(e.to_string()))?;
        if !worth_storing_compressed(input.len(), LEN_PREFIX_SIZE + payload.len()) {
            return Ok((input.to_vec(), CompressionCodecType::None));
        }
        Ok((prepend_len(input.len(), payload), CompressionCodecType::Lz4))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        let (len, payload) = read_len_prefix(input)?;
        let out = lz4::block::decompress(payload, Some(len as i32))
            .map_err(|e| Error::Lz4(e.to_string()))?;
        if out.len() != len {
            return Err(Error::LengthMismatch(out.len(), len));
        }
        Ok(Bytes::from(out))
    }
}

/// Zstd codec.
#[derive(Debug, Clone, Copy)]
pub struct ZstdCodec {
    /// Compression level; zstd's default is 3.
    pub level: i32,
}

impl Default for ZstdCodec {
    fn default() -> Self {
        Self { level: 3 }
    }
}

impl CompressionCodec for ZstdCodec {
    fn ty(&self) -> CompressionCodecType {
        CompressionCodecType::Zstd
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionCodecType)> {
        reject_oversized(input)?;
        let payload = zstd::bulk::compress(input, self.level)
            .map_err(|e| Error::Zstd(e.to_string()))?;
        if !worth_storing_compressed(input.len(), LEN_PREFIX_SIZE + payload.len()) {
            return Ok((input.to_vec(), CompressionCodecType::None));
        }
        Ok((prepend_len(input.len(), payload), CompressionCodecType::Zstd))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        let (len, payload) = read_len_prefix(input)?;
        let out = zstd::bulk::decompress(payload, len)
            .map_err(|e| Error::Zstd(e.to_string()))?;
        if out.len() != len {
            return Err(Error::LengthMismatch(out.len(), len));
        }
        Ok(Bytes::from(out))
    }
}

/// Snappy block codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct SnappyCodec;

impl CompressionCodec for SnappyCodec {
    fn ty(&self) -> CompressionCodecType {
        CompressionCodecType::Snappy
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionCodecType)> {
        reject_oversized(input)?;
        let mut encoder = snap::raw::Encoder::new();
        let payload = encoder
            .compress_vec(input)
            .map_err(|e| Error::Snappy(e.to_string()))?;
        if !worth_storing_compressed(input.len(), payload.len()) {
            return Ok((input.to_vec(), CompressionCodecType::None));
        }
        Ok((payload, CompressionCodecType::Snappy))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        let len = snap::raw::decompress_len(input)
            .map_err(|e| Error::Snappy(e.to_string()))?;
        if len > DEFAULT_MAX_DECOMPRESSED_SIZE {
            return Err(Error::DecompressedTooLarge(len, DEFAULT_MAX_DECOMPRESSED_SIZE));
        }
        let mut decoder = snap::raw::Decoder::new();
        let mut out = vec![0u8; len];
        decoder
            .decompress(input, &mut out)
            .map_err(|e| Error::Snappy(e.to_string()))?;
        Ok(Bytes::from(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_codecs() {
        let data = b"the quick brown fox jumps over the lazy dog. ".repeat(100);
        let codecs: Vec<Box<dyn CompressionCodec>> = vec![
            Box::new(Lz4Codec),
            Box::new(ZstdCodec::default()),
            Box::new(SnappyCodec),
        ];
        for codec in codecs {
            let (encoded, ty) = codec.encode(&data).unwrap();
            let decoded = codec.decode(&encoded).unwrap();
            assert_eq!(decoded.as_ref(), data.as_slice());
            assert_eq!(ty, codec.ty());
        }
    }

    #[test]
    fn incompressible_data_stored_as_none() {
        let data = vec![0u8; 16];
        let codecs: Vec<Box<dyn CompressionCodec>> = vec![
            Box::new(Lz4Codec),
            Box::new(ZstdCodec::default()),
            Box::new(SnappyCodec),
        ];
        for codec in codecs {
            let (_encoded, ty) = codec.encode(&data).unwrap();
            assert_eq!(ty, CompressionCodecType::None);
        }
    }
}
