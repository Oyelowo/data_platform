//! Serialized on-disk page representation.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use crc32c::crc32c;

use crate::error::{Error, Result};

/// Current on-disk format version.
pub(crate) const PAGE_FORMAT_VERSION: u16 = 1;

/// Magic bytes at the start of every page.
pub(crate) const PAGE_MAGIC: u32 = 0x42_54_52_45; // "BTRE"

/// Unique identifier for a page in the page file.
pub(crate) type PageId = u64;

/// Sentinel value meaning "no page".
pub(crate) const NULL_PAGE_ID: PageId = 0;

/// On-disk page types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum PageType {
    /// Leaf page containing sorted key/value entries.
    Leaf = 1,
    /// Internal page containing sorted separator keys and child page ids.
    Internal = 2,
    /// Overflow page holding a fragment of a large value.
    Overflow = 3,
}

impl PageType {
    pub(crate) fn encode(self) -> u8 {
        self as u8
    }

    fn decode(byte: u8) -> Result<Self> {
        match byte {
            1 => Ok(Self::Leaf),
            2 => Ok(Self::Internal),
            3 => Ok(Self::Overflow),
            _ => Err(Error::Corruption(format!("unknown page type {byte}"))),
        }
    }
}

/// Header stored at the start of every serialized page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageHeader {
    pub page_type: PageType,
    pub version: u16,
    pub entry_count: u16,
    pub free_space: u16,
    pub checksum: u32,
}

impl PageHeader {
    /// Number of bytes used by the fixed header fields.
    ///
    /// The checksum itself is stored in the last four bytes of the page, after
    /// the payload, so it is not counted here.
    pub const SIZE: usize = 4 + 2 + 1 + 1 + 2 + 2;

    fn decode(src: &mut &[u8]) -> Result<Self> {
        if src.len() < Self::SIZE + 4 {
            return Err(Error::Corruption("page header truncated".into()));
        }
        let magic = src.get_u32_le();
        if magic != PAGE_MAGIC {
            return Err(Error::Corruption(format!(
                "page magic mismatch: expected {PAGE_MAGIC:#x}, got {magic:#x}"
            )));
        }
        let version = src.get_u16_le();
        if version != PAGE_FORMAT_VERSION {
            return Err(Error::Corruption(format!(
                "unsupported page format version {version}"
            )));
        }
        let page_type = PageType::decode(src.get_u8())?;
        let _flags = src.get_u8();
        let entry_count = src.get_u16_le();
        let free_space = src.get_u16_le();
        // The checksum is stored in the last four bytes of the page, after the
        // payload, so we read it relative to the end of the buffer.
        let checksum = u32::from_le_bytes([
            src[src.len() - 4],
            src[src.len() - 3],
            src[src.len() - 2],
            src[src.len() - 1],
        ]);
        Ok(Self {
            page_type,
            version,
            entry_count,
            free_space,
            checksum,
        })
    }
}

/// A serialized page ready to be written to disk or read from cache.
#[derive(Clone, Debug)]
pub(crate) struct Page {
    pub id: PageId,
    pub data: Bytes,
}

impl Page {
    /// Create a page from raw bytes, validating the header and checksum.
    pub fn from_bytes(id: PageId, data: Bytes, page_size: usize) -> Result<Self> {
        if data.len() != page_size {
            return Err(Error::Corruption(format!(
                "page {id} size mismatch: expected {page_size}, got {}",
                data.len()
            )));
        }
        let mut view = &data[..];
        let header = PageHeader::decode(&mut view)?;

        // The checksum covers everything except the trailing checksum field.
        let mut body = bytes::BytesMut::with_capacity(data.len() - 4);
        body.extend_from_slice(&data[..data.len() - 4]);
        let expected = crc32c(&body);
        if expected != header.checksum {
            let got = header.checksum;
            return Err(Error::Corruption(format!(
                "page {id} checksum mismatch: expected {expected:#x}, got {got:#x}"
            )));
        }

        Ok(Self { id, data })
    }

    /// Return the page header.
    pub fn header(&self) -> Result<PageHeader> {
        let mut view = &self.data[..];
        PageHeader::decode(&mut view)
    }

    /// Build a serialized page from a body buffer.
    ///
    /// `body` must contain the header fields except the checksum and must be
    /// exactly `page_size - 4` bytes long.
    pub fn build(id: PageId, mut body: BytesMut, page_size: usize) -> Result<Self> {
        if body.len() + 4 != page_size {
            return Err(Error::Corruption(format!(
                "page body size mismatch: body {} + checksum 4 != {page_size}",
                body.len()
            )));
        }
        let checksum = crc32c(&body);
        body.put_u32_le(checksum);
        Ok(Self {
            id,
            data: body.freeze(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_page_roundtrip() {
        let mut body = BytesMut::with_capacity(4092);
        body.resize(4092, 0);
        body[0..4].copy_from_slice(&PAGE_MAGIC.to_le_bytes());
        body[4..6].copy_from_slice(&PAGE_FORMAT_VERSION.to_le_bytes());
        body[6] = PageType::Leaf.encode();
        body[8..10].copy_from_slice(&3u16.to_le_bytes());
        body[10..12].copy_from_slice(&100u16.to_le_bytes());

        let page = Page::build(7, body, 4096).unwrap();
        assert_eq!(page.id, 7);
        assert_eq!(page.data.len(), 4096);

        let decoded = Page::from_bytes(7, page.data.clone(), 4096).unwrap();
        let header = decoded.header().unwrap();
        assert_eq!(header.page_type, PageType::Leaf);
        assert_eq!(header.entry_count, 3);
        assert_eq!(header.free_space, 100);
    }

    #[test]
    fn checksum_failure_detected() {
        let mut body = BytesMut::with_capacity(4092);
        body.resize(4092, 0);
        body[0..4].copy_from_slice(&PAGE_MAGIC.to_le_bytes());
        body[4..6].copy_from_slice(&PAGE_FORMAT_VERSION.to_le_bytes());
        body[6] = PageType::Leaf.encode();
        let page = Page::build(1, body, 4096).unwrap();

        let mut corrupted = page.data.to_vec();
        corrupted[20] ^= 0xFF;
        let result = Page::from_bytes(1, Bytes::from(corrupted), 4096);
        assert!(result.is_err());
    }

    #[test]
    fn version_mismatch_detected() {
        let mut body = BytesMut::with_capacity(4092);
        body.resize(4092, 0);
        body[0..4].copy_from_slice(&PAGE_MAGIC.to_le_bytes());
        body[4..6].copy_from_slice(&999u16.to_le_bytes());
        body[6] = PageType::Leaf.encode();
        let page = Page::build(1, body, 4096).unwrap();
        let result = Page::from_bytes(1, page.data, 4096);
        assert!(result.is_err());
    }
}
