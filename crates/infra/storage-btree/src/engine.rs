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
}

impl storage_traits::Engine for BtreeEngine {
    type Error = Error;
    type Transaction = BtreeTransaction;
    type Cursor = BtreeCursor;

    fn name(&self) -> &'static str {
        "storage-btree"
    }

    fn begin(&self, opts: storage_traits::TxnOptions) -> Result<Self::Transaction> {
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
        let file = std::fs::metadata(self.inner.path.join("pages.dat"))?;
        Ok(storage_traits::EngineStats {
            name: self.name(),
            disk_bytes: file.len(),
            memory_bytes: 0,
            num_keys: None,
            metrics: std::collections::HashMap::new(),
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
    pub(crate) fn apply_ops(&self, ops: &[WriteOp]) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        let _guard = self.write_lock.lock();

        let mut root = self.root.load(Ordering::Acquire);
        for op in ops {
            let record = match op {
                WriteOp::Put { key, value } => WalRecord::Put {
                    key: Bytes::copy_from_slice(key),
                    value: Bytes::copy_from_slice(value),
                },
                WriteOp::Delete { key } => WalRecord::Delete {
                    key: Bytes::copy_from_slice(key),
                },
            };
            self.wal
                .append(record.encode(), storage_wal::Durability::Immediate)?;
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
        // The operations are already durable because each WAL append uses
        // `Durability::Immediate`. An explicit `sync()` writes the checkpoint
        // `META` and truncates the WAL.
        Ok(())
    }

    pub(crate) fn sync(&self) -> Result<()> {
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
        self.wal.truncate_before(u64::MAX)?;
        Ok(())
    }
}

impl Drop for BtreeEngineInner {
    fn drop(&mut self) {
        let _ = self.sync();
    }
}
