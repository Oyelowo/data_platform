//! Physiological write-ahead log records for the in-place B+ tree.
//!
//! These records are wrapped by `storage-wal`'s segment framing and describe
//! page-level changes in logical terms (cells, splits, merges) so that redo is
//! page-local and idempotent.  See `PHASE5_DESIGN.md` for the full recovery
//! protocol.

use parking_lot::Mutex as ParkingMutex;

use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::page::PageId;
use crate::slot::{OwnedCell, OwnedValue, parse_cell};
use crate::txn::{NULL_TXN_ID, Timestamp, TxnId};

/// Log sequence number.
///
/// LSNs are opaque 64-bit values that increase monotonically within a single
/// WAL.  `0` is reserved as `NULL_LSN` and means "no LSN".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Lsn(pub u64);

/// Sentinel LSN meaning "none".
pub const NULL_LSN: Lsn = Lsn(0);

impl Lsn {
    /// Create an LSN from its raw 64-bit value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Return the raw 64-bit value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Encode as little-endian bytes.
    pub const fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    /// Decode from little-endian bytes.
    pub const fn from_le_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
    }
}

impl std::fmt::Display for Lsn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Type tag for a physiological WAL record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordType {
    /// Insert a single cell into a leaf page.
    InsertCell = 1,
    /// Delete a single cell from a leaf page.
    DeleteCell = 2,
    /// Overwrite an existing cell in a leaf page.
    UpdateCell = 3,
    /// Split a leaf or internal page; the new right sibling is created.
    SplitPage = 4,
    /// Merge two pages; the victim page is retired.
    MergePage = 5,
    /// Update the rightmost child pointer of an internal page (root shrink).
    MoveRightmost = 6,
    /// Install a new root page pointer (used for root shrink and for final
    /// root installation after a root split).
    SetRoot = 7,
    /// Create and populate a brand-new root page during a root split.
    NewRoot = 8,
    /// Transaction begin marker.
    Begin = 9,
    /// Transaction commit marker, carrying the commit timestamp.
    Commit = 10,
    /// Transaction abort marker.
    Abort = 11,
    /// Compensation log record written during undo.
    Clr = 12,
}

impl RecordType {
    fn from_u8(value: u8) -> Result<Self> {
        match value {
            1 => Ok(RecordType::InsertCell),
            2 => Ok(RecordType::DeleteCell),
            3 => Ok(RecordType::UpdateCell),
            4 => Ok(RecordType::SplitPage),
            5 => Ok(RecordType::MergePage),
            6 => Ok(RecordType::MoveRightmost),
            7 => Ok(RecordType::SetRoot),
            8 => Ok(RecordType::NewRoot),
            9 => Ok(RecordType::Begin),
            10 => Ok(RecordType::Commit),
            11 => Ok(RecordType::Abort),
            12 => Ok(RecordType::Clr),
            other => Err(Error::Corruption(format!(
                "unknown WAL record type {other}"
            ))),
        }
    }
}

/// Common header shared by every physiological WAL record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordHeader {
    pub record_type: RecordType,
    pub transaction_id: TxnId,
    pub prev_lsn: Lsn,
    pub page_id: PageId,
    /// Page LSN *before* this record was applied.
    pub page_lsn: Lsn,
}

impl RecordHeader {
    /// Size of the common header on disk.
    pub const SIZE: usize = 1 + 8 + 8 + 8 + 8;

    pub fn new(
        record_type: RecordType,
        transaction_id: TxnId,
        prev_lsn: Lsn,
        page_id: PageId,
        page_lsn: Lsn,
    ) -> Self {
        Self {
            record_type,
            transaction_id,
            prev_lsn,
            page_id,
            page_lsn,
        }
    }

    fn encode(&self, buf: &mut [u8]) {
        debug_assert!(buf.len() >= Self::SIZE);
        buf[0] = self.record_type as u8;
        buf[1..9].copy_from_slice(&self.transaction_id.to_le_bytes());
        buf[9..17].copy_from_slice(&self.prev_lsn.to_le_bytes());
        buf[17..25].copy_from_slice(&self.page_id.to_le_bytes());
        buf[25..33].copy_from_slice(&self.page_lsn.to_le_bytes());
    }

    fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::SIZE {
            return Err(Error::Corruption("WAL record header truncated".into()));
        }
        Ok(Self {
            record_type: RecordType::from_u8(buf[0])?,
            transaction_id: TxnId::from_le_bytes([
                buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8],
            ]),
            prev_lsn: Lsn::from_le_bytes([
                buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16],
            ]),
            page_id: PageId::from_le_bytes([
                buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23], buf[24],
            ]),
            page_lsn: Lsn::from_le_bytes([
                buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31], buf[32],
            ]),
        })
    }
}

/// Payload of a physiological WAL record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordPayload {
    /// Insert `cell` into `page_id`.
    InsertCell { cell: OwnedCell },
    /// Delete the cell with `key` from `page_id`.  `old_cell`/`old_header` hold
    /// the previous version for undo.
    DeleteCell {
        key: Vec<u8>,
        old_cell: Option<OwnedCell>,
        old_header: Option<crate::version::MvccHeader>,
    },
    /// Overwrite the cell with `key` to `value` in `page_id`.  `old_cell`/
    /// `old_header` hold the previous version for undo.
    UpdateCell {
        cell: OwnedCell,
        old_cell: Option<OwnedCell>,
        old_header: Option<crate::version::MvccHeader>,
    },
    /// Split a page at `separator`; the new right sibling is `right_page_id`.
    SplitPage {
        separator: Vec<u8>,
        right_page_id: PageId,
        is_internal: bool,
    },
    /// Merge two pages.  `victim_page_id` is retired; `victim_is_left` tells
    /// recovery whether the victim was the left or right sibling of the
    /// surviving page.  `separator` and `victim_leftmost` are used for internal
    /// merges; for leaf merges `separator` is empty and `victim_leftmost` is
    /// `NULL_PAGE_ID`.
    MergePage {
        victim_page_id: PageId,
        victim_is_left: bool,
        separator: Vec<u8>,
        victim_leftmost: PageId,
    },
    /// Replace the rightmost child pointer of an internal page.
    MoveRightmost {
        old_rightmost: PageId,
        new_rightmost: PageId,
    },
    /// Install `new_root_page_id` as the tree root.
    SetRoot { new_root_page_id: PageId },
    /// Create and populate a brand-new root page during a root split.
    NewRoot {
        new_root_page_id: PageId,
        leftmost_child: PageId,
        separator: Vec<u8>,
        right_child: PageId,
    },
    /// Transaction begin marker.
    Begin,
    /// Transaction commit marker carrying the commit timestamp.
    Commit { commit_ts: crate::txn::Timestamp },
    /// Transaction abort marker.
    Abort,
    /// Compensation log record.  `undo_next_lsn` points to the next older record
    /// that still needs undoing for this transaction; `original` is the record
    /// being compensated so the CLR itself can be redone idempotently.
    Clr {
        undo_next_lsn: Lsn,
        original: Box<Record>,
    },
}

