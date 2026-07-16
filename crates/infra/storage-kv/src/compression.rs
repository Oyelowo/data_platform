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

use bytes::Bytes;

use crate::sstable::format::{CompressionType, MAX_BLOCK_SIZE};
use crate::{Error, Result};

/// Right-shift used for the 1/8 minimum-savings threshold: a block is stored
/// compressed only when `stored + (original >> 3) < original`.
const MIN_SAVINGS_SHIFT: u32 = 3;

/// Size of the little-endian uncompressed-length prefix used by the LZ4 and
/// ZSTD payload layouts.
const LEN_PREFIX_SIZE: usize = 4;

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
    fn decode(&self, input: &[u8]) -> Result<Bytes>;
}

/// Return the codec for `ty`, or `None` for `CompressionType::None`.
pub fn codec_for(ty: CompressionType) -> Option<Box<dyn CompressionCodec>> {
    match ty {
        CompressionType::None => None,
        CompressionType::Lz4 => Some(Box::new(Lz4Codec)),
        CompressionType::Zstd => Some(Box::new(ZstdCodec::default())),
        CompressionType::Snappy => Some(Box::new(SnappyCodec)),
    }
}

/// Decompress a stored block payload read from an SSTable.
///
/// Takes ownership of the stored bytes so the `None` path is zero-copy: the
/// already-allocated read buffer is returned as-is instead of being copied.
pub fn decompress_block(ty: CompressionType, stored: Bytes) -> Result<Bytes> {
    match ty {
        CompressionType::None => Ok(stored),
        CompressionType::Lz4 => Lz4Codec.decode(&stored),
        CompressionType::Zstd => ZstdCodec::default().decode(&stored),
        CompressionType::Snappy => SnappyCodec.decode(&stored),
    }
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

/// Split a length-prefixed payload into (uncompressed length, codec payload),
/// rejecting truncated prefixes and decompressed sizes above the format's
/// block-size limit *before* any allocation is made.
fn read_len_prefix(input: &[u8]) -> Result<(usize, &[u8])> {
    if input.len() < LEN_PREFIX_SIZE {
        return Err(Error::Sstable("compressed block too short".into()));
    }
    let len = u32::from_le_bytes(input[..LEN_PREFIX_SIZE].try_into().unwrap()) as usize;
    check_decompressed_len(len)?;
    Ok((len, &input[LEN_PREFIX_SIZE..]))
}

fn check_decompressed_len(len: usize) -> Result<()> {
    if len as u64 > MAX_BLOCK_SIZE {
        return Err(Error::Sstable(format!(
            "decompressed block size {len} exceeds maximum {MAX_BLOCK_SIZE}"
        )));
    }
    Ok(())
}

fn reject_oversized(input: &[u8]) -> Result<()> {
    if input.len() > u32::MAX as usize {
        return Err(Error::InvalidArgument(format!(
            "block of {} bytes is too large to compress",
            input.len()
        )));
    }
    Ok(())
}

/// LZ4 block codec (default acceleration).  The engine default: decompression
/// speed matters on every read, and LZ4 is the fastest codec we support.
#[derive(Debug, Default, Clone, Copy)]
pub struct Lz4Codec;

impl CompressionCodec for Lz4Codec {
    fn ty(&self) -> CompressionType {
        CompressionType::Lz4
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)> {
        reject_oversized(input)?;
        let payload = lz4::block::compress(input, None, false)
            .map_err(|e| Error::Sstable(format!("lz4 compress: {e}")))?;
        if !worth_storing_compressed(input.len(), LEN_PREFIX_SIZE + payload.len()) {
            return Ok((input.to_vec(), CompressionType::None));
        }
        Ok((prepend_len(input.len(), payload), CompressionType::Lz4))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        let (len, payload) = read_len_prefix(input)?;
        let out = lz4::block::decompress(payload, Some(len as i32))
            .map_err(|e| Error::Sstable(format!("lz4 decompress: {e}")))?;
        if out.len() != len {
            return Err(Error::Sstable(format!(
                "lz4 decompressed length {} does not match prefix {len}",
                out.len()
            )));
        }
        Ok(Bytes::from(out))
    }
}

/// ZSTD codec.  Used for the bottommost level, where blocks are read rarely
/// and the better compression ratio pays for the slower decompression.
#[derive(Debug, Clone, Copy)]
pub struct ZstdCodec {
    /// Compression level; zstd's own default is 3.
    pub level: i32,
}

impl Default for ZstdCodec {
    fn default() -> Self {
        Self { level: 3 }
    }
}

impl CompressionCodec for ZstdCodec {
    fn ty(&self) -> CompressionType {
        CompressionType::Zstd
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)> {
        reject_oversized(input)?;
        let payload = zstd::bulk::compress(input, self.level)
            .map_err(|e| Error::Sstable(format!("zstd compress: {e}")))?;
        if !worth_storing_compressed(input.len(), LEN_PREFIX_SIZE + payload.len()) {
            return Ok((input.to_vec(), CompressionType::None));
        }
        Ok((prepend_len(input.len(), payload), CompressionType::Zstd))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        let (len, payload) = read_len_prefix(input)?;
        // `decompress` pre-allocates exactly `len` bytes and fails if the
        // frame expands beyond it, so the allocation is always bounded by the
        // checked prefix.  The level is irrelevant for decompression.
        let out = zstd::bulk::decompress(payload, len)
            .map_err(|e| Error::Sstable(format!("zstd decompress: {e}")))?;
        if out.len() != len {
            return Err(Error::Sstable(format!(
                "zstd decompressed length {} does not match prefix {len}",
                out.len()
            )));
        }
        Ok(Bytes::from(out))
    }
}

/// Snappy block codec (the classic LevelDB codec).
#[derive(Debug, Default, Clone, Copy)]
pub struct SnappyCodec;

impl CompressionCodec for SnappyCodec {
    fn ty(&self) -> CompressionType {
        CompressionType::Snappy
    }

    fn encode(&self, input: &[u8]) -> Result<(Vec<u8>, CompressionType)> {
        reject_oversized(input)?;
        let payload = snap::raw::Encoder::new()
            .compress_vec(input)
            .map_err(|e| Error::Sstable(format!("snappy compress: {e}")))?;
        if !worth_storing_compressed(input.len(), payload.len()) {
            return Ok((input.to_vec(), CompressionType::None));
        }
        Ok((payload, CompressionType::Snappy))
    }

    fn decode(&self, input: &[u8]) -> Result<Bytes> {
        // The raw snappy format embeds the uncompressed length in its header;
        // check it before decoding so a corrupt header cannot cause an
        // oversized allocation.
        let len = snap::raw::decompress_len(input)
            .map_err(|e| Error::Sstable(format!("snappy header: {e}")))?;
        check_decompressed_len(len)?;
        let out = snap::raw::Decoder::new()
            .decompress_vec(input)
            .map_err(|e| Error::Sstable(format!("snappy decompress: {e}")))?;
        debug_assert_eq!(out.len(), len);
        Ok(Bytes::from(out))
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
