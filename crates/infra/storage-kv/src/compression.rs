//! Block compression codecs for SSTable blocks.
//!
//! Data and index blocks may be stored compressed.  The block trailer's type
//! byte records the codec and its CRC32C covers the stored (compressed)
//! bytes, so on-disk corruption is detected *before* any decompression is
//! attempted.
//!
//! Stored payload layout per codec:
//!
//! * `None`: the raw block bytes.
//! * `Lz4` / `Zstd`: a 4-byte little-endian uncompressed length followed by
//!   the raw codec payload.  The length prefix lets the reader bound its
//!   output buffer *before* allocating, which protects against corrupt or
//!   malicious payloads claiming huge decompressed sizes.
//! * `Snappy`: the raw snappy block format, which embeds the uncompressed
//!   length in its own header; the length is parsed and checked against the
//!   same bound before decoding.
//!
//! A block is only stored compressed when the codec shrinks it (including the
//! length prefix) to at most 7/8 of its original size — RocksDB's 12.5%
//! minimum-savings threshold.  Otherwise the raw block is stored with type
//! `None`: the CPU cost of decompression is not worth a smaller saving.
//!
//! The concrete codecs are provided by the shared [`storage_compression`]
//! crate; this module wraps them so that the on-disk [`CompressionType`]
//! enum and the engine's error type stay local.

use bytes::Bytes;

use crate::sstable::format::CompressionType;
use crate::{Error, Result};

#[cfg(test)]
use storage_compression::CompressionCodec as SharedCompressionCodec;

/// Right-shift used for the 1/8 minimum-savings threshold: a block is stored
/// compressed only when `stored + (original >> 3) < original`.
#[cfg(test)]
const MIN_SAVINGS_SHIFT: u32 = 3;

/// A block compression codec.
pub trait CompressionCodec: Send + Sync {
    /// The type recorded in the block trailer when this codec's output is
    /// stored.
    fn ty(&self) -> CompressionType;

    /// Compress `input`.  Returns the bytes to store and the compression type
    /// to record in the trailer; the type is `CompressionType::None` when
    /// compression did not shrink the block enough to be worthwhile.
    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)>;

    /// Decompress stored bytes back into the original block.
    #[allow(dead_code)]
    fn decode(&self, input: &[u8]) -> Result<Bytes>;
}

/// Return the codec for `ty`, or `None` for `CompressionType::None`.
pub fn codec_for(ty: CompressionType) -> Option<Box<dyn CompressionCodec>> {
    storage_compression::codec_for(to_shared_type(ty)).map(|codec| {
        Box::new(SharedCodecAdapter(codec)) as Box<dyn CompressionCodec>
    })
}

/// Decompress a stored block payload read from an SSTable.
///
/// Takes ownership of the stored bytes so the `None` path is zero-copy: the
/// already-allocated read buffer is returned as-is instead of being copied.
pub fn decompress_block(ty: CompressionType, stored: Bytes) -> Result<Bytes> {
    storage_compression::decompress(to_shared_type(ty), stored).map_err(map_err)
}

/// Whether storing `stored` bytes instead of `original` bytes is worthwhile.
#[cfg(test)]
fn worth_storing_compressed(original: usize, stored: usize) -> bool {
    stored + (original >> MIN_SAVINGS_SHIFT) < original
}

fn map_err(e: storage_compression::Error) -> Error {
    Error::Sstable(e.to_string())
}

fn to_shared_type(ty: CompressionType) -> storage_compression::CompressionCodecType {
    match ty {
        CompressionType::None => storage_compression::CompressionCodecType::None,
        CompressionType::Lz4 => storage_compression::CompressionCodecType::Lz4,
        CompressionType::Zstd => storage_compression::CompressionCodecType::Zstd,
        CompressionType::Snappy => storage_compression::CompressionCodecType::Snappy,
    }
}

