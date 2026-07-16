//! Transaction cursor that merges a pinned snapshot scan with buffered writes.
//!
//! A transaction must observe its own uncommitted writes (read-your-writes) while
//! still being isolated from concurrent writes that happened after the
//! transaction began.  `TxnCursor` builds a base cursor from the transaction's
//! pinned snapshot and overlays the transaction's buffered `WriteOp`s on top of
//! it, applying point puts, point deletes and range deletes in application
//! order.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;

use crate::Result;
use crate::column_family::ColumnFamilyId;
use crate::cursor::LsmCursor;
use crate::engine::LsmEngineInner;
use crate::transaction::{CfSnapshotView, WriteOp};
use crate::SequenceNumber;

/// Cursor over a key range that includes both the transaction snapshot and any
/// writes buffered inside the transaction.
pub(crate) struct TxnCursor {
    /// Snapshot cursor.  Already filtered by the transaction's sequence number
    /// and by deletion/range-deletion tombstones visible in the snapshot.
    base: LsmCursor,
    /// Buffered point entries that survive resolution, sorted by user key.
    buffered: Vec<(Vec<u8>, Vec<u8>)>,
    /// Buffered range tombstones that overlap the scan range.  All are
    /// logically newer than the snapshot, so they suppress matching base keys.
    range_tombstones: Vec<(Vec<u8>, Vec<u8>)>,
    /// Keys explicitly deleted by buffered point ops.  Kept separately from
    /// `buffered` so that base keys with the same user key can be skipped.
    deleted_keys: HashSet<Vec<u8>>,
    /// Cached next entry from `base`, or `Some(Err(...))` if the base cursor
    /// produced an error.
    base_peek: Option<Result<(Bytes, Bytes)>>,
    /// Position into `buffered` for the next buffered entry.
    buffered_pos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BufferedValue {
    /// A buffered put that has not been overwritten by a later delete.
    Present(Vec<u8>),
    /// A buffered delete or a range delete that covers the key.
    Deleted,
}

impl TxnCursor {
    /// Build a transaction cursor over `[start, end)` in column family `cf_id`.
    pub(crate) fn new(
        inner: LsmEngineInner,
        view: &CfSnapshotView,
        ops: &[WriteOp],
        cf_id: ColumnFamilyId,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        snapshot: SequenceNumber,
    ) -> Result<Self> {
        let base = LsmCursor::new_cf_view(
            inner,
            view,
            start.clone(),
            end.clone(),
            snapshot,
        )?;
        let resolved = resolve_buffered_ops(ops, cf_id, start.as_deref(), end.as_deref());

        let mut cursor = Self {
            base,
            buffered: resolved.present,
            range_tombstones: resolved.tombstones,
            deleted_keys: resolved.deleted,
            base_peek: None,
            buffered_pos: 0,
        };
        cursor.advance_base();
        Ok(cursor)
    }

    /// Advance the base cursor to the next entry that is not suppressed by a
    /// buffered delete or buffered range tombstone.
    fn advance_base(&mut self) {
        loop {
            match self.base.next() {
                None => {
                    self.base_peek = None;
                    return;
                }
                Some(Err(e)) => {
                    self.base_peek = Some(Err(e));
                    return;
                }
                Some(Ok((key, value))) => {
                    if self.should_skip_base(&key) {
                        continue;
                    }
                    self.base_peek = Some(Ok((key, value)));
                    return;
                }
            }
        }
    }

    /// True if a base key should be hidden because of a buffered point delete
    /// or buffered range tombstone.
    fn should_skip_base(&self, key: &[u8]) -> bool {
        if self.deleted_keys.contains(key) {
            return true;
        }
        self.range_tombstones
            .iter()
            .any(|(start, end)| key >= start.as_slice() && key < end.as_slice())
    }

