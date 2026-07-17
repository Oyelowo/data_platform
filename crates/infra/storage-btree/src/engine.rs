//! B+ tree engine implementation.

use std::collections::HashMap;
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

/// RAII handle that pins a snapshot root so GC cannot reclaim pages reachable
/// from it.
///
/// On creation the root's reference count in `BtreeEngineInner::active_roots` is
/// incremented; on drop it is decremented and removed when it reaches zero.
pub(crate) struct SnapshotGuard {
    inner: Arc<BtreeEngineInner>,
    root: crate::page::PageId,
}

impl SnapshotGuard {
    pub(crate) fn new(inner: Arc<BtreeEngineInner>, root: crate::page::PageId) -> Self {
        inner.pin_root(root);
        Self { inner, root }
    }
}

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        self.inner.unpin_root(self.root);
    }
}

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
                active_roots: Mutex::new(HashMap::new()),
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
        let (root, _guard) = self.inner.pin_current_root();
        self.inner.tree.check_integrity(root)
    }

    /// Run garbage collection: reclaim page ids that are no longer reachable
    /// from the current root or any pinned snapshot.
    ///
    /// This is also called automatically by `sync()` after the checkpoint. The
    /// operation is serialized with writers via the engine's writer lock.
    pub fn compact(&self) -> Result<()> {
        self.inner.compact()
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
        let (snapshot_root, guard) = self.inner.pin_current_root();
        Ok(BtreeTransaction::new(
            Arc::clone(&self.inner),
            opts.read_only,
            snapshot_root,
            guard,
        ))
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        let (root, _guard) = self.inner.pin_current_root();
        self.inner.tree.get(root, key)
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor> {
        let (root, guard) = self.inner.pin_current_root();
        BtreeCursor::new(
            Arc::clone(&self.inner),
            root,
            guard,
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
        metrics.insert(
            "storage_btree.freelist_pages".to_string(),
            self.inner.pager.freelist_count() as u64,
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
    /// Roots currently pinned by active readers, transactions, or cursors.
    /// A non-zero count prevents `compact()` from reclaiming pages reachable
    /// from that root.
    pub active_roots: Mutex<HashMap<crate::page::PageId, usize>>,
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

    /// Pin a snapshot root so GC does not reclaim pages reachable from it.
    pub(crate) fn pin_root(&self, root: crate::page::PageId) {
        if root == crate::page::NULL_PAGE_ID {
            return;
        }
        *self.active_roots.lock().entry(root).or_insert(0) += 1;
    }

    /// Unpin a snapshot root previously pinned by `pin_root`.
    pub(crate) fn unpin_root(&self, root: crate::page::PageId) {
        if root == crate::page::NULL_PAGE_ID {
            return;
        }
        let mut roots = self.active_roots.lock();
        if let std::collections::hash_map::Entry::Occupied(mut entry) = roots.entry(root) {
            *entry.get_mut() -= 1;
            if *entry.get() == 0 {
                entry.remove();
            }
        }
    }

    /// Observe the current root, pin it, and verify the pin succeeded.
    ///
    /// See `GC_DESIGN.md` for the correctness argument for the optimistic
    /// load-pin-verify loop.
    pub(crate) fn pin_current_root(self: &Arc<Self>) -> (crate::page::PageId, SnapshotGuard) {
        loop {
            let root = self.root.load(Ordering::Acquire);
            let guard = SnapshotGuard::new(Arc::clone(self), root);
            let current = self.root.load(Ordering::Acquire);
            if current == root {
                return (root, guard);
            }
        }
    }

    /// Reclaim page ids that are no longer reachable from the current root or
    /// any pinned snapshot.
    ///
    /// This method is serialized with writers via `write_lock`. It briefly holds
    /// `active_roots` during the reclamation phase so that no reader can pin a
    /// newly discovered old root while ids are being moved to the freelist.
    fn compact(&self) -> Result<()> {
        let _writer_guard = self.write_lock.lock();
        let current_root = self.root.load(Ordering::Acquire);

        // Collect pinned roots without holding the lock for the expensive walk.
        let pinned: Vec<crate::page::PageId> = {
            let roots = self.active_roots.lock();
            roots.keys().copied().collect()
        };

        let mut live = std::collections::HashSet::new();
        if current_root != crate::page::NULL_PAGE_ID {
            live.extend(self.tree.reachable_pages(current_root)?);
        }
        for root in pinned {
            if root != current_root && root != crate::page::NULL_PAGE_ID && !live.contains(&root) {
                live.extend(self.tree.reachable_pages(root)?);
            }
        }

        // Reclaim unreachable retired pages. Hold active_roots so readers cannot
        // pin a new old root between the live-set computation and the move to
        // freelist. The loop handles roots pinned while we were walking.
        {
            let roots = self.active_roots.lock();
            loop {
                let extra: Vec<crate::page::PageId> = roots
                    .keys()
                    .copied()
                    .filter(|r| *r != crate::page::NULL_PAGE_ID && !live.contains(r))
                    .collect();
                if extra.is_empty() {
                    break;
                }
                for root in extra {
                    live.extend(self.tree.reachable_pages(root)?);
                }
            }

            self.pager.reclaim_retired(&live)?;
        }
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
        // GC is safe after the checkpoint: the checkpoint wrote the current root,
        // so any pages it reclaims were not reachable from that committed state.
        self.compact()?;
        // Remove completed WAL segments. The active segment is kept open by the
        // committer; truncating it would lose subsequent writes.
        self.wal.truncate_completed()?;
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
