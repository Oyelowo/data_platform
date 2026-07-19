//! MVCC version header and visibility function.
//!
//! Each leaf cell can carry an optional [`MvccHeader`] that records:
//! - `begin_ts`: the transaction that created this version,
//! - `end_ts`:   the transaction that invalidated this version (or NULL),
//! - `prev_version_lsn`: the WAL LSN of the record that holds the previous
//!   version, so snapshot reads can walk backward in time.

use storage_format::{read_u64_le, write_u64_le};

use crate::error::Result;
use crate::slot::{OwnedValue, ValueKind};
use crate::txn::{NULL_TXN_ID, Timestamp, TxnId, TxnOracle};
use crate::wal::Lsn;

/// MVCC metadata attached to a cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MvccHeader {
    /// Transaction that created this version.
    pub begin_ts: TxnId,
    /// Transaction that invalidated this version, or `NULL_TXN_ID` if current.
    pub end_ts: TxnId,
    /// WAL LSN of the record holding the previous version, or `NULL_LSN`.
    pub prev_version_lsn: Lsn,
}

impl MvccHeader {
    /// On-wire size of an MVCC header in bytes.
    pub const SIZE: usize = 8 + 8 + 8;

    /// Create a header representing an autocommit (always-committed) value.
    pub fn autocommit() -> Self {
        Self {
            begin_ts: NULL_TXN_ID,
            end_ts: NULL_TXN_ID,
            prev_version_lsn: crate::wal::NULL_LSN,
        }
    }

    /// True if this header is the autocommit sentinel.
    pub fn is_autocommit(&self) -> bool {
        self.begin_ts == NULL_TXN_ID && self.end_ts == NULL_TXN_ID
    }

    /// Encode the header into `buf`, which must be at least [`Self::SIZE`] bytes.
    pub fn encode(&self, buf: &mut [u8]) -> Result<()> {
        if buf.len() < Self::SIZE {
            return Err(crate::error::Error::Corruption(
                "MVCC header buffer too small".into(),
            ));
        }
        write_u64_le(&mut buf[0..8], self.begin_ts.get());
        write_u64_le(&mut buf[8..16], self.end_ts.get());
        write_u64_le(&mut buf[16..24], self.prev_version_lsn.get());
        Ok(())
    }

    /// Decode a header from `buf`.
    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::SIZE {
            return Err(crate::error::Error::Corruption(
                "MVCC header truncated".into(),
            ));
        }
        Ok(Self {
            begin_ts: TxnId::new(read_u64_le(&buf[0..8])),
            end_ts: TxnId::new(read_u64_le(&buf[8..16])),
            prev_version_lsn: Lsn::new(read_u64_le(&buf[16..24])),
        })
    }
}

/// Result of resolving a version chain for a reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisibleValue {
    /// Key is present with this value.
    Found(OwnedValue),
    /// Key is deleted in this snapshot.
    NotFound,
    /// The visible version lives in the WAL at the given LSN.
    InWal(Lsn),
}

/// Decide whether the cell described by `header` is visible to a reader with
/// `read_ts` and `self_txn_id` under `oracle`.
///
/// `self_txn_id` is `NULL_TXN_ID` for autocommit reads.
pub fn is_visible(
    header: &MvccHeader,
    value: &ValueKind<'_>,
    read_ts: Timestamp,
    self_txn_id: TxnId,
    oracle: &dyn TxnOracle,
) -> VisibleValue {
    // Autocommit values are always visible unless explicitly invalidated.
    if header.is_autocommit() {
        if header.end_ts != NULL_TXN_ID {
            // Even autocommit cells can be invalidated by a transaction.
            return if invalidated(header.end_ts, read_ts, self_txn_id, oracle) {
                VisibleValue::NotFound
            } else {
                value_to_visible(value)
            };
        }
        return value_to_visible(value);
    }

    // Own writes are always visible to the writing transaction.
    if header.begin_ts == self_txn_id {
        return if value_is_tombstone(value) {
            VisibleValue::NotFound
        } else {
            value_to_visible(value)
        };
    }

    // Creator must be committed by the time of the read.
    let creator_commit = match oracle.commit_ts(header.begin_ts) {
        Some(ts) => ts,
        None => {
            // Active or aborted creator: not visible to anyone else.
            return follow_prev_version(header, value);
        }
    };
    if creator_commit > read_ts {
        // Created after our snapshot.
        return follow_prev_version(header, value);
    }

    // Check invalidation.
    if header.end_ts == NULL_TXN_ID {
        return value_to_visible(value);
    }

    if header.end_ts == self_txn_id {
        // We deleted it; our own snapshot must not see it.
        return VisibleValue::NotFound;
    }

    let end_commit = oracle.commit_ts(header.end_ts);
    match end_commit {
        Some(ts) if ts <= read_ts => {
            // Deleted at or before our snapshot.
            VisibleValue::NotFound
        }
        _ => {
            // Delete is active/aborted or after our snapshot: version is visible.
            value_to_visible(value)
        }
    }
}