    /// The next buffered entry, if any.
    fn peek_buffered(&self) -> Option<&(Vec<u8>, Vec<u8>)> {
        self.buffered.get(self.buffered_pos)
    }
}

impl Iterator for TxnCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        let buffered = self.peek_buffered().cloned();
        match (self.base_peek.take(), buffered) {
            (None, None) => None,

            (Some(base_res), None) => {
                self.advance_base();
                Some(base_res)
            }

            (None, Some((key, value))) => {
                self.buffered_pos += 1;
                Some(Ok((
                    Bytes::copy_from_slice(&key),
                    Bytes::copy_from_slice(&value),
                )))
            }

            (Some(Err(e)), Some(_)) => {
                self.advance_base();
                Some(Err(e))
            }

            (Some(Ok((base_key, base_value))), Some((buf_key, buf_value))) => {
                match buf_key.as_slice().cmp(&base_key) {
                    Ordering::Less => {
                        // Buffered key comes first; return it and keep the
                        // base entry for the next comparison.
                        self.buffered_pos += 1;
                        self.base_peek = Some(Ok((base_key, base_value)));
                        Some(Ok((
                            Bytes::copy_from_slice(&buf_key),
                            Bytes::copy_from_slice(&buf_value),
                        )))
                    }
                    Ordering::Equal => {
                        // Buffered write wins; skip the matching base entry.
                        self.buffered_pos += 1;
                        self.advance_base();
                        Some(Ok((
                            Bytes::copy_from_slice(&buf_key),
                            Bytes::copy_from_slice(&buf_value),
                        )))
                    }
                    Ordering::Greater => {
                        // Base key comes first and is not suppressed
                        // (advance_base already skipped suppressed keys).
                        self.advance_base();
                        Some(Ok((base_key, base_value)))
                    }
                }
            }
        }
    }
}

impl storage_traits::Cursor for TxnCursor {
    type Error = crate::Error;

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.base.seek(target)?;
        self.buffered_pos = self
            .buffered
            .partition_point(|(key, _)| key.as_slice() < target);
        self.base_peek = None;
        self.advance_base();
        Ok(())
    }
}

/// Resolved state of the buffered operations for a single column family and
/// scan range.
struct ResolvedBuffered {
    /// Buffered keys that survive resolution, sorted ascending.
    present: Vec<(Vec<u8>, Vec<u8>)>,
    /// Buffered range tombstones that overlap the scan range.
    tombstones: Vec<(Vec<u8>, Vec<u8>)>,
    /// Keys explicitly deleted by buffered operations.
    deleted: HashSet<Vec<u8>>,
}

/// Apply the transaction's buffered operations in application order and produce
/// a sorted, de-duplicated view of the buffered changes that fall inside the
/// requested scan range.
///
/// All buffered operations are logically newer than the transaction snapshot,
/// so a later operation always overrides an earlier one and buffered range
/// tombstones suppress matching base keys.
fn resolve_buffered_ops(
    ops: &[WriteOp],
    cf_id: ColumnFamilyId,
    start: Option<&[u8]>,
    end: Option<&[u8]>,
) -> ResolvedBuffered {
    let mut state: BTreeMap<Vec<u8>, BufferedValue> = BTreeMap::new();
    let mut tombstones: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

    for op in ops {
        match op {
            WriteOp::Put { cf, key, value }
                if *cf == cf_id && key_in_range(key, start, end) =>
            {
                state.insert(key.clone(), BufferedValue::Present(value.clone()));
            }
            WriteOp::Delete { cf, key }
                if *cf == cf_id && key_in_range(key, start, end) =>
            {
                state.insert(key.clone(), BufferedValue::Deleted);
            }
            WriteOp::DeleteRange {
                cf,
                start: range_start,
                end: range_end,
            } if *cf == cf_id
                && ranges_overlap(
                    Some(range_start.as_slice()),
                    Some(range_end.as_slice()),
                    start,
                    end,
                ) =>
            {
                // Any buffered keys inside the range are deleted.  Later
                // point operations may re-insert them, which is correct
                // because operations are applied in order.
                for (_, value) in state.range_mut(range_start.clone()..range_end.clone()) {
                    *value = BufferedValue::Deleted;
                }
                tombstones.push((range_start.clone(), range_end.clone()));
            }
            _ => {}
        }
    }

    let mut present = Vec::new();
    let mut deleted = HashSet::new();
    for (key, value) in state {
        match value {
            BufferedValue::Present(v) => present.push((key, v)),
            BufferedValue::Deleted => {
                deleted.insert(key);
            }
        }
    }

    ResolvedBuffered {
        present,
        tombstones,
        deleted,
    }
}

