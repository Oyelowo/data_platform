//! LSM transactions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;

use crate::cache::BlockCaches;
use crate::column_family::{ColumnFamilyHandle, ColumnFamilyId};
use crate::engine::LsmEngineInner;
use crate::txn_cursor::TxnCursor;
use crate::memtable::MemTable;
use crate::version::Version;
use crate::{Error, Result, SequenceNumber};

/// A single buffered write inside a transaction.
///
/// Operations are stored in the order the application issued them and are
/// applied to the engine on commit.  Range deletes and point operations
/// interleave naturally because each operation receives its own sequence
/// number in commit order.
#[derive(Debug)]
pub(crate) enum WriteOp {
    Put {
        cf: ColumnFamilyId,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        cf: ColumnFamilyId,
        key: Vec<u8>,
    },
    DeleteRange {
        cf: ColumnFamilyId,
        start: Vec<u8>,
        end: Vec<u8>,
    },
}

/// A consistent view of a single column family captured when a transaction
/// begins.
#[derive(Clone)]
pub(crate) struct CfSnapshotView {
    pub(crate) memtable: Arc<MemTable>,
    pub(crate) immutable: Vec<Arc<MemTable>>,
    pub(crate) version: Arc<Version>,
    pub(crate) caches: BlockCaches,
    pub(crate) path: PathBuf,
}

/// A consistent view of the engine state captured when a transaction begins.
///
/// The view pins the active MemTable, the immutable queue, and the current
/// `Version` for every column family so that compaction and flush cannot
/// invalidate the snapshot.
#[derive(Clone)]
pub(crate) struct SnapshotView {
    pub(crate) default: CfSnapshotView,
    pub(crate) cf_views: HashMap<ColumnFamilyId, CfSnapshotView>,
}

impl SnapshotView {
    /// Return the view for a column family, or `None` if the CF did not exist
    /// when the transaction began.
    pub(crate) fn cf(&self, cf: &ColumnFamilyHandle) -> Option<&CfSnapshotView> {
        self.cf_views.get(&cf.id)
    }
}

/// A write transaction for the LSM engine.
pub struct LsmTransaction {
    pub(crate) inner: LsmEngineInner,
    pub(crate) read_only: bool,
    pub(crate) sequence: SequenceNumber,
    pub(crate) view: SnapshotView,
    pub(crate) finished: bool,
    /// Buffered writes, in application order.
    pub(crate) ops: Vec<WriteOp>,
}

impl LsmTransaction {
    pub fn new(inner: LsmEngineInner, read_only: bool, sequence: SequenceNumber) -> Self {
        // Register the snapshot so compaction preserves versions visible to
        // this transaction, and pin the full engine view so its MemTables and
        // SSTable files are not deleted or replaced while the transaction is
        // alive.
        let view = {
            let mut state = inner.state.lock().unwrap();
            state.snapshots.register(sequence);
            let mut cf_views = HashMap::new();
            for cf in state.column_families.iter() {
                let view = CfSnapshotView {
                    memtable: cf.memtable.lock().unwrap().clone(),
                    immutable: cf.immutable.snapshot(),
                    version: cf.version_set.current(),
                    caches: cf.caches.clone(),
                    path: state.path.clone(),
                };
                cf_views.insert(cf.id, view);
            }
            let default = cf_views.get(&0).cloned().unwrap_or_else(|| CfSnapshotView {
                memtable: Arc::new(MemTable::new()),
                immutable: Vec::new(),
                version: Arc::new(Version::new(7)),
                caches: state.default_cf().caches.clone(),
                path: state.path.clone(),
            });
            SnapshotView { default, cf_views }
        };
        Self {
            inner,
            read_only,
            sequence,
            view,
            finished: false,
            ops: Vec::new(),
        }
    }

    fn check_write(&self) -> Result<()> {
        if self.read_only {
            return Err(Error::ReadOnlyTxn);
        }
        if self.finished {
            return Err(Error::TxnFinished);
        }
        Ok(())
    }
}

impl Drop for LsmTransaction {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock().unwrap();
        state.snapshots.unregister(self.sequence);
    }
}

impl LsmTransaction {
    /// The sequence number that defines this transaction's snapshot.
    pub fn snapshot_sequence(&self) -> SequenceNumber {
        self.sequence
    }

