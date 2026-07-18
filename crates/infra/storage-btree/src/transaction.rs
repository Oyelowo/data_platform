//! Public transaction wrapper for the in-place B+ tree engine.

use std::sync::Arc;

use bytes::Bytes;

use crate::cursor::BPlusTreeCursor;
use crate::error::{Error, Result};
use crate::options::BtreeOptions;
use crate::tree::BPlusTree;
use crate::txn::{IsolationLevel as V2IsolationLevel, Transaction as V2Transaction};

/// A multi-record transaction against a v2 B+ tree engine.
pub struct BtreeTransaction {
    tree: Arc<BPlusTree>,
    txn: V2Transaction,
    options: BtreeOptions,
    read_only: bool,
    finished: bool,
    /// Number of mutating operations performed so far.
    op_count: usize,
}

impl BtreeTransaction {
    /// Begin a new transaction on `tree` with the requested options.
    pub(crate) fn new(
        tree: Arc<BPlusTree>,
        options: BtreeOptions,
        read_only: bool,
        isolation: storage_traits::IsolationLevel,
    ) -> Result<Self> {
        let v2_isolation = map_isolation(isolation)?;
        if read_only && isolation != storage_traits::IsolationLevel::Snapshot {
            return Err(Error::Unsupported(
                "read-only transactions must use Snapshot isolation",
            ));
        }
        let txn = tree.begin_txn(v2_isolation)?;
        Ok(Self {
            tree,
            txn,
            options,
            read_only,
            finished: false,
            op_count: 0,
        })
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

    fn check_value_size(&self, value: &[u8]) -> Result<()> {
        if value.len() > self.options.max_value_size {
            return Err(Error::OutOfBounds {
                kind: crate::error::BoundKind::Value,
                limit: self.options.max_value_size,
                got: value.len(),
            });
        }
        Ok(())
    }

    fn check_op_limit_at_commit(&self) -> Result<()> {
        if self.op_count > self.options.max_batch_ops {
            return Err(Error::OutOfBounds {
                kind: crate::error::BoundKind::Batch,
                limit: self.options.max_batch_ops,
                got: self.op_count,
            });
        }
        Ok(())
    }
}

impl storage_traits::Transaction for BtreeTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        self.tree
            .get_txn(&self.txn, key)
            .map(|v| v.map(Bytes::from))
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.check_write()?;
        self.check_value_size(value)?;
        self.tree.insert_txn(&self.txn, key, value)?;
        self.op_count += 1;
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.check_write()?;
        self.tree.delete_txn(&self.txn, key)?;
        self.op_count += 1;
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> std::result::Result<impl storage_traits::Cursor<Error = Self::Error>, Self::Error> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        BPlusTreeCursor::new(self.tree.clone(), &self.txn, start, end)
    }

    fn commit(mut self) -> Result<()> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        self.check_op_limit_at_commit()?;
        self.finished = true;
        self.tree.commit_txn(&self.txn)?;
        Ok(())
    }

    fn rollback(mut self) -> Result<()> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        self.finished = true;
        self.tree.rollback_txn(&self.txn)
    }

    fn set_isolation(&mut self, level: storage_traits::IsolationLevel) -> Result<()> {
        if self.finished {
            return Err(Error::TxnFinished);
        }
        let mapped = map_isolation(level)?;
        self.txn.isolation = mapped;
        Ok(())
    }
}

fn map_isolation(level: storage_traits::IsolationLevel) -> Result<V2IsolationLevel> {
    match level {
        storage_traits::IsolationLevel::ReadCommitted => Ok(V2IsolationLevel::ReadCommitted),
        storage_traits::IsolationLevel::Snapshot
        | storage_traits::IsolationLevel::RepeatableRead => Ok(V2IsolationLevel::Snapshot),
        storage_traits::IsolationLevel::Serializable => Err(Error::Unsupported(
            "Serializable isolation is not supported; use Snapshot",
        )),
        storage_traits::IsolationLevel::ReadUncommitted => Err(Error::Unsupported(
            "ReadUncommitted isolation is not supported; use ReadCommitted",
        )),
    }
}

#[cfg(test)]
mod tests {
    use storage_traits::Transaction;

    use super::*;
    use crate::buffer::BufferPool;
    use crate::disk::PagedFile;
    use crate::page::PageId;
    use crate::space::PageAllocator;
    use crate::sync::Mutex as SyncMutex;

    fn make_tree() -> (Arc<BPlusTree>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(64, 512, disk, alloc).unwrap());
        let wal = Arc::new(
            crate::wal::WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap(),
        );
        let mut tree = BPlusTree::new(pool, 64).unwrap().with_wal(wal);
        tree.set_min_cells(1);
        (Arc::new(tree), dir)
    }

    #[test]
    fn txn_read_your_writes() {
        let (tree, _dir) = make_tree();
        let mut txn = BtreeTransaction::new(
            tree.clone(),
            BtreeOptions::default(),
            false,
            storage_traits::IsolationLevel::Snapshot,
        )
        .unwrap();
        txn.put(b"a", b"1").unwrap();
        assert_eq!(txn.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        txn.commit().unwrap();
        assert_eq!(
            tree.get(b"a").unwrap(),
            Some(b"1".to_vec()),
            "committed write must be visible"
        );
    }

    #[test]
    fn txn_rollback_discards_writes() {
        let (tree, _dir) = make_tree();
        tree.insert(b"a", b"old").unwrap();
        let mut txn = BtreeTransaction::new(
            tree.clone(),
            BtreeOptions::default(),
            false,
            storage_traits::IsolationLevel::Snapshot,
        )
        .unwrap();
        txn.put(b"a", b"new").unwrap();
        txn.rollback().unwrap();
        assert_eq!(tree.get(b"a").unwrap(), Some(b"old".to_vec()));
    }

    #[test]
    fn read_only_txn_rejects_writes() {
        let (tree, _dir) = make_tree();
        let mut txn = BtreeTransaction::new(
            tree.clone(),
            BtreeOptions::default(),
            true,
            storage_traits::IsolationLevel::Snapshot,
        )
        .unwrap();
        assert!(txn.put(b"a", b"1").is_err());
    }

    #[test]
    fn value_size_limit_enforced() {
        let (tree, _dir) = make_tree();
        let options = BtreeOptions {
            max_value_size: 4,
            ..Default::default()
        };
        let mut txn = BtreeTransaction::new(
            tree.clone(),
            options,
            false,
            storage_traits::IsolationLevel::Snapshot,
        )
        .unwrap();
        assert!(txn.put(b"a", b"12345").is_err());
        txn.put(b"a", b"1234").unwrap();
    }
}