/// A fully decoded physiological WAL record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub header: RecordHeader,
    pub payload: RecordPayload,
}

impl Record {
    /// Total on-wire size of this record when encoded (including header).
    pub fn encoded_size(&self) -> usize {
        RecordHeader::SIZE + self.payload_encoded_size()
    }

    /// Size of the payload portion only.
    fn payload_encoded_size(&self) -> usize {
        match &self.payload {
            RecordPayload::InsertCell { cell } => {
                2 + cell_size_with_mvcc(cell.key.len(), &cell.value, cell.mvcc.as_ref())
            }
            RecordPayload::UpdateCell {
                cell,
                old_cell,
                old_header,
            } => {
                2 + cell_size_with_mvcc(cell.key.len(), &cell.value, cell.mvcc.as_ref())
                    + optional_cell_size(old_cell.as_ref())
                    + optional_header_size(old_header.as_ref())
            }
            RecordPayload::DeleteCell {
                key,
                old_cell,
                old_header,
            } => {
                2 + key.len()
                    + optional_cell_size(old_cell.as_ref())
                    + optional_header_size(old_header.as_ref())
            }
            RecordPayload::SplitPage {
                separator,
                right_page_id: _,
                is_internal: _,
            } => 2 + separator.len() + 8 + 1,
            RecordPayload::MergePage {
                victim_page_id: _,
                victim_is_left: _,
                separator,
                victim_leftmost: _,
            } => 8 + 1 + 2 + separator.len() + 8,
            RecordPayload::MoveRightmost {
                old_rightmost: _,
                new_rightmost: _,
            } => 8 + 8,
            RecordPayload::SetRoot {
                new_root_page_id: _,
            } => 8,
            RecordPayload::NewRoot {
                separator,
                new_root_page_id: _,
                leftmost_child: _,
                right_child: _,
            } => 8 + 8 + 2 + separator.len() + 8,
            RecordPayload::Begin | RecordPayload::Abort => 0,
            RecordPayload::Commit { .. } => 8,
            RecordPayload::Clr {
                undo_next_lsn: _,
                original,
            } => 8 + original.encoded_size(),
        }
    }

    /// Encode this record into `buf`.  `buf` must be at least
    /// `self.encoded_size()` bytes long.
    pub fn encode(&self, buf: &mut [u8]) -> Result<()> {
        let expected = self.encoded_size();
        if buf.len() < expected {
            return Err(Error::Corruption(format!(
                "WAL record encode buffer too small: expected {expected}, got {}",
                buf.len()
            )));
        }
        self.header.encode(buf);
        let mut off = RecordHeader::SIZE;
        match &self.payload {
            RecordPayload::InsertCell { cell } => {
                let cell_bytes = encode_owned_cell(cell)?;
                put_u16(&mut buf[off..], cell_bytes.len() as u16)?;
                off += 2;
                buf[off..off + cell_bytes.len()].copy_from_slice(&cell_bytes);
                off += cell_bytes.len();
            }
            RecordPayload::UpdateCell {
                cell,
                old_cell,
                old_header,
            } => {
                let cell_bytes = encode_owned_cell(cell)?;
                put_u16(&mut buf[off..], cell_bytes.len() as u16)?;
                off += 2;
                buf[off..off + cell_bytes.len()].copy_from_slice(&cell_bytes);
                off += cell_bytes.len();
                encode_optional_cell(buf, &mut off, old_cell.as_ref())?;
                encode_optional_header(buf, &mut off, old_header.as_ref())?;
            }
            RecordPayload::DeleteCell {
                key,
                old_cell,
                old_header,
            } => {
                put_u16(&mut buf[off..], key.len() as u16)?;
                off += 2;
                buf[off..off + key.len()].copy_from_slice(key);
                off += key.len();
                encode_optional_cell(buf, &mut off, old_cell.as_ref())?;
                encode_optional_header(buf, &mut off, old_header.as_ref())?;
            }
            RecordPayload::SplitPage {
                separator,
                right_page_id,
                is_internal,
            } => {
                put_u16(&mut buf[off..], separator.len() as u16)?;
                off += 2;
                buf[off..off + separator.len()].copy_from_slice(separator);
                off += separator.len();
                buf[off..off + 8].copy_from_slice(&right_page_id.to_le_bytes());
                off += 8;
                buf[off] = *is_internal as u8;
                off += 1;
            }
            RecordPayload::MergePage {
                victim_page_id,
                victim_is_left,
                separator,
                victim_leftmost,
            } => {
                buf[off..off + 8].copy_from_slice(&victim_page_id.to_le_bytes());
                off += 8;
                buf[off] = *victim_is_left as u8;
                off += 1;
                put_u16(&mut buf[off..], separator.len() as u16)?;
                off += 2;
                buf[off..off + separator.len()].copy_from_slice(separator);
                off += separator.len();
                buf[off..off + 8].copy_from_slice(&victim_leftmost.to_le_bytes());
                off += 8;
            }
            RecordPayload::MoveRightmost {
                old_rightmost,
                new_rightmost,
            } => {
                buf[off..off + 8].copy_from_slice(&old_rightmost.to_le_bytes());
                off += 8;
                buf[off..off + 8].copy_from_slice(&new_rightmost.to_le_bytes());
                off += 8;
            }
            RecordPayload::SetRoot { new_root_page_id } => {
                buf[off..off + 8].copy_from_slice(&new_root_page_id.to_le_bytes());
                off += 8;
            }
            RecordPayload::NewRoot {
                new_root_page_id,
                leftmost_child,
                separator,
                right_child,
            } => {
                buf[off..off + 8].copy_from_slice(&new_root_page_id.to_le_bytes());
                off += 8;
                buf[off..off + 8].copy_from_slice(&leftmost_child.to_le_bytes());
                off += 8;
                put_u16(&mut buf[off..], separator.len() as u16)?;
                off += 2;
                buf[off..off + separator.len()].copy_from_slice(separator);
                off += separator.len();
                buf[off..off + 8].copy_from_slice(&right_child.to_le_bytes());
                off += 8;
            }
            RecordPayload::Begin | RecordPayload::Abort => {}
            RecordPayload::Commit { commit_ts } => {
                buf[off..off + 8].copy_from_slice(&commit_ts.to_le_bytes());
                off += 8;
            }
            RecordPayload::Clr {
                undo_next_lsn,
                original,
            } => {
                buf[off..off + 8].copy_from_slice(&undo_next_lsn.to_le_bytes());
                off += 8;
                let inner_len = original.encoded_size();
                buf[off..off + inner_len].copy_from_slice(&encode_record_to_vec(original)?);
                off += inner_len;
            }
        }
        debug_assert_eq!(off, expected);
        Ok(())
    }