fn invalidated(
    end_ts: TxnId,
    read_ts: Timestamp,
    self_txn_id: TxnId,
    oracle: &dyn TxnOracle,
) -> bool {
    if end_ts == self_txn_id {
        return true;
    }
    match oracle.commit_ts(end_ts) {
        Some(ts) => ts <= read_ts,
        None => false,
    }
}

fn value_to_visible(value: &ValueKind<'_>) -> VisibleValue {
    if value_is_tombstone(value) {
        VisibleValue::NotFound
    } else {
        VisibleValue::Found(value.into_owned())
    }
}

fn value_is_tombstone(value: &ValueKind<'_>) -> bool {
    matches!(value, ValueKind::Tombstone)
}

/// If the current version is not visible, direct the caller to the previous
/// version in the WAL.  If there is no previous version, the key is not found.
fn follow_prev_version(header: &MvccHeader, value: &ValueKind<'_>) -> VisibleValue {
    if header.prev_version_lsn != crate::wal::NULL_LSN {
        VisibleValue::InWal(header.prev_version_lsn)
    } else if header.begin_ts == NULL_TXN_ID && value_is_tombstone(value) {
        // Autocommit tombstone with no history.
        VisibleValue::NotFound
    } else {
        VisibleValue::NotFound
    }
}

