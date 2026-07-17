//! B+ tree transactions.

use bytes::Bytes;

use crate::cursor::BtreeCursor;
use crate::engine::{BtreeEngineInner, SnapshotGuard};
use crate::error::{Error, Result};
use crate::page::PageId;

#[derive(Clone, Debug)]
pub(crate) enum WriteOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// A transaction against a B+ tree engine.
pub struct BtreeTransaction {
    pub(crate) inner: std::sync::Arc<BtreeEngineInner>,
    pub(crate) read_only: bool,
    pub(crate) finished: bool,
    pub(crate) snapshot_root: PageId,
    /// Keeps the snapshot root pinned for the lifetime of the transaction.
    pub(crate) _guard: SnapshotGuard,
    pub(crate) ops: Vec<WriteOp>,
}

impl BtreeTransaction {
    pub(crate) fn new(
        inner: std::sync::Arc<BtreeEngineInner>,
        read_only: bool,
        snapshot_root: PageId,
        guard: SnapshotGuard,
    ) -> Self {
        Self {
            inner,
            read_only,
            finished: false,
            snapshot_root,
            _guard: guard,
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

    fn local_get(&self, key: &[u8]) -> Option<Option<Bytes>> {
        let mut result: Option<Option<Bytes>> = None;
        for op in &self.ops {
            match op {
                WriteOp::Put { key: k, value } if k.as_slice() == key => {
                    result = Some(Some(Bytes::copy_from_slice(value)));
                }
                WriteOp::Delete { key: k } if k.as_slice() == key => {
                    result = Some(None);
                }
                _ => {}
            }
        }
        result
    }
}

impl storage_traits::Transaction for BtreeTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        if let Some(local) = self.local_get(key) {
            return Ok(local);
        }
        self.inner.tree.get(self.snapshot_root, key)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        });
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.check_write()?;
        self.ops.push(WriteOp::Delete { key: key.to_vec() });
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<impl storage_traits::Cursor<Error = Self::Error>> {
        let guard = SnapshotGuard::new(std::sync::Arc::clone(&self.inner), self.snapshot_root);
        BtreeCursor::new(
            std::sync::Arc::clone(&self.inner),
            self.snapshot_root,
            guard,
            start.map(Bytes::copy_from_slice),
            end.map(Bytes::copy_from_slice),
        )
    }

    fn commit(mut self) -> Result<()> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        self.finished = true;
        self.inner.apply_ops(&self.ops)
    }

    fn rollback(mut self) -> Result<()> {
        self.finished = true;
        self.ops.clear();
        Ok(())
    }

    fn set_isolation(&mut self, level: storage_traits::IsolationLevel) -> Result<()> {
        match level {
            storage_traits::IsolationLevel::ReadCommitted => Ok(()),
            _ => Err(Error::Unsupported("only ReadCommitted is supported")),
        }
    }
}