    /// Decode a record from `buf`.  Returns the record and the number of bytes
    /// consumed.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
        let header = RecordHeader::decode(buf)?;
        let mut off = RecordHeader::SIZE;
        let payload = match header.record_type {
            RecordType::InsertCell => {
                let cell_len = read_u16(buf, off)? as usize;
                off += 2;
                if buf.len() < off + cell_len {
                    return Err(Error::Corruption("WAL InsertCell payload truncated".into()));
                }
                let cell = decode_owned_cell(&buf[off..off + cell_len])?;
                off += cell_len;
                RecordPayload::InsertCell { cell }
            }
            RecordType::UpdateCell => {
                let cell_len = read_u16(buf, off)? as usize;
                off += 2;
                if buf.len() < off + cell_len {
                    return Err(Error::Corruption("WAL UpdateCell payload truncated".into()));
                }
                let cell = decode_owned_cell(&buf[off..off + cell_len])?;
                off += cell_len;
                let old_cell = decode_optional_cell(buf, &mut off)?;
                let old_header = decode_optional_header(buf, &mut off)?;
                RecordPayload::UpdateCell {
                    cell,
                    old_cell,
                    old_header,
                }
            }
            RecordType::DeleteCell => {
                let key_len = read_u16(buf, off)? as usize;
                off += 2;
                if buf.len() < off + key_len {
                    return Err(Error::Corruption("WAL DeleteCell payload truncated".into()));
                }
                let key = buf[off..off + key_len].to_vec();
                off += key_len;
                let old_cell = decode_optional_cell(buf, &mut off)?;
                let old_header = decode_optional_header(buf, &mut off)?;
                RecordPayload::DeleteCell {
                    key,
                    old_cell,
                    old_header,
                }
            }
            RecordType::SplitPage => {
                let sep_len = read_u16(buf, off)? as usize;
                off += 2;
                if buf.len() < off + sep_len + 8 + 1 {
                    return Err(Error::Corruption("WAL SplitPage payload truncated".into()));
                }
                let separator = buf[off..off + sep_len].to_vec();
                off += sep_len;
                let right_page_id = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                let is_internal = buf[off] != 0;
                off += 1;
                RecordPayload::SplitPage {
                    separator,
                    right_page_id,
                    is_internal,
                }
            }
            RecordType::MergePage => {
                if buf.len() < off + 8 + 1 + 2 + 8 {
                    return Err(Error::Corruption("WAL MergePage payload truncated".into()));
                }
                let victim_page_id = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                let victim_is_left = buf[off] != 0;
                off += 1;
                let sep_len = read_u16(buf, off)? as usize;
                off += 2;
                if buf.len() < off + sep_len + 8 {
                    return Err(Error::Corruption(
                        "WAL MergePage separator truncated".into(),
                    ));
                }
                let separator = buf[off..off + sep_len].to_vec();
                off += sep_len;
                let victim_leftmost = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                RecordPayload::MergePage {
                    victim_page_id,
                    victim_is_left,
                    separator,
                    victim_leftmost,
                }
            }
            RecordType::MoveRightmost => {
                if buf.len() < off + 8 + 8 {
                    return Err(Error::Corruption(
                        "WAL MoveRightmost payload truncated".into(),
                    ));
                }
                let old_rightmost = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                let new_rightmost = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                RecordPayload::MoveRightmost {
                    old_rightmost,
                    new_rightmost,
                }
            }
            RecordType::SetRoot => {
                if buf.len() < off + 8 {
                    return Err(Error::Corruption("WAL SetRoot payload truncated".into()));
                }
                let new_root_page_id = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                RecordPayload::SetRoot { new_root_page_id }
            }
            RecordType::NewRoot => {
                if buf.len() < off + 8 + 8 + 2 + 8 {
                    return Err(Error::Corruption("WAL NewRoot payload truncated".into()));
                }
                let new_root_page_id = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                let leftmost_child = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                let sep_len = read_u16(buf, off)? as usize;
                off += 2;
                if buf.len() < off + sep_len + 8 {
                    return Err(Error::Corruption("WAL NewRoot separator truncated".into()));
                }
                let separator = buf[off..off + sep_len].to_vec();
                off += sep_len;
                let right_child = PageId::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                RecordPayload::NewRoot {
                    new_root_page_id,
                    leftmost_child,
                    separator,
                    right_child,
                }
            }
            RecordType::Begin => RecordPayload::Begin,
            RecordType::Commit => {
                if buf.len() < off + 8 {
                    return Err(Error::Corruption("WAL Commit payload truncated".into()));
                }
                let commit_ts = Timestamp::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                RecordPayload::Commit { commit_ts }
            }
            RecordType::Abort => RecordPayload::Abort,
            RecordType::Clr => {
                if buf.len() < off + 8 {
                    return Err(Error::Corruption("WAL Clr payload truncated".into()));
                }
                let undo_next_lsn = Lsn::from_le_bytes([
                    buf[off],
                    buf[off + 1],
                    buf[off + 2],
                    buf[off + 3],
                    buf[off + 4],
                    buf[off + 5],
                    buf[off + 6],
                    buf[off + 7],
                ]);
                off += 8;
                let (original, consumed) = Self::decode(&buf[off..])?;
                off += consumed;
                RecordPayload::Clr {
                    undo_next_lsn,
                    original: Box::new(original),
                }
            }
        };
        Ok((Record { header, payload }, off))
    }
}

