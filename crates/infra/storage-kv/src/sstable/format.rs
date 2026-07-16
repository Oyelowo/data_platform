//! Low-level SSTable format constants and helpers.

use bytes::{Buf, BufMut};

/// Magic number at the end of every SSTable footer.
pub const TABLE_MAGIC: u64 = 0x53_54_41_42_4C_45_30_30;

/// Footer size in bytes.
pub const FOOTER_SIZE: usize = 48;

/// Block trailer size: compression type (1) + CRC32C (4).
pub const BLOCK_TRAILER_SIZE: usize = 5;

/// Compression type stored in block trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionType {
    /// No compression.
    None = 0,
}

impl CompressionType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(CompressionType::None),
            _ => None,
        }
    }
}

/// A handle to a block within an SSTable file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockHandle {
    pub offset: u64,
    pub size: u64,
}

impl BlockHandle {
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.put_u64_le(self.offset);
        buf.put_u64_le(self.size);
    }

    pub fn decode(buf: &[u8]) -> (Self, usize) {
        let mut cursor = buf;
        let offset = cursor.get_u64_le();
        let size = cursor.get_u64_le();
        (Self { offset, size }, 16)
    }
}

/// SSTable footer written at the end of the file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Footer {
    pub metaindex_handle: BlockHandle,
    pub index_handle: BlockHandle,
}

impl Footer {
    pub fn encode(&self, buf: &mut Vec<u8>) {
        self.metaindex_handle.encode(buf);
        self.index_handle.encode(buf);
        buf.put_u64_le(0); // padding / version
        buf.put_u64_le(TABLE_MAGIC);
    }

    pub fn decode(buf: &[u8]) -> crate::Result<Self> {
        if buf.len() < FOOTER_SIZE {
            return Err(crate::Error::Sstable("truncated footer".into()));
        }
        let (metaindex_handle, consumed1) = BlockHandle::decode(buf);
        let (index_handle, _consumed2) = BlockHandle::decode(&buf[consumed1..]);
        let mut cursor = &buf[32..];
        let _version = cursor.get_u64_le();
        let magic = cursor.get_u64_le();
        if magic != TABLE_MAGIC {
            return Err(crate::Error::Sstable("bad table magic".into()));
        }
        Ok(Self {
            metaindex_handle,
            index_handle,
        })
    }
}

/// Compute the CRC32C checksum for a byte slice.
pub fn checksum(data: &[u8]) -> u32 {
    crc32c::crc32c(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_roundtrip() {
        let footer = Footer {
            metaindex_handle: BlockHandle { offset: 100, size: 200 },
            index_handle: BlockHandle { offset: 300, size: 400 },
        };
        let mut buf = Vec::new();
        footer.encode(&mut buf);
        let decoded = Footer::decode(&buf).unwrap();
        assert_eq!(decoded, footer);
    }
}