/// Walk a version chain using `fetch` until a visible version is found.
/// `fetch(lsn)` must return `(MvccHeader, ValueKind)` for the WAL record at
/// `lsn`.
pub fn resolve_version_chain<F>(
    initial_header: &MvccHeader,
    initial_value: &ValueKind<'_>,
    read_ts: Timestamp,
    self_txn_id: TxnId,
    oracle: &dyn TxnOracle,
    mut fetch: F,
) -> Result<VisibleValue>
where
    F: FnMut(Lsn) -> Result<Option<(MvccHeader, OwnedValue)>>,
{
    let mut header = *initial_header;
    let mut value = initial_value.into_owned();

    loop {
        let vk = value.as_value_kind();
        match is_visible(&header, &vk, read_ts, self_txn_id, oracle) {
            VisibleValue::InWal(lsn) => {
                if let Some((h, v)) = fetch(lsn)? {
                    header = h;
                    value = v;
                } else {
                    return Ok(VisibleValue::NotFound);
                }
            }
            other => return Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot::OwnedValue;
    use crate::txn::{TxnId, TxnOracle};
    use crate::wal::NULL_LSN;

    struct StaticOracle {
        committed: std::collections::HashMap<TxnId, Timestamp>,
        aborted: std::collections::HashSet<TxnId>,
    }

    impl TxnOracle for StaticOracle {
        fn commit_ts(&self, txn_id: TxnId) -> Option<Timestamp> {
            self.committed.get(&txn_id).copied()
        }
        fn is_active(&self, txn_id: TxnId) -> bool {
            !self.committed.contains_key(&txn_id) && !self.aborted.contains(&txn_id)
        }
        fn is_aborted(&self, txn_id: TxnId) -> bool {
            self.aborted.contains(&txn_id)
        }
    }

    fn oracle() -> StaticOracle {
        StaticOracle {
            committed: std::collections::HashMap::new(),
            aborted: std::collections::HashSet::new(),
        }
    }

    #[test]
    fn autocommit_value_visible() {
        let o = oracle();
        let h = MvccHeader::autocommit();
        let v = ValueKind::Inline(b"v");
        assert_eq!(
            is_visible(&h, &v, Timestamp::new(100), NULL_TXN_ID, &o),
            VisibleValue::Found(OwnedValue::Inline(b"v".to_vec()))
        );
    }

    #[test]
    fn committed_version_visible() {
        let mut o = oracle();
        o.committed.insert(TxnId::new(5), Timestamp::new(10));
        let h = MvccHeader {
            begin_ts: TxnId::new(5),
            end_ts: NULL_TXN_ID,
            prev_version_lsn: NULL_LSN,
        };
        let v = ValueKind::Inline(b"v");
        assert_eq!(
            is_visible(&h, &v, Timestamp::new(20), NULL_TXN_ID, &o),
            VisibleValue::Found(OwnedValue::Inline(b"v".to_vec()))
        );
        // Before creator commit: not visible.
        assert_eq!(
            is_visible(&h, &v, Timestamp::new(5), NULL_TXN_ID, &o),
            VisibleValue::NotFound
        );
    }

    #[test]
    fn own_write_visible_even_before_commit() {
        let o = oracle();
        let h = MvccHeader {
            begin_ts: TxnId::new(5),
            end_ts: NULL_TXN_ID,
            prev_version_lsn: NULL_LSN,
        };
        let v = ValueKind::Inline(b"v");
        assert_eq!(
            is_visible(&h, &v, Timestamp::new(1), TxnId::new(5), &o),
            VisibleValue::Found(OwnedValue::Inline(b"v".to_vec()))
        );
    }

    #[test]
    fn deleted_version_not_visible_after_commit() {
        let mut o = oracle();
        o.committed.insert(TxnId::new(5), Timestamp::new(10)); // creator
        o.committed.insert(TxnId::new(7), Timestamp::new(15)); // deleter
        let h = MvccHeader {
            begin_ts: TxnId::new(5),
            end_ts: TxnId::new(7),
            prev_version_lsn: NULL_LSN,
        };
        let v = ValueKind::Inline(b"v");
        assert_eq!(
            is_visible(&h, &v, Timestamp::new(20), NULL_TXN_ID, &o),
            VisibleValue::NotFound
        );
        assert_eq!(
            is_visible(&h, &v, Timestamp::new(12), NULL_TXN_ID, &o),
            VisibleValue::Found(OwnedValue::Inline(b"v".to_vec()))
        );
    }

    #[test]
    fn aborted_delete_is_ignored() {
        let mut o = oracle();
        o.committed.insert(TxnId::new(5), Timestamp::new(10));
        o.aborted.insert(TxnId::new(7));
        let h = MvccHeader {
            begin_ts: TxnId::new(5),
            end_ts: TxnId::new(7),
            prev_version_lsn: NULL_LSN,
        };
        let v = ValueKind::Inline(b"v");
        assert_eq!(
            is_visible(&h, &v, Timestamp::new(20), NULL_TXN_ID, &o),
            VisibleValue::Found(OwnedValue::Inline(b"v".to_vec()))
        );
    }

    #[test]
    fn chain_resolution_fetches_previous_version() {
        let mut o = oracle();
        o.committed.insert(TxnId::new(10), Timestamp::new(20));
        let h = MvccHeader {
            begin_ts: TxnId::new(10),
            end_ts: NULL_TXN_ID,
            prev_version_lsn: Lsn::new(100),
        };
        let v = ValueKind::Inline(b"new");
        let result = resolve_version_chain(
            &h,
            &v,
            Timestamp::new(5), // before creator commit
            NULL_TXN_ID,
            &o,
            |lsn| {
                assert_eq!(lsn, Lsn::new(100));
                Ok(Some((
                    MvccHeader::autocommit(),
                    OwnedValue::Inline(b"old".to_vec()),
                )))
            },
        )
        .unwrap();
        assert_eq!(
            result,
            VisibleValue::Found(OwnedValue::Inline(b"old".to_vec()))
        );
    }
}