fn encode_owned_cell(cell: &OwnedCell) -> Result<Vec<u8>> {
    let value_kind = cell.value.as_value_kind();
    let size = crate::slot::cell_size_with_mvcc(cell.key.len(), &value_kind, cell.mvcc.as_ref());
    let mut buf = vec![0u8; size];
    crate::slot::write_cell_with_mvcc(&mut buf, &cell.key, &value_kind, cell.mvcc.as_ref())?;
    Ok(buf)
}

fn decode_owned_cell(buf: &[u8]) -> Result<OwnedCell> {
    let cell = parse_cell(buf)?;
    Ok(OwnedCell {
        key: cell.key.to_vec(),
        value: cell.value.into_owned(),
        mvcc: cell.mvcc,
    })
}

fn optional_cell_size(cell: Option<&OwnedCell>) -> usize {
    match cell {
        None => 1,
        Some(c) => 1 + 2 + encode_owned_cell(c).unwrap_or_default().len(),
    }
}

fn optional_header_size(header: Option<&crate::version::MvccHeader>) -> usize {
    match header {
        None => 1,
        Some(_) => 1 + crate::version::MvccHeader::SIZE,
    }
}

fn encode_optional_cell(buf: &mut [u8], off: &mut usize, cell: Option<&OwnedCell>) -> Result<()> {
    match cell {
        None => {
            buf[*off] = 0;
            *off += 1;
            Ok(())
        }
        Some(c) => {
            buf[*off] = 1;
            *off += 1;
            let bytes = encode_owned_cell(c)?;
            put_u16(&mut buf[*off..], bytes.len() as u16)?;
            *off += 2;
            buf[*off..*off + bytes.len()].copy_from_slice(&bytes);
            *off += bytes.len();
            Ok(())
        }
    }
}

fn decode_optional_cell(buf: &[u8], off: &mut usize) -> Result<Option<OwnedCell>> {
    if buf.len() < *off + 1 {
        return Err(Error::Corruption("WAL optional-cell flag truncated".into()));
    }
    let present = buf[*off];
    *off += 1;
    if present == 0 {
        return Ok(None);
    }
    let len = read_u16(buf, *off)? as usize;
    *off += 2;
    if buf.len() < *off + len {
        return Err(Error::Corruption(
            "WAL optional-cell bytes truncated".into(),
        ));
    }
    let cell = decode_owned_cell(&buf[*off..*off + len])?;
    *off += len;
    Ok(Some(cell))
}

fn encode_optional_header(
    buf: &mut [u8],
    off: &mut usize,
    header: Option<&crate::version::MvccHeader>,
) -> Result<()> {
    match header {
        None => {
            buf[*off] = 0;
            *off += 1;
            Ok(())
        }
        Some(h) => {
            buf[*off] = 1;
            *off += 1;
            h.encode(&mut buf[*off..*off + crate::version::MvccHeader::SIZE])?;
            *off += crate::version::MvccHeader::SIZE;
            Ok(())
        }
    }
}

fn decode_optional_header(
    buf: &[u8],
    off: &mut usize,
) -> Result<Option<crate::version::MvccHeader>> {
    if buf.len() < *off + 1 {
        return Err(Error::Corruption(
            "WAL optional-header flag truncated".into(),
        ));
    }
    let present = buf[*off];
    *off += 1;
    if present == 0 {
        return Ok(None);
    }
    if buf.len() < *off + crate::version::MvccHeader::SIZE {
        return Err(Error::Corruption(
            "WAL optional-header bytes truncated".into(),
        ));
    }
    let header = crate::version::MvccHeader::decode(&buf[*off..])?;
    *off += crate::version::MvccHeader::SIZE;
    Ok(Some(header))
}

fn cell_size_with_mvcc(
    key_len: usize,
    value: &OwnedValue,
    mvcc: Option<&crate::version::MvccHeader>,
) -> usize {
    crate::slot::cell_size_with_mvcc(key_len, &value.as_value_kind(), mvcc)
}

fn put_u16(buf: &mut [u8], value: u16) -> Result<()> {
    if buf.len() < 2 {
        return Err(Error::Corruption("buffer too small for u16".into()));
    }
    buf[0..2].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn read_u16(buf: &[u8], off: usize) -> Result<u16> {
    if buf.len() < off + 2 {
        return Err(Error::Corruption("buffer too short for u16".into()));
    }
    Ok(u16::from_le_bytes([buf[off], buf[off + 1]]))
}

fn encode_record_to_vec(record: &Record) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; record.encoded_size()];
    record.encode(&mut buf)?;
    Ok(buf)
}

