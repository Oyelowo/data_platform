//! Slotted-page cell layout and encoding.
//!
//! A slotted page stores an ordered slot directory at the front and
//! variable-length cells growing backward from the end of the page.  The slot
//! directory gives the offset and length of each cell, so inserting or deleting
//! a record only needs to shift slot pointers, not cell bytes.

use crate::error::{Error, Result};

/// Size of one slot-directory entry in bytes: `offset: u16` + `len: u16`.
pub const SLOT_SIZE: usize = 4;

/// Value-kind tag for an inline value stored inside the cell.
pub const VALUE_KIND_INLINE: u8 = 0;
/// Value-kind tag for a reference into the separate value log.
pub const VALUE_KIND_VALUE_LOG: u8 = 1;
/// Value-kind tag for a deleted-record tombstone.
pub const VALUE_KIND_TOMBSTONE: u8 = 2;
/// Bit set in the value-kind byte when the cell carries an MVCC header.
pub const VALUE_KIND_MVCC_FLAG: u8 = 0x80;

/// One entry in the slot directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    /// Byte offset of the cell within the page.
    pub offset: u16,
    /// Byte length of the cell.
    pub len: u16,
}

impl Slot {
    /// Encode a slot as 4 little-endian bytes.
    pub fn encode(&self) -> [u8; SLOT_SIZE] {
        let mut buf = [0u8; SLOT_SIZE];
        buf[0..2].copy_from_slice(&self.offset.to_le_bytes());
        buf[2..4].copy_from_slice(&self.len.to_le_bytes());
        buf
    }

    /// Decode a slot from 4 little-endian bytes.
    pub fn decode(buf: &[u8]) -> Self {
        Self {
            offset: u16::from_le_bytes([buf[0], buf[1]]),
            len: u16::from_le_bytes([buf[2], buf[3]]),
        }
    }

    /// True if this slot is unused (a deleted or unallocated entry).
    pub fn is_deleted(&self) -> bool {
        self.offset == 0 && self.len == 0
    }

    /// Sentinel representing a deleted slot.
    pub fn deleted() -> Self {
        Self { offset: 0, len: 0 }
    }
}

/// The value half of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind<'a> {
    /// Small value stored inline in the cell.
    Inline(&'a [u8]),
    /// Large value stored in the separate value log at `(offset, len)`.
    ValueLog { offset: u64, len: u32 },
    /// Deleted-record tombstone.
    Tombstone,
}

/// An owned version of [`ValueKind`] that does not borrow a page buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedValue {
    /// Small value stored inline in the cell.
    Inline(Vec<u8>),
    /// Large value stored in the separate value log at `(offset, len)`.
    ValueLog { offset: u64, len: u32 },
    /// Deleted-record tombstone.
    Tombstone,
}

impl OwnedValue {
    /// Return a borrowed [`ValueKind`] view of this owned value.
    pub fn as_value_kind(&self) -> ValueKind<'_> {
        match self {
            OwnedValue::Inline(v) => ValueKind::Inline(v),
            OwnedValue::ValueLog { offset, len } => ValueKind::ValueLog {
                offset: *offset,
                len: *len,
            },
            OwnedValue::Tombstone => ValueKind::Tombstone,
        }
    }
}

impl ValueKind<'_> {
    /// Convert to an owned value.
    pub fn into_owned(self) -> OwnedValue {
        match self {
            ValueKind::Inline(v) => OwnedValue::Inline(v.to_vec()),
            ValueKind::ValueLog { offset, len } => OwnedValue::ValueLog { offset, len },
            ValueKind::Tombstone => OwnedValue::Tombstone,
        }
    }

    /// Serialized size of the value *payload* (excluding the key length and
    /// value-kind tag, but including inline value length prefixes and log
    /// references).
    pub fn payload_size(&self) -> usize {
        match self {
            ValueKind::Inline(v) => 1 + 4 + v.len(),
            ValueKind::ValueLog { .. } => 1 + 8 + 4,
            ValueKind::Tombstone => 1,
        }
    }

    /// Return the value-kind tag byte.
    pub fn tag(&self) -> u8 {
        match self {
            ValueKind::Inline(_) => VALUE_KIND_INLINE,
            ValueKind::ValueLog { .. } => VALUE_KIND_VALUE_LOG,
            ValueKind::Tombstone => VALUE_KIND_TOMBSTONE,
        }
    }
}