    /// Evaluate buffered writes for `cf_id`/`key` in application order.
    ///
    /// * `Some(Some(value))` – a buffered put.
    /// * `Some(None)`        – a buffered delete or a range delete covering the key.
    /// * `None`              – no buffered write; fall back to the snapshot view.
    fn local_get(&self, cf_id: ColumnFamilyId, key: &[u8]) -> Option<Option<Bytes>> {
        let mut result: Option<Option<Bytes>> = None;
        for op in &self.ops {
            match op {
                WriteOp::Put { cf, key: k, value } if *cf == cf_id && k.as_slice() == key => {
                    result = Some(Some(Bytes::copy_from_slice(value)));
                }
                WriteOp::Delete { cf, key: k } if *cf == cf_id && k.as_slice() == key => {
                    result = Some(None);
                }
                WriteOp::DeleteRange { cf, start, end }
                    if *cf == cf_id
                        && key >= start.as_slice()
                        && key < end.as_slice()
                        && result.is_none() =>
                {
                    // A later point operation can overwrite this; until then the
                    // key is considered deleted.
                    result = Some(None);
                }
                _ => {}
            }
        }
        result
    }

    /// Read `key` from `cf` using this transaction's snapshot and any buffered
    /// writes.
    pub fn get_cf(&self, cf: &ColumnFamilyHandle, key: &[u8]) -> Result<Option<Bytes>> {
        if let Some(local) = self.local_get(cf.id, key) {
            return Ok(local);
        }
        match self.view.cf(cf) {
            Some(view) => self.inner.get_cf_with_view(key, self.sequence, view),
            None => Err(Error::InvalidArgument(
                "column family did not exist at transaction start".into(),
            )),
        }
    }

    /// Write `value` under `key` in `cf`.
    ///
    /// The write is buffered until the transaction commits; it is visible to
    /// reads inside the transaction immediately (read-your-writes).
    pub fn put_cf(&mut self, cf: &ColumnFamilyHandle, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Put {
            cf: cf.id,
            key: key.to_vec(),
            value: value.to_vec(),
        });
        Ok(())
    }

    /// Delete `key` from `cf`.
    pub fn delete_cf(&mut self, cf: &ColumnFamilyHandle, key: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Delete {
            cf: cf.id,
            key: key.to_vec(),
        });
        Ok(())
    }

    /// Delete all keys in `[start, end)` from `cf`.
    pub fn delete_range_cf(
        &mut self,
        cf: &ColumnFamilyHandle,
        start: &[u8],
        end: &[u8],
    ) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::DeleteRange {
            cf: cf.id,
            start: start.to_vec(),
            end: end.to_vec(),
        });
        Ok(())
    }

    /// Return a cursor over `[start, end)` in `cf` using this transaction's
    /// snapshot, merged with any writes buffered inside the transaction.
    pub fn scan_cf(
        &self,
        cf: &ColumnFamilyHandle,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl storage_traits::Cursor<Error = Error>> {
        match self.view.cf(cf) {
            Some(view) => TxnCursor::new(
                self.inner.clone(),
                view,
                &self.ops,
                cf.id,
                start.map(|s| s.to_vec()),
                end.map(|e| e.to_vec()),
                self.sequence,
            ),
            None => Err(Error::InvalidArgument(
                "column family did not exist at transaction start".into(),
            )),
        }
    }
}

impl storage_traits::Transaction for LsmTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        if let Some(local) = self.local_get(0, key) {
            return Ok(local);
        }
        self.inner.get_with_view(key, self.sequence, &self.view.default)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Put {
            cf: 0,
            key: key.to_vec(),
            value: value.to_vec(),
        });
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Delete {
            cf: 0,
            key: key.to_vec(),
        });
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl storage_traits::Cursor<Error = Self::Error>> {
        TxnCursor::new(
            self.inner.clone(),
            &self.view.default,
            &self.ops,
            0,
            start.map(|s| s.to_vec()),
            end.map(|e| e.to_vec()),
            self.sequence,
        )
    }

    fn commit(mut self) -> Result<()> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        self.finished = true;
        self.inner.apply_transaction_ops(&self.ops)?;
        self.inner.sync()
    }

    fn rollback(mut self) -> Result<()> {
        self.finished = true;
        self.ops.clear();
        Ok(())
    }

    fn set_isolation(&mut self, level: storage_traits::IsolationLevel) -> Result<()> {
        match level {
            storage_traits::IsolationLevel::ReadCommitted => Ok(()),
            _ => Err(Error::InvalidArgument("unsupported isolation level".into())),
        }
    }
}