/// Adapter that wraps `storage_wal::Wal` and appends physiological v2 records.
pub struct WalLog {
    inner: storage_wal::Wal,
    metrics: ParkingMutex<Option<std::sync::Arc<crate::metrics::Metrics>>>,
    /// Set to true once a sync fails.  Used by the engine layer to avoid
    /// blocking forever in close()/checkpoint() on a WAL whose commit worker
    /// has already exited.
    sync_failed: AtomicBool,
}

impl WalLog {
    /// Open or create a physiological WAL at `<dir>/wal`.
    pub fn open(
        dir: impl AsRef<std::path::Path>,
        options: storage_wal::WalOptions,
    ) -> Result<Self> {
        Self::open_with_fault_config(dir, options, None)
    }

    /// Open or create a physiological WAL with an optional fault-injection
    /// config for durability testing.
    pub fn open_with_fault_config(
        dir: impl AsRef<std::path::Path>,
        options: storage_wal::WalOptions,
        fault_config: Option<storage_wal::FaultConfig>,
    ) -> Result<Self> {
        let wal_dir = dir.as_ref().join("wal");
        let inner = storage_wal::Wal::open_with_fault_config(&wal_dir, options, fault_config)
            .map_err(|e| Error::Io(std::io::Error::other(format!("failed to open WAL: {e}"))))?;
        Ok(Self {
            inner,
            metrics: ParkingMutex::new(None),
            sync_failed: AtomicBool::new(false),
        })
    }

    /// Attach a metrics collector to this WAL.
    pub fn set_metrics(&self, metrics: std::sync::Arc<crate::metrics::Metrics>) {
        *self.metrics.lock() = Some(metrics);
    }

    /// Append a raw physiological record and wait for durability.  Returns the
    /// assigned LSN.
    pub fn append(&self, record: Record) -> Result<Lsn> {
        self.append_with_durability(record, storage_wal::Durability::Immediate)
    }

    /// Append a raw physiological record without waiting for fsync.  Returns
    /// the assigned LSN immediately. The caller must later call `sync` to make
    /// the record durable.
    pub fn append_buffered(&self, record: Record) -> Result<Lsn> {
        self.append_with_durability(record, storage_wal::Durability::Buffered)
    }

    fn append_with_durability(
        &self,
        record: Record,
        durability: storage_wal::Durability,
    ) -> Result<Lsn> {
        let mut payload = vec![0u8; record.encoded_size()];
        record.encode(&mut payload)?;
        let n = payload.len() as u64;
        let result = self
            .inner
            .append_record(
                storage_wal::Record::new(storage_wal::RecordType::Put, bytes::Bytes::from(payload)),
                durability,
            )
            .map_err(|e| Error::Io(std::io::Error::other(format!("WAL append failed: {e}"))))
            .map(Lsn::new);
        if result.is_ok()
            && let Some(m) = self.metrics.lock().as_ref()
        {
            m.inc_wal_bytes(n);
        }
        result
    }

    /// Force a flush of all buffered WAL records.  Blocks until durable.
    pub fn sync(&self) -> Result<()> {
        let start = std::time::Instant::now();
        let result = self
            .inner
            .sync()
            .map_err(|e| Error::Io(std::io::Error::other(format!("WAL sync failed: {e}"))));
        if result.is_ok()
            && let Some(m) = self.metrics.lock().as_ref()
        {
            m.record_wal_sync(start.elapsed().as_nanos() as u64);
        }
        if result.is_err() {
            self.sync_failed.store(true, Ordering::Release);
        }
        result
    }

    /// True if no prior `sync` failed.  Once this returns false the WAL worker
    /// may have exited; callers should avoid further blocking operations.
    pub fn is_healthy(&self) -> bool {
        !self.sync_failed.load(Ordering::Acquire)
    }

    /// Ensure the WAL is durable up to `lsn`.
    ///
    /// The underlying `storage-wal` layer only exposes a global `sync()`, so this
    /// flushes all pending records.  That is safe because LSNs are monotonic and
    /// the record at `lsn` has already been appended before the caller updates the
    /// in-memory `page_lsn`.  Returns the durable LSN.
    pub fn sync_up_to(&self, lsn: Lsn) -> Result<Lsn> {
        if lsn == NULL_LSN {
            return Ok(NULL_LSN);
        }
        self.sync()?;
        Ok(lsn)
    }

    /// Iterate over all physiological records starting from `start_lsn`.
    pub fn iter(&self, start_lsn: Lsn) -> Result<WalRecordIter> {
        let iter = self
            .inner
            .iter(start_lsn.get())
            .map_err(|e| Error::Io(map_wal_error(e)))?;
        Ok(WalRecordIter {
            inner: Box::new(iter),
        })
    }

    /// Append a storage-wal `Checkpoint` marker and return its LSN.
    pub fn checkpoint(&self, payload: impl AsRef<[u8]>) -> Result<Lsn> {
        self.inner
            .checkpoint(payload)
            .map_err(|e| Error::Io(map_wal_error(e)))
            .map(Lsn::new)
    }

    /// Truncate all WAL segments fully before the active segment.
    pub fn truncate_completed(&self) -> Result<usize> {
        self.inner
            .truncate_completed()
            .map_err(|e| Error::Io(map_wal_error(e)))
    }

    /// Read the single physiological record at `lsn`.
    ///
    /// This is used during rollback and version-chain traversal.  It iterates
    /// forward from `lsn`; the first decoded record must be the one requested.
    pub fn read_at(&self, lsn: Lsn) -> Result<Record> {
        let mut iter = self.iter(lsn)?;
        let (_rec_lsn, record) = iter
            .next()
            .ok_or_else(|| Error::Corruption(format!("WAL record at lsn {lsn} not found")))??;
        Ok(record)
    }

    /// Simulate a power-loss crash by truncating unfsynced WAL records.
    pub fn crash(&self) -> Result<()> {
        self.inner.crash().map_err(|e| Error::Io(map_wal_error(e)))
    }

    /// Close the WAL gracefully.
    pub fn close(&self) -> Result<()> {
        self.inner.close().map_err(|e| Error::Io(map_wal_error(e)))
    }
}

fn map_wal_error(e: storage_wal::Error) -> std::io::Error {
    std::io::Error::other(format!("{e}"))
}