/// A parsed cell exposing references into the page buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell<'a> {
    /// The key bytes.
    pub key: &'a [u8],
    /// The value or tombstone.
    pub value: ValueKind<'a>,
    /// Optional MVCC metadata.  `None` means the cell is an autocommit value
    /// with no version history.
    pub mvcc: Option<crate::version::MvccHeader>,
}

/// An owned cell that does not borrow the page buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedCell {
    /// The key bytes.
    pub key: Vec<u8>,
    /// The value or tombstone.
    pub value: OwnedValue,
    /// Optional MVCC metadata.
    pub mvcc: Option<crate::version::MvccHeader>,
}

impl OwnedCell {
    /// Return a borrowed [`Cell`] view of this owned cell.
    pub fn as_cell(&self) -> Cell<'_> {
        Cell {
            key: &self.key,
            value: self.value.as_value_kind(),
            mvcc: self.mvcc,
        }
    }
}

/// Total serialized size of a cell with the given key and value.
#[cfg(test)]
pub fn cell_size(key_len: usize, value: &ValueKind<'_>) -> usize {
    cell_size_with_mvcc(key_len, value, None)
}

/// Total serialized size of a cell that may carry an MVCC header.
pub fn cell_size_with_mvcc(
    key_len: usize,
    value: &ValueKind<'_>,
    mvcc: Option<&crate::version::MvccHeader>,
) -> usize {
    2 + key_len + value.payload_size() + mvcc.map_or(0, |_| crate::version::MvccHeader::SIZE)
}

/// Serialize a cell into `buf`, which must be exactly `cell_size(key, value)`
/// bytes long.
#[cfg(test)]
pub fn write_cell(buf: &mut [u8], key: &[u8], value: &ValueKind<'_>) -> Result<()> {
    write_cell_with_mvcc(buf, key, value, None)
}

/// Serialize a cell that may carry MVCC metadata into `buf`.
pub fn write_cell_with_mvcc(
    buf: &mut [u8],
    key: &[u8],
    value: &ValueKind<'_>,
    mvcc: Option<&crate::version::MvccHeader>,
) -> Result<()> {
    let expected = cell_size_with_mvcc(key.len(), value, mvcc);
    if buf.len() != expected {
        return Err(Error::Corruption(format!(
            "cell buffer size mismatch: expected {expected}, got {}",
            buf.len()
        )));
    }
    if key.len() > u16::MAX as usize {
        return Err(Error::OutOfBounds {
            kind: crate::error::BoundKind::Key,
            limit: u16::MAX as usize,
            got: key.len(),
        });
    }

    let mut off = 0;
    buf[off..off + 2].copy_from_slice(&(key.len() as u16).to_le_bytes());
    off += 2;
    let kind_byte = value.tag() | mvcc.map_or(0, |_| VALUE_KIND_MVCC_FLAG);
    buf[off] = kind_byte;
    off += 1;

    if let Some(header) = mvcc {
        header.encode(&mut buf[off..off + crate::version::MvccHeader::SIZE])?;
        off += crate::version::MvccHeader::SIZE;
    }

    match value {
        ValueKind::Inline(v) => {
            if v.len() > u32::MAX as usize {
                return Err(Error::OutOfBounds {
                    kind: crate::error::BoundKind::Value,
                    limit: u32::MAX as usize,
                    got: v.len(),
                });
            }
            buf[off..off + 4].copy_from_slice(&(v.len() as u32).to_le_bytes());
            off += 4;
            buf[off..off + v.len()].copy_from_slice(v);
            off += v.len();
        }
        ValueKind::ValueLog { offset, len } => {
            buf[off..off + 8].copy_from_slice(&offset.to_le_bytes());
            off += 8;
            buf[off..off + 4].copy_from_slice(&len.to_le_bytes());
            off += 4;
        }
        ValueKind::Tombstone => {}
    }

    buf[off..off + key.len()].copy_from_slice(key);
    off += key.len();

    debug_assert_eq!(off, expected);
    Ok(())
}