fn from_shared_type(ty: storage_compression::CompressionCodecType) -> CompressionType {
    match ty {
        storage_compression::CompressionCodecType::None => CompressionType::None,
        storage_compression::CompressionCodecType::Lz4 => CompressionType::Lz4,
        storage_compression::CompressionCodecType::Zstd => CompressionType::Zstd,
        storage_compression::CompressionCodecType::Snappy => CompressionType::Snappy,
    }
}

struct SharedCodecAdapter(Box<dyn storage_compression::CompressionCodec>);

impl CompressionCodec for SharedCodecAdapter {
    fn ty(&self) -> CompressionType {
        from_shared_type(self.0.ty())
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)> {
        let (encoded, ty) = self.0.encode(input).map_err(map_err)?;
        Ok((encoded, from_shared_type(ty)))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        self.0.decode(input).map_err(map_err)
    }
}

/// LZ4 block codec (default acceleration).  The engine default: decompression
/// speed matters on every read, and LZ4 is the fastest codec we support.
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Lz4Codec;

#[cfg(test)]
impl CompressionCodec for Lz4Codec {
    fn ty(&self) -> CompressionType {
        CompressionType::Lz4
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)> {
        let (encoded, ty) = storage_compression::codec::Lz4Codec
            .encode(input)
            .map_err(map_err)?;
        Ok((encoded, from_shared_type(ty)))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        storage_compression::codec::Lz4Codec
            .decode(input)
            .map_err(map_err)
    }
}

/// ZSTD codec.  Used for the bottommost level, where blocks are read rarely
/// and the better compression ratio pays for the slower decompression.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct ZstdCodec {
    /// Compression level; zstd's own default is 3.
    pub level: i32,
}

#[cfg(test)]
impl Default for ZstdCodec {
    fn default() -> Self {
        Self { level: 3 }
    }
}

#[cfg(test)]
impl CompressionCodec for ZstdCodec {
    fn ty(&self) -> CompressionType {
        CompressionType::Zstd
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)> {
        let (encoded, ty) = storage_compression::codec::ZstdCodec { level: self.level }
            .encode(input)
            .map_err(map_err)?;
        Ok((encoded, from_shared_type(ty)))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        storage_compression::codec::ZstdCodec { level: self.level }
            .decode(input)
            .map_err(map_err)
    }
}

/// Snappy block codec (the classic LevelDB codec).
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub struct SnappyCodec;