/// Iterator over decoded physiological records, yielding `(WAL_LSN, Record)`.
pub struct WalRecordIter {
    inner: Box<dyn Iterator<Item = std::result::Result<storage_wal::Record, storage_wal::Error>>>,
}

impl Iterator for WalRecordIter {
    type Item = Result<(Lsn, Record)>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let rec = match self.inner.next()? {
                Ok(r) => r,
                Err(e) => return Some(Err(Error::Io(map_wal_error(e)))),
            };
            // We store physiological records inside storage-wal Put records.
            // Other record types (Checkpoint, etc.) are skipped here; checkpoint
            // metadata lives in META.
            if rec.ty != storage_wal::RecordType::Put {
                continue;
            }
            let lsn = Lsn::new(rec.lsn);
            match Record::decode(&rec.payload) {
                Ok((record, consumed)) => {
                    debug_assert_eq!(consumed, rec.payload.len());
                    return Some(Ok((lsn, record)));
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

// ------------------------------------------------------------------
// Page-level logging helpers used by the tree layer.
// ------------------------------------------------------------------

use crate::page::Page;

/// Append a WAL record for a page modification and update the page's
/// `page_lsn`.  The caller must hold the page's exclusive OLC latch.
pub(crate) fn log_and_set_lsn(page: &Page, wal: Option<&WalLog>, record: Record) -> Result<Lsn> {
    // Buffered append: the tree layer issues an explicit sync at operation
    // boundaries (autocommit end, transaction commit/rollback), so we only
    // pay one fsync per user-facing mutation instead of one per page change.
    let new_lsn = match wal {
        Some(w) => w.append_buffered(record)?,
        None => NULL_LSN,
    };
    set_page_lsn(page, new_lsn)?;
    Ok(new_lsn)
}

/// Set the on-page `page_lsn` without appending to the WAL.
pub(crate) fn set_page_lsn(page: &Page, lsn: Lsn) -> Result<()> {
    let mut header = page.header()?;
    header.page_lsn = lsn;
    page.set_header(&header);
    Ok(())
}

/// Insert a cell into `page`, logging the change and updating `page_lsn`.
/// The caller must hold the page's exclusive latch.
pub(crate) fn page_insert_logged(
    page: &Page,
    wal: Option<&WalLog>,
    key: &[u8],
    value: &crate::slot::ValueKind<'_>,
) -> Result<usize> {
    let page_lsn = page.header()?.page_lsn;
    let cell = OwnedCell {
        key: key.to_vec(),
        value: (*value).into_owned(),
        mvcc: None,
    };
    let record = Record {
        header: RecordHeader::new(
            RecordType::InsertCell,
            NULL_TXN_ID,
            NULL_LSN,
            page.id,
            page_lsn,
        ),
        payload: RecordPayload::InsertCell { cell },
    };
    let idx = page.insert(key, value)?;
    log_and_set_lsn(page, wal, record)?;
    Ok(idx)
}

/// Delete a cell from `page`, logging the change and updating `page_lsn`.
/// The caller must hold the page's exclusive latch.
pub(crate) fn page_delete_logged(page: &Page, wal: Option<&WalLog>, key: &[u8]) -> Result<bool> {
    let page_lsn = page.header()?.page_lsn;
    let record = Record {
        header: RecordHeader::new(
            RecordType::DeleteCell,
            NULL_TXN_ID,
            NULL_LSN,
            page.id,
            page_lsn,
        ),
        payload: RecordPayload::DeleteCell {
            key: key.to_vec(),
            old_cell: None,
            old_header: None,
        },
    };
    let existed = page.delete(key)?;
    if existed {
        log_and_set_lsn(page, wal, record)?;
    }
    Ok(existed)
}

/// Insert a cell into `page` on behalf of a transaction, logging the change with
/// the transaction id and `prev_lsn` chain.  The in-page cell carries `mvcc`.
/// The caller must hold the page's exclusive latch.
pub(crate) fn page_insert_txn_logged(
    page: &Page,
    wal: Option<&WalLog>,
    txn_id: TxnId,
    prev_lsn: Lsn,
    key: &[u8],
    value: &crate::slot::ValueKind<'_>,
    mvcc: crate::version::MvccHeader,
) -> Result<Lsn> {
    let page_lsn = page.header()?.page_lsn;
    let cell = OwnedCell {
        key: key.to_vec(),
        value: (*value).into_owned(),
        mvcc: Some(mvcc),
    };
    let record = Record {
        header: RecordHeader::new(RecordType::InsertCell, txn_id, prev_lsn, page.id, page_lsn),
        payload: RecordPayload::InsertCell { cell },
    };
    page.insert_with_mvcc(key, value, Some(&mvcc))?;
    log_and_set_lsn(page, wal, record)
}

/// Overwrite a cell in `page` on behalf of a transaction, logging the new cell,
/// the previous version, and the `prev_lsn` chain.  The in-page cell carries
/// `mvcc`.  The caller must hold the page's exclusive latch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn page_update_txn_logged(
    page: &Page,
    wal: Option<&WalLog>,
    txn_id: TxnId,
    prev_lsn: Lsn,
    key: &[u8],
    value: &crate::slot::ValueKind<'_>,
    mvcc: crate::version::MvccHeader,
    old_cell: OwnedCell,
    old_header: crate::version::MvccHeader,
) -> Result<Lsn> {
    let page_lsn = page.header()?.page_lsn;
    let cell = OwnedCell {
        key: key.to_vec(),
        value: (*value).into_owned(),
        mvcc: Some(mvcc),
    };
    let record = Record {
        header: RecordHeader::new(RecordType::UpdateCell, txn_id, prev_lsn, page.id, page_lsn),
        payload: RecordPayload::UpdateCell {
            cell,
            old_cell: Some(old_cell),
            old_header: Some(old_header),
        },
    };
    let lsn = log_and_set_lsn(page, wal, record)?;
    let mut page_mvcc = mvcc;
    page_mvcc.prev_version_lsn = lsn;
    page.insert_with_mvcc(key, value, Some(&page_mvcc))?;
    set_page_lsn(page, lsn)?;
    Ok(lsn)
}

/// Replace a cell with a tombstone on behalf of a transaction, logging the
/// previous version and the `prev_lsn` chain.  The in-page tombstone carries
/// `mvcc`.  The caller must hold the page's exclusive latch.
pub(crate) fn page_delete_txn_logged(
    page: &Page,
    wal: Option<&WalLog>,
    txn_id: TxnId,
    prev_lsn: Lsn,
    key: &[u8],
    old_cell: OwnedCell,
    old_header: crate::version::MvccHeader,
) -> Result<Lsn> {
    let page_lsn = page.header()?.page_lsn;
    let tombstone_mvcc = crate::version::MvccHeader {
        begin_ts: txn_id,
        end_ts: crate::txn::NULL_TXN_ID,
        prev_version_lsn: NULL_LSN,
    };
    let record = Record {
        header: RecordHeader::new(RecordType::DeleteCell, txn_id, prev_lsn, page.id, page_lsn),
        payload: RecordPayload::DeleteCell {
            key: key.to_vec(),
            old_cell: Some(old_cell),
            old_header: Some(old_header),
        },
    };
    let lsn = log_and_set_lsn(page, wal, record)?;
    let mut page_mvcc = tombstone_mvcc;
    page_mvcc.prev_version_lsn = lsn;
    page.insert_with_mvcc(key, &crate::slot::ValueKind::Tombstone, Some(&page_mvcc))?;
    set_page_lsn(page, lsn)?;
    Ok(lsn)
}

/// Move a cell from `src` to `dst`, logging the delete on `src` and the insert
/// on `dst` with a correct page-LSN chain.  The source cell's MVCC metadata is
/// preserved on the destination.  The caller must hold both pages' exclusive
/// latches.
pub(crate) fn page_move_cell_logged(
    src: &Page,
    dst: &Page,
    wal: Option<&WalLog>,
    cell: &OwnedCell,
) -> Result<()> {
    let src_lsn = src.header()?.page_lsn;
    let del_record = Record {
        header: RecordHeader::new(
            RecordType::DeleteCell,
            NULL_TXN_ID,
            NULL_LSN,
            src.id,
            src_lsn,
        ),
        payload: RecordPayload::DeleteCell {
            key: cell.key.clone(),
            old_cell: None,
            old_header: None,
        },
    };
    let del_lsn = match wal {
        Some(w) => w.append(del_record)?,
        None => NULL_LSN,
    };
    src.delete(&cell.key)?;
    set_page_lsn(src, del_lsn)?;

    let dst_lsn = dst.header()?.page_lsn;
    let ins_record = Record {
        header: RecordHeader::new(
            RecordType::InsertCell,
            NULL_TXN_ID,
            NULL_LSN,
            dst.id,
            dst_lsn,
        ),
        payload: RecordPayload::InsertCell { cell: cell.clone() },
    };
    let ins_lsn = match wal {
        Some(w) => w.append(ins_record)?,
        None => NULL_LSN,
    };
    dst.insert_with_mvcc(&cell.key, &cell.value.as_value_kind(), cell.mvcc.as_ref())?;
    set_page_lsn(dst, ins_lsn)?;
    Ok(())
}

/// Replace `old_key` with `new_key` (keeping the same value) on `page`, logging
/// the delete and insert with a correct page-LSN chain.  Used to update parent
/// separators during redistributions.
pub(crate) fn page_replace_key_logged(
    page: &Page,
    wal: Option<&WalLog>,
    old_key: &[u8],
    new_key: &[u8],
    value: &crate::slot::ValueKind<'_>,
) -> Result<()> {
    let page_lsn = page.header()?.page_lsn;
    let del_record = Record {
        header: RecordHeader::new(
            RecordType::DeleteCell,
            NULL_TXN_ID,
            NULL_LSN,
            page.id,
            page_lsn,
        ),
        payload: RecordPayload::DeleteCell {
            key: old_key.to_vec(),
            old_cell: None,
            old_header: None,
        },
    };
    let del_lsn = match wal {
        Some(w) => w.append(del_record)?,
        None => NULL_LSN,
    };
    page.delete(old_key)?;
    set_page_lsn(page, del_lsn)?;

    let page_lsn = page.header()?.page_lsn;
    let cell = OwnedCell {
        key: new_key.to_vec(),
        value: (*value).into_owned(),
        mvcc: None,
    };
    let ins_record = Record {
        header: RecordHeader::new(
            RecordType::InsertCell,
            NULL_TXN_ID,
            NULL_LSN,
            page.id,
            page_lsn,
        ),
        payload: RecordPayload::InsertCell { cell },
    };
    let ins_lsn = match wal {
        Some(w) => w.append(ins_record)?,
        None => NULL_LSN,
    };
    page.insert(new_key, value)?;
    set_page_lsn(page, ins_lsn)?;
    Ok(())
}

/// Update an internal page's leftmost child pointer, logging the change as a
/// `MoveRightmost` record.  The caller must hold the page's exclusive latch.
pub(crate) fn page_set_leftmost_child_logged(
    page: &Page,
    wal: Option<&WalLog>,
    old_leftmost: PageId,
    new_leftmost: PageId,
) -> Result<()> {
    let page_lsn = page.header()?.page_lsn;
    let record = Record {
        header: RecordHeader::new(
            RecordType::MoveRightmost,
            NULL_TXN_ID,
            NULL_LSN,
            page.id,
            page_lsn,
        ),
        payload: RecordPayload::MoveRightmost {
            old_rightmost: old_leftmost,
            new_rightmost: new_leftmost,
        },
    };
    let lsn = match wal {
        Some(w) => w.append(record)?,
        None => NULL_LSN,
    };
    page.set_leftmost_child(new_leftmost);
    set_page_lsn(page, lsn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::NULL_PAGE_ID;

    fn roundtrip(record: &Record) {
        let mut buf = vec![0u8; record.encoded_size()];
        record.encode(&mut buf).unwrap();
        let (decoded, consumed) = Record::decode(&buf).unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(&decoded, record);
    }

    #[test]
    fn insert_cell_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::InsertCell,
                TxnId::new(1),
                NULL_LSN,
                PageId::new(3),
                Lsn::new(7),
            ),
            payload: RecordPayload::InsertCell {
                cell: OwnedCell {
                    key: b"hello".to_vec(),
                    value: OwnedValue::Inline(b"world".to_vec()),
                    mvcc: None,
                },
            },
        });
    }

    #[test]
    fn delete_cell_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::DeleteCell,
                TxnId::new(2),
                Lsn::new(5),
                PageId::new(3),
                Lsn::new(7),
            ),
            payload: RecordPayload::DeleteCell {
                key: b"bye".to_vec(),
                old_cell: None,
                old_header: None,
            },
        });
    }

    #[test]
    fn delete_cell_with_undo_roundtrip() {
        use crate::version::MvccHeader;
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::DeleteCell,
                TxnId::new(2),
                Lsn::new(5),
                PageId::new(3),
                Lsn::new(7),
            ),
            payload: RecordPayload::DeleteCell {
                key: b"bye".to_vec(),
                old_cell: Some(OwnedCell {
                    key: b"bye".to_vec(),
                    value: OwnedValue::Inline(b"old".to_vec()),
                    mvcc: Some(MvccHeader::autocommit()),
                }),
                old_header: Some(MvccHeader::autocommit()),
            },
        });
    }

    #[test]
    fn update_cell_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::UpdateCell,
                TxnId::new(1),
                Lsn::new(10),
                PageId::new(3),
                Lsn::new(12),
            ),
            payload: RecordPayload::UpdateCell {
                cell: OwnedCell {
                    key: b"k".to_vec(),
                    value: OwnedValue::ValueLog {
                        offset: 0x1234_5678_9abc_def0,
                        len: 42,
                    },
                    mvcc: None,
                },
                old_cell: None,
                old_header: None,
            },
        });
    }

    #[test]
    fn split_page_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::SplitPage,
                NULL_TXN_ID,
                NULL_LSN,
                PageId::new(5),
                Lsn::new(9),
            ),
            payload: RecordPayload::SplitPage {
                separator: b"sep".to_vec(),
                right_page_id: PageId::new(6),
                is_internal: true,
            },
        });
    }

    #[test]
    fn merge_page_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::MergePage,
                NULL_TXN_ID,
                NULL_LSN,
                PageId::new(5),
                Lsn::new(9),
            ),
            payload: RecordPayload::MergePage {
                victim_page_id: PageId::new(6),
                victim_is_left: false,
                separator: b"sep".to_vec(),
                victim_leftmost: PageId::new(7),
            },
        });
    }

    #[test]
    fn move_rightmost_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::MoveRightmost,
                NULL_TXN_ID,
                NULL_LSN,
                PageId::new(2),
                Lsn::new(4),
            ),
            payload: RecordPayload::MoveRightmost {
                old_rightmost: PageId::new(10),
                new_rightmost: PageId::new(11),
            },
        });
    }

    #[test]
    fn set_root_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::SetRoot,
                NULL_TXN_ID,
                NULL_LSN,
                NULL_PAGE_ID,
                NULL_LSN,
            ),
            payload: RecordPayload::SetRoot {
                new_root_page_id: PageId::new(42),
            },
        });
    }

    #[test]
    fn new_root_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::NewRoot,
                NULL_TXN_ID,
                NULL_LSN,
                PageId::new(7),
                NULL_LSN,
            ),
            payload: RecordPayload::NewRoot {
                new_root_page_id: PageId::new(7),
                leftmost_child: PageId::new(3),
                separator: b"sep".to_vec(),
                right_child: PageId::new(5),
            },
        });
    }

    #[test]
    fn commit_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::Commit,
                TxnId::new(7),
                Lsn::new(99),
                NULL_PAGE_ID,
                Lsn::new(100),
            ),
            payload: RecordPayload::Commit {
                commit_ts: Timestamp::new(101),
            },
        });
    }

    #[test]
    fn begin_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::Begin,
                TxnId::new(7),
                NULL_LSN,
                NULL_PAGE_ID,
                NULL_LSN,
            ),
            payload: RecordPayload::Begin,
        });
    }

    #[test]
    fn abort_roundtrip() {
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::Abort,
                TxnId::new(7),
                Lsn::new(50),
                NULL_PAGE_ID,
                NULL_LSN,
            ),
            payload: RecordPayload::Abort,
        });
    }

    #[test]
    fn clr_roundtrip() {
        let original = Record {
            header: RecordHeader::new(
                RecordType::DeleteCell,
                TxnId::new(7),
                Lsn::new(50),
                PageId::new(3),
                Lsn::new(60),
            ),
            payload: RecordPayload::DeleteCell {
                key: b"undo-me".to_vec(),
                old_cell: None,
                old_header: None,
            },
        };
        roundtrip(&Record {
            header: RecordHeader::new(
                RecordType::Clr,
                TxnId::new(7),
                Lsn::new(110),
                PageId::new(3),
                Lsn::new(120),
            ),
            payload: RecordPayload::Clr {
                undo_next_lsn: Lsn::new(50),
                original: Box::new(original),
            },
        });
    }

    #[test]
    fn truncated_record_rejected() {
        let record = Record {
            header: RecordHeader::new(
                RecordType::DeleteCell,
                TxnId::new(1),
                NULL_LSN,
                PageId::new(3),
                Lsn::new(7),
            ),
            payload: RecordPayload::DeleteCell {
                key: b"key".to_vec(),
                old_cell: None,
                old_header: None,
            },
        };
        let mut buf = vec![0u8; record.encoded_size()];
        record.encode(&mut buf).unwrap();
        for n in 1..buf.len() {
            assert!(Record::decode(&buf[..n]).is_err() || n == buf.len());
        }
    }
}
