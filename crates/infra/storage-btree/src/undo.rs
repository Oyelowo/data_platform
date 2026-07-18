//! MVCC undo helpers.
//!
//! This module contains the logic for generating undo images, applying them to
//! a page during runtime rollback, and applying Compensation Log Records during
//! recovery.

use crate::error::Result;
use crate::page::Page;
use crate::slot::OwnedCell;
use crate::txn::TxnId;
use crate::version::MvccHeader;
use crate::wal::{Lsn, WalLog};

/// Information needed to restore the previous version of a key during rollback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoImage {
    /// The cell as it existed before the modifying operation.
    pub cell: OwnedCell,
    /// The MVCC header that belonged to the old cell.
    pub header: MvccHeader,
}

/// Build an undo image from an old cell and its header.
pub fn make_undo_image(cell: OwnedCell, header: MvccHeader) -> UndoImage {
    UndoImage { cell, header }
}

/// Restore the cell described by `image` into `page`.
///
/// The current in-page cell for `key` (an uncommitted update or tombstone) is
/// replaced with the old cell from the undo image.  This is the inverse of an
/// update or delete performed by a transaction.
pub fn apply_undo_to_page(page: &Page, key: &[u8], image: &UndoImage) -> Result<()> {
    // Re-insert the old cell with its original MVCC header.  `insert_with_mvcc`
    // replaces any existing cell for the key, including a tombstone.
    page.insert_with_mvcc(key, &image.cell.value.as_value_kind(), Some(&image.header))?;
    Ok(())
}

/// Append a Compensation Log Record to `wal` for a record being undone.
///
/// `original_lsn` is the LSN of the record being compensated.  `undo_next_lsn`
/// is the original record's `prev_lsn`: the next record that still needs
/// undoing.  `original` is the record being compensated.  The CLR is redone by
/// restoring `original`'s old cell (or deleting the key for an insert).
pub fn append_clr(
    wal: &WalLog,
    txn_id: TxnId,
    original_lsn: Lsn,
    undo_next_lsn: Lsn,
    original: &crate::wal::Record,
) -> Result<Lsn> {
    let record = crate::wal::Record {
        header: crate::wal::RecordHeader::new(
            crate::wal::RecordType::Clr,
            txn_id,
            undo_next_lsn,
            original.header.page_id,
            original_lsn,
        ),
        payload: crate::wal::RecordPayload::Clr {
            undo_next_lsn,
            original: Box::new(original.clone()),
        },
    };
    wal.append(record)
}
