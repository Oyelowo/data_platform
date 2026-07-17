//! B+ tree engine implementation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use parking_lot::Mutex;

use crate::cursor::BtreeCursor;
use crate::error::{Error, Result};
use crate::options::BtreeOptions;
use crate::pager::Pager;
use crate::recovery::{self, Meta};
use crate::transaction::{BtreeTransaction, WriteOp};
use crate::tree::Tree;
use crate::wal_record::WalRecord;

/// Persistent B+ tree key-value engine.
pub struct BtreeEngine {
    inner: Arc<BtreeEngineInner>,
}

impl BtreeEngine {
    /// Open or create a B+ tree engine at `path`.
    pub fn open(path: impl AsRef<Path>, options: BtreeOptions) -> Result<Self> {
        let options = options.validate()?;
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;

        let pager = Arc::new(Pager::open(&path, options.page_size, options.cache_size)?);
        let wal_dir = path.join("wal");
        let wal = storage_wal::Wal::open(
            &wal_dir,
            storage_wal::WalOptions {
                // Segment must be large enough for the largest value the engine
                // is expected to store (the test suite writes 1 MiB values).
                segment_size: (options.page_size as u64 * 256).max(2 * 1024 * 1024),
                ..Default::default()
            },
        )?;

        let root = recovery::recover(&path, &options, Arc::clone(&pager), &wal)?;
        let tree = Tree::new(Arc::clone(&pager), &options);

        Ok(Self {
            inner: Arc::new(BtreeEngineInner {
                path,
                options,
                pager,
                tree,
                root: AtomicU64::new(root),
                write_lock: Mutex::new(()),
                wal,
            }),
        })
    }

    /// Return the engine options.
    pub fn options(&self) -> &BtreeOptions {
        &self.inner.options
    }

    /// Validate the structural integrity of the on-disk B+ tree.
    ///
    /// This checks page checksums, key ordering, separator correctness, and
    /// overflow chain integrity for all pages reachable from the current root.
    /// It is intended for tests, health checks, and after crash recovery.
    pub fn check_integrity(&self) -> Result<()> {
        let root = self.inner.root.load(Ordering::Acquire);
        self.inner.tree.check_integrity(root)
    }

    /// Flush all dirty pages and write a durable checkpoint.
    ///
    /// This lets callers observe checkpoint errors. `Drop` also attempts a
    /// best-effort checkpoint, but its result is ignored.
    pub fn close(&self) -> Result<()> {
        self.inner.checkpoint()
    }
}

impl storage_traits::Engine for BtreeEngine {
    type Error = Error;
    type Transaction = BtreeTransaction;
    type Cursor = BtreeCursor;

    fn name(&self) -> &'static str {
        "storage-btree"
    }

    fn begin(&self, opts: storage_traits::TxnOptions) -> Result<Self::Transaction> {
        if !opts.read_only && opts.isolation != storage_traits::IsolationLevel::ReadCommitted {
            return Err(Error::Unsupported("only ReadCommitted is supported"));
        }
        let snapshot_root = self.inner.root.load(Ordering::Acquire);
        Ok(BtreeTransaction::new(
            Arc::clone(&self.inner),
            opts.read_only,
            snapshot_root,
        ))
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        let root = self.inner.root.load(Ordering::Acquire);
        self.inner.tree.get(root, key)
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor> {
        let root = self.inner.root.load(Ordering::Acquire);
        BtreeCursor::new(
            Arc::clone(&self.inner),
            root,
            start.map(Bytes::copy_from_slice),
            end.map(Bytes::copy_from_slice),
        )
    }

    fn stats(&self) -> Result<storage_traits::EngineStats> {
        let mut disk_bytes = 0u64;
        if let Ok(meta) = std::fs::metadata(self.inner.path.join("pages.dat")) {
            disk_bytes += meta.len();
        }
        if let Ok(meta) = std::fs::metadata(self.inner.path.join("META")) {
            disk_bytes += meta.len();
        }
        let wal_dir = self.inner.path.join("wal");
        if let Ok(entries) = std::fs::read_dir(&wal_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    disk_bytes += meta.len();
                }
            }
        }

        let memory_bytes = self.inner.pager.approx_memory_bytes();
        let mut metrics = std::collections::HashMap::new();
        metrics.insert(
            "storage_btree.retired_pages".to_string(),
            self.inner.pager.retired_count() as u64,
        );
        metrics.insert("storage_btree.cache_memory_bytes".to_string(), memory_bytes);

        Ok(storage_traits::EngineStats {
            name: self.name(),
            disk_bytes,
            memory_bytes,
            num_keys: None,
            metrics,
        })
    }

    fn sync(&self) -> Result<()> {
        self.inner.sync()
    }
}

/// Shared inner state for the engine, transactions, and cursors.
pub(crate) struct BtreeEngineInner {
    pub path: PathBuf,
    pub options: BtreeOptions,
    pub pager: Arc<Pager>,
    pub tree: Tree,
    pub root: AtomicU64,
    pub write_lock: Mutex<()>,
    pub wal: storage_wal::Wal,
}