#[cfg(test)]
impl CompressionCodec for SnappyCodec {
    fn ty(&self) -> CompressionType {
        CompressionType::Snappy
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)> {
        let (encoded, ty) = storage_compression::codec::SnappyCodec
            .encode(input)
            .map_err(map_err)?;
        Ok((encoded, from_shared_type(ty)))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        storage_compression::codec::SnappyCodec
            .decode(input)
            .map_err(map_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Highly compressible data: repeated patterned runs.
    fn compressible(len: usize) -> Vec<u8> {
        (0..len).map(|i| b'a' + (i / 64) as u8 % 26).collect()
    }

    /// Deterministic pseudo-random (incompressible) data, xorshift64.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 32) as u8
            })
            .collect()
    }

    fn codecs() -> Vec<Box<dyn CompressionCodec>> {
        vec![
            Box::new(Lz4Codec),
            Box::new(ZstdCodec::default()),
            Box::new(ZstdCodec { level: 19 }),
            Box::new(SnappyCodec),
        ]
    }

    #[test]
    fn roundtrip_compressible() {
        for codec in codecs() {
            for len in [1usize, 64, 4 * 1024, 1024 * 1024] {
                let input = compressible(len);
                let (stored, ty) = codec.encode(&input).unwrap();
                let decoded = decompress_block(ty, Bytes::copy_from_slice(&stored)).unwrap();
                assert_eq!(decoded.as_ref(), input.as_slice(), "len {len} roundtrip");
                if len >= 4 * 1024 {
                    // Larger compressible blocks must actually be compressed.
                    assert_eq!(ty, codec.ty(), "len {len} should compress");
                    assert!(
                        stored.len() < input.len(),
                        "len {len}: stored {} >= original {}",
                        stored.len(),
                        input.len()
                    );
                }
            }
        }
    }

    #[test]
    fn roundtrip_incompressible_falls_back_to_none() {
        for codec in codecs() {
            let input = incompressible(64 * 1024);
            let (stored, ty) = codec.encode(&input).unwrap();
            assert_eq!(ty, CompressionType::None);
            assert_eq!(stored, input);
            // The raw payload is readable as an uncompressed block.
            let decoded = decompress_block(ty, Bytes::copy_from_slice(&stored)).unwrap();
            assert_eq!(decoded.as_ref(), input.as_slice());
        }
    }

    #[test]
    fn empty_input_stored_uncompressed() {
        for codec in codecs() {
            let (stored, ty) = codec.encode(&[]).unwrap();
            assert_eq!(ty, CompressionType::None);
            assert!(stored.is_empty());
        }
    }

    #[test]
    fn decode_rejects_truncated_len_prefix() {
        for ty in [CompressionType::Lz4, CompressionType::Zstd] {
            assert!(decompress_block(ty, Bytes::from_static(&[1, 2])).is_err());
            assert!(decompress_block(ty, Bytes::new()).is_err());
        }
    }

    #[test]
    fn decode_rejects_oversized_len_prefix_before_allocating() {
        // u32::MAX little-endian: claims a ~4 GiB decompressed block.
        let evil = Bytes::copy_from_slice(&u32::MAX.to_le_bytes());
        for ty in [CompressionType::Lz4, CompressionType::Zstd] {
            let err = decompress_block(ty, evil.clone()).unwrap_err();
            assert!(
                err.to_string().contains("exceeds maximum"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn decode_corrupt_payload_never_returns_original() {
        // The codecs carry no internal checksum (that is what the block
        // trailer's CRC32C is for), so a corrupted payload may fail to decode
        // or may decode to wrong bytes — but it must never silently
        // reproduce the original block.
        for codec in codecs() {
            let input = compressible(8 * 1024);
            let (mut stored, ty) = codec.encode(&input).unwrap();
            assert_eq!(ty, codec.ty());
            // Corrupt the tail of the codec payload (past any length prefix).
            let last = stored.len() - 1;
            stored[last] ^= 0xFF;
            match codec.decode(&stored) {
                Err(_) => {}
                Ok(decoded) => assert_ne!(
                    decoded.as_ref(),
                    input.as_slice(),
                    "corruption must not silently reproduce the original block"
                ),
            }
        }
    }

    #[test]
    fn decode_rejects_truncated_payload() {
        for codec in codecs() {
            let input = compressible(8 * 1024);
            let (stored, ty) = codec.encode(&input).unwrap();
            assert_eq!(ty, codec.ty());
            let truncated = &stored[..stored.len() / 2];
            assert!(codec.decode(truncated).is_err());
        }
    }

    #[test]
    fn snappy_decode_rejects_oversized_embedded_len() {
        // Craft a snappy header claiming a huge uncompressed length: varint
        // preamble for 0xFFFFFFFF followed by no payload.
        let evil = [0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        let err = SnappyCodec.decode(&evil).unwrap_err();
        assert!(
            err.to_string().contains("exceeds maximum"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn savings_threshold() {
        assert!(worth_storing_compressed(1000, 874));
        assert!(!worth_storing_compressed(1000, 875));
        assert!(!worth_storing_compressed(1000, 1000));
        // Tiny blocks: any real saving counts.
        assert!(worth_storing_compressed(7, 6));
        assert!(!worth_storing_compressed(7, 7));
    }

    #[test]
    fn codec_for_covers_all_types() {
        assert!(codec_for(CompressionType::None).is_none());
        for ty in [
            CompressionType::Lz4,
            CompressionType::Zstd,
            CompressionType::Snappy,
        ] {
            assert_eq!(codec_for(ty).unwrap().ty(), ty);
        }
    }
}