/// Parse a cell from `buf`.
pub fn parse_cell(buf: &[u8]) -> Result<Cell<'_>> {
    if buf.len() < 3 {
        return Err(Error::Corruption("cell too short for key length".into()));
    }
    let key_len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    let kind_byte = buf[2];
    let has_mvcc = (kind_byte & VALUE_KIND_MVCC_FLAG) != 0;
    let kind = kind_byte & !VALUE_KIND_MVCC_FLAG;
    let mut off = 3;

    let mvcc = if has_mvcc {
        if buf.len() < off + crate::version::MvccHeader::SIZE {
            return Err(Error::Corruption("cell MVCC header truncated".into()));
        }
        let header = crate::version::MvccHeader::decode(&buf[off..])?;
        off += crate::version::MvccHeader::SIZE;
        Some(header)
    } else {
        None
    };

    let value = match kind {
        VALUE_KIND_INLINE => {
            if buf.len() < off + 4 {
                return Err(Error::Corruption("inline value length truncated".into()));
            }
            let val_len =
                u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as usize;
            off += 4;
            if buf.len() < off + val_len {
                return Err(Error::Corruption("inline value truncated".into()));
            }
            let value = &buf[off..off + val_len];
            off += val_len;
            ValueKind::Inline(value)
        }
        VALUE_KIND_VALUE_LOG => {
            if buf.len() < off + 12 {
                return Err(Error::Corruption("value-log reference truncated".into()));
            }
            let offset = u64::from_le_bytes([
                buf[off],
                buf[off + 1],
                buf[off + 2],
                buf[off + 3],
                buf[off + 4],
                buf[off + 5],
                buf[off + 6],
                buf[off + 7],
            ]);
            let len =
                u32::from_le_bytes([buf[off + 8], buf[off + 9], buf[off + 10], buf[off + 11]]);
            off += 12;
            ValueKind::ValueLog { offset, len }
        }
        VALUE_KIND_TOMBSTONE => ValueKind::Tombstone,
        _ => return Err(Error::Corruption(format!("unknown cell value kind {kind}"))),
    };

    if buf.len() < off + key_len {
        return Err(Error::Corruption("cell key truncated".into()));
    }
    let key = &buf[off..off + key_len];
    Ok(Cell { key, value, mvcc })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_cell_roundtrip() {
        let key = b"hello";
        let value = ValueKind::Inline(b"world");
        let size = cell_size(key.len(), &value);
        let mut buf = vec![0u8; size];
        write_cell(&mut buf, key, &value).unwrap();
        let parsed = parse_cell(&buf).unwrap();
        assert_eq!(parsed.key, key);
        assert_eq!(parsed.value, value);
    }

    #[test]
    fn value_log_cell_roundtrip() {
        let key = b"bigkey";
        let value = ValueKind::ValueLog {
            offset: 0x1234_5678_9abc_def0,
            len: 42,
        };
        let size = cell_size(key.len(), &value);
        let mut buf = vec![0u8; size];
        write_cell(&mut buf, key, &value).unwrap();
        let parsed = parse_cell(&buf).unwrap();
        assert_eq!(parsed.key, key);
        assert_eq!(parsed.value, value);
    }

    #[test]
    fn tombstone_cell_roundtrip() {
        let key = b"deleted";
        let value = ValueKind::Tombstone;
        let size = cell_size(key.len(), &value);
        let mut buf = vec![0u8; size];
        write_cell(&mut buf, key, &value).unwrap();
        let parsed = parse_cell(&buf).unwrap();
        assert_eq!(parsed.key, key);
        assert_eq!(parsed.value, value);
    }

    #[test]
    fn truncated_cell_rejected() {
        let buf = [5u8, 0, VALUE_KIND_INLINE, 0, 0]; // missing value length
        assert!(parse_cell(&buf).is_err());
    }

    #[test]
    fn unknown_value_kind_rejected() {
        let mut buf = vec![1u8, 0, 255, 0, b"k"[0]];
        buf.resize(cell_size(1, &ValueKind::Tombstone), 0);
        assert!(parse_cell(&buf).is_err());
    }

    #[test]
    fn mvcc_cell_roundtrip() {
        use crate::version::MvccHeader;
        let key = b"mvcc";
        let value = ValueKind::Inline(b"data");
        let header = MvccHeader {
            begin_ts: 5,
            end_ts: 7,
            prev_version_lsn: 123,
        };
        let size = cell_size_with_mvcc(key.len(), &value, Some(&header));
        let mut buf = vec![0u8; size];
        write_cell_with_mvcc(&mut buf, key, &value, Some(&header)).unwrap();
        let parsed = parse_cell(&buf).unwrap();
        assert_eq!(parsed.key, key);
        assert_eq!(parsed.value, value);
        assert_eq!(parsed.mvcc, Some(header));
    }
}