/// True if `key` lies in `[start, end)`.
fn key_in_range(key: &[u8], start: Option<&[u8]>, end: Option<&[u8]>) -> bool {
    start.is_none_or(|s| key >= s) && end.is_none_or(|e| key < e)
}

/// True if `[a_start, a_end)` overlaps `[b_start, b_end)`.
fn ranges_overlap(
    a_start: Option<&[u8]>,
    a_end: Option<&[u8]>,
    b_start: Option<&[u8]>,
    b_end: Option<&[u8]>,
) -> bool {
    let left = match (a_start, b_end) {
        (Some(a), Some(b)) => a < b,
        (Some(_), None) => true,
        (None, _) => true,
    };
    let right = match (a_end, b_start) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        (None, _) => true,
    };
    left && right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_put_overrides_earlier_put() {
        let ops = vec![
            WriteOp::Put {
                cf: 0,
                key: b"a".to_vec(),
                value: b"first".to_vec(),
            },
            WriteOp::Put {
                cf: 0,
                key: b"a".to_vec(),
                value: b"second".to_vec(),
            },
        ];
        let resolved = resolve_buffered_ops(&ops, 0, None, None);
        assert_eq!(resolved.present, vec![(b"a".to_vec(), b"second".to_vec())]);
        assert!(resolved.deleted.is_empty());
    }

    #[test]
    fn resolve_delete_overrides_put() {
        let ops = vec![
            WriteOp::Put {
                cf: 0,
                key: b"a".to_vec(),
                value: b"v".to_vec(),
            },
            WriteOp::Delete {
                cf: 0,
                key: b"a".to_vec(),
            },
        ];
        let resolved = resolve_buffered_ops(&ops, 0, None, None);
        assert!(resolved.present.is_empty());
        assert!(resolved.deleted.contains(b"a".as_slice()));
    }

    #[test]
    fn resolve_later_put_after_range_delete_wins() {
        let ops = vec![
            WriteOp::DeleteRange {
                cf: 0,
                start: b"a".to_vec(),
                end: b"z".to_vec(),
            },
            WriteOp::Put {
                cf: 0,
                key: b"m".to_vec(),
                value: b"v".to_vec(),
            },
        ];
        let resolved = resolve_buffered_ops(&ops, 0, None, None);
        assert_eq!(resolved.present, vec![(b"m".to_vec(), b"v".to_vec())]);
        assert_eq!(resolved.tombstones.len(), 1);
    }

    #[test]
    fn resolve_range_delete_after_put_deletes_it() {
        let ops = vec![
            WriteOp::Put {
                cf: 0,
                key: b"m".to_vec(),
                value: b"v".to_vec(),
            },
            WriteOp::DeleteRange {
                cf: 0,
                start: b"a".to_vec(),
                end: b"z".to_vec(),
            },
        ];
        let resolved = resolve_buffered_ops(&ops, 0, None, None);
        assert!(resolved.present.is_empty());
        assert!(resolved.deleted.contains(b"m".as_slice()));
    }

    #[test]
    fn resolve_filters_by_column_family() {
        let ops = vec![WriteOp::Put {
            cf: 7,
            key: b"a".to_vec(),
            value: b"v".to_vec(),
        }];
        let resolved = resolve_buffered_ops(&ops, 0, None, None);
        assert!(resolved.present.is_empty());
    }

    #[test]
    fn resolve_filters_by_range() {
        let ops = vec![WriteOp::Put {
            cf: 0,
            key: b"c".to_vec(),
            value: b"v".to_vec(),
        }];
        let resolved = resolve_buffered_ops(&ops, 0, Some(b"a"), Some(b"b"));
        assert!(resolved.present.is_empty());
    }
}