impl BtreeEngineInner {
    /// Apply buffered transaction operations durably.
    ///
    /// All operations are encoded into a single WAL batch record, fsynced once,
    /// and then applied to the in-memory tree under the writer lock. This gives
    /// atomic durability: a crash before the WAL record is acknowledged leaves
    /// none of the operations committed; a crash after it replays the whole
    /// batch.
    pub(crate) fn apply_ops(&self, ops: &[WriteOp]) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        if ops.len() > self.options.max_batch_ops {
            return Err(Error::OutOfBounds {
                kind: crate::error::BoundKind::Batch,
                limit: self.options.max_batch_ops,
                got: ops.len(),
            });
        }
        let _guard = self.write_lock.lock();

        let record = WalRecord::Batch(
            ops.iter()
                .map(|op| match op {
                    WriteOp::Put { key, value } => crate::wal_record::BatchOp::Put {
                        key: Bytes::copy_from_slice(key),
                        value: Bytes::copy_from_slice(value),
                    },
                    WriteOp::Delete { key } => crate::wal_record::BatchOp::Delete {
                        key: Bytes::copy_from_slice(key),
                    },
                })
                .collect(),
        );
        self.wal
            .append(record.encode(), storage_wal::Durability::Immediate)?;

        let mut root = self.root.load(Ordering::Acquire);
        for op in ops {
            match op {
                WriteOp::Put { key, value } => {
                    root = self.tree.insert(root, key, value)?;
                }
                WriteOp::Delete { key } => {
                    root = self.tree.delete(root, key)?;
                }
            }
        }

        self.root.store(root, Ordering::Release);
        Ok(())
    }

    /// Write a durable checkpoint (page file + META) without truncating the WAL.
    ///
    /// Used by `sync()` and by `Drop`. Keeping the WAL around on drop means a
    /// subsequent open can still recover if the `META` file is lost or stale.
    fn checkpoint(&self) -> Result<()> {
        let _guard = self.write_lock.lock();
        // WAL records are already durable because `append` uses
        // `Durability::Immediate`. We only need to sync the page file and write
        // the atomic metadata checkpoint.
        self.pager.sync()?;

        let root = self.root.load(Ordering::Acquire);
        let (freelist, next_page_id) = self.pager.freelist_snapshot();
        let meta = Meta {
            root,
            freelist,
            next_page_id,
        };
        recovery::write_meta(&self.path, &meta)?;
        Ok(())
    }

    pub(crate) fn sync(&self) -> Result<()> {
        self.checkpoint()?;
        self.wal.truncate_before(u64::MAX)?;
        Ok(())
    }
}

impl Drop for BtreeEngineInner {
    fn drop(&mut self) {
        let _ = self.checkpoint();
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use storage_traits::Engine;

    use crate::wal_record::{BatchOp, WalRecord};
    use crate::{BtreeEngine, BtreeOptions};

    #[test]
    fn recover_wal_batch_replays_all_ops() {
        let dir = tempfile::tempdir().unwrap();
        let wal_dir = dir.path().join("wal");
        let wal = storage_wal::Wal::open(
            &wal_dir,
            storage_wal::WalOptions {
                segment_size: 2 * 1024 * 1024,
                ..Default::default()
            },
        )
        .unwrap();
        let record = WalRecord::Batch(vec![
            BatchOp::Put {
                key: Bytes::from_static(b"a"),
                value: Bytes::from_static(b"1"),
            },
            BatchOp::Put {
                key: Bytes::from_static(b"b"),
                value: Bytes::from_static(b"2"),
            },
            BatchOp::Delete {
                key: Bytes::from_static(b"c"),
            },
        ]);
        wal.append(record.encode(), storage_wal::Durability::Immediate)
            .unwrap();
        wal.close().unwrap();

        let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
        assert_eq!(engine.get(b"a").unwrap(), Some(Bytes::from_static(b"1")));
        assert_eq!(engine.get(b"b").unwrap(), Some(Bytes::from_static(b"2")));
        assert_eq!(engine.get(b"c").unwrap(), None);
        engine.check_integrity().unwrap();
    }

    #[test]
    fn recover_torn_wal_tail() {
        let dir = tempfile::tempdir().unwrap();
        let wal_dir = dir.path().join("wal");
        let wal = storage_wal::Wal::open(
            &wal_dir,
            storage_wal::WalOptions {
                segment_size: 2 * 1024 * 1024,
                ..Default::default()
            },
        )
        .unwrap();
        let record = WalRecord::Batch(vec![BatchOp::Put {
            key: Bytes::from_static(b"a"),
            value: Bytes::from_static(b"1"),
        }]);
        wal.append(record.encode(), storage_wal::Durability::Immediate)
            .unwrap();
        wal.close().unwrap();

        let segment = std::fs::read_dir(&wal_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let len = std::fs::metadata(&segment).unwrap().len();
        if len > 3 {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&segment)
                .unwrap();
            file.set_len(len - 3).unwrap();
        }

        let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
        let value = engine.get(b"a").unwrap();
        assert!(value == Some(Bytes::from_static(b"1")) || value.is_none());
        engine.check_integrity().unwrap();
    }
}
