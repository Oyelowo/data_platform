//! Public `storage_traits::Engine` implementation for the in-place B+ tree.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;

use crate::buffer::BufferPool;
use crate::checkpoint::{Checkpoint, CheckpointOptions, CheckpointThread, Meta};
use crate::cleaner::PageCleaner;
use crate::cursor::BPlusTreeCursor;
use crate::disk::PagedFile;
use crate::error::{Error, Result};
use crate::options::BtreeOptions;
use crate::recovery::Recovery;
use crate::space::PageAllocator;
use crate::sync::Mutex as SyncMutex;
use crate::transaction::BtreeTransaction;
use crate::tree::BPlusTree;
use crate::txn::{NULL_TXN_ID, Transaction as V2Transaction};
use crate::valuelog::ValueLog;
use crate::wal::{NULL_LSN, WalLog};

/// Persistent in-place B+ tree key-value engine.
pub struct BtreeEngine {
    inner: Arc<BtreeEngineInner>,
}

impl BtreeEngine {
    /// Open or create a B+ tree engine at `path`.
    pub fn open(path: impl AsRef<Path>, options: BtreeOptions) -> Result<Self> {
        let options = options.validate()?;
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;

        let page_file = PagedFile::open(path.join("pages.dat"), options.page_size)?;
        let disk = Arc::new(page_file);

        // Read the latest checkpoint, if any, so the allocator starts from the
        // correct state.
        let meta = Meta::read(&path)?;
        let allocator = Arc::new(SyncMutex::new(if let Some(ref m) = meta {
            m.allocator.clone()
        } else {
            PageAllocator::new(1)
        }));

        let pool = Arc::new(BufferPool::new(
            options.cache_frames(),
            options.page_size,
            disk,
            Arc::clone(&allocator),
        )?);

        let wal = Arc::new(WalLog::open_with_fault_config(
            &path,
            options.wal_options(),
            options.wal_fault_config.clone(),
        )?);
        // Use buffered value-log appends: the engine calls sync at operation
        // boundaries, so we pay one fsync per commit instead of one per value.
        let value_log = Arc::new(ValueLog::open_with_durability(
            &path,
            crate::valuelog::Durability::Buffered,
        )?);

        let (recovery_root, checkpoint_lsn) = match meta {
            Some(m) => (m.root_page_id, m.checkpoint_lsn),
            None => {
                // Fresh database: allocate an empty leaf root and recover from
                // the beginning of the (empty) WAL.
                let root_id = pool.with_new_page_mut(|page| {
                    page.set_leaf();
                    Ok(page.id)
                })?;
                (root_id, NULL_LSN)
            }
        };

        let recovered_root = Recovery::new(Arc::clone(&pool), Arc::clone(&wal), recovery_root)
            .with_value_log(Arc::clone(&value_log))
            .recover(checkpoint_lsn)?;

        let mut tree = BPlusTree::open(
            Arc::clone(&pool),
            recovered_root,
            options.inline_threshold(),
        )
        .with_wal(Arc::clone(&wal))
        .with_value_log(Arc::clone(&value_log));
        tree.set_min_cells(options.min_cells());
        let tree = Arc::new(tree);

        let checkpoint = Checkpoint::new_with_tree(
            &path,
            Arc::clone(&pool),
            Arc::clone(&wal),
            Arc::clone(&tree),
            Arc::clone(&allocator),
        );

        let bg_handle = SyncMutex::new(None);
        let cleaner_handle = SyncMutex::new(None);
        let inner = Arc::new(BtreeEngineInner {
            path,
            options,
            tree,
            wal,
            value_log,
            pool,
            allocator,
            checkpoint,
            bg_handle,
            cleaner_handle,
        });

        inner.start_background_cleaner()?;
        inner.start_background_checkpoint()?;

        Ok(Self { inner })
    }

    /// Return the engine options.
    pub fn options(&self) -> &BtreeOptions {
        &self.inner.options
    }

    /// Validate the structural integrity of the on-disk B+ tree.
    pub fn check_integrity(&self) -> Result<()> {
        self.inner.tree.check_integrity()
    }

    /// Validate structural integrity and every value-log reference.
    pub fn check_integrity_with_value_log(&self) -> Result<()> {
        self.inner.tree.check_integrity_with_value_log()
    }

    /// Reclaim page ids that are no longer reachable from the current root or
    /// any pinned snapshot.
    pub fn compact(&self) -> Result<()> {
        self.inner.tree.compact()
    }

    /// Compact the value log and update all leaf-cell references.
    ///
    /// This is a stop-the-world operation and should be scheduled explicitly.
    pub fn compact_value_log(&self) -> Result<HashMap<(u64, u32), u64>> {
        self.inner.tree.compact_value_log()
    }

    /// Flush all durable state to stable storage and write a checkpoint.
    ///
    /// The background checkpoint thread is paused while the manual checkpoint
    /// runs and restarted afterwards if one is configured.
    pub fn close(&self) -> Result<()> {
        self.inner.stop_background_cleaner()?;
        self.inner.stop_background_checkpoint()?;
        self.inner.checkpoint()?;
        self.inner.tree.compact()?;
        self.inner.value_log.close()?;
        self.inner.wal.close()?;
        Ok(())
    }
}

impl storage_traits::Engine for BtreeEngine {
    type Error = Error;
    type Transaction = BtreeTransaction;
    type Cursor = BPlusTreeCursor;

    fn name(&self) -> &'static str {
        "storage-btree"
    }

    fn begin(&self, opts: storage_traits::TxnOptions) -> Result<Self::Transaction> {
        if opts.read_only {
            if opts.isolation != storage_traits::IsolationLevel::Snapshot {
                return Err(Error::Unsupported(
                    "read-only transactions must use Snapshot isolation",
                ));
            }
        } else if !matches!(
            opts.isolation,
            storage_traits::IsolationLevel::ReadCommitted
                | storage_traits::IsolationLevel::Snapshot
                | storage_traits::IsolationLevel::RepeatableRead
        ) {
            return Err(Error::Unsupported(
                "write transactions must use ReadCommitted or Snapshot isolation",
            ));
        }
        BtreeTransaction::new(
            Arc::clone(&self.inner.tree),
            self.inner.options.clone(),
            opts.read_only,
            opts.isolation,
        )
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.inner.tree.get(key).map(|v| v.map(Bytes::from))
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor> {
        // Construct a transient snapshot transaction so the cursor gets a stable
        // read timestamp without registering in the transaction table. The
        // cursor only copies the timestamp and txn id, so the transient handle
        // can be dropped immediately.
        let ts = self.inner.tree.current_timestamp();
        let read_ts = ts.saturating_sub(1);
        let txn = V2Transaction::new(
            NULL_TXN_ID,
            read_ts,
            crate::txn::IsolationLevel::ReadCommitted,
        );
        BPlusTreeCursor::new(Arc::clone(&self.inner.tree), &txn, start, end)
    }

    fn stats(&self) -> Result<storage_traits::EngineStats> {
        let mut disk_bytes = 0u64;
        for filename in &["pages.dat", "values.log", "META"] {
            if let Ok(meta) = std::fs::metadata(self.inner.path.join(filename)) {
                disk_bytes += meta.len();
            }
        }
        let wal_dir = self.inner.path.join("wal");
        if let Ok(entries) = std::fs::read_dir(&wal_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    disk_bytes += meta.len();
                }
            }
        }

        let memory_bytes =
            self.inner.options.cache_frames() as u64 * self.inner.options.page_size as u64;

        let mut metrics = HashMap::new();
        metrics.insert(
            "storage_btree.retired_pages".to_string(),
            self.inner.tree.retired_count() as u64,
        );
        metrics.insert(
            "storage_btree.freelist_pages".to_string(),
            self.inner.allocator.with_mut(|a| a.reusable_count() as u64),
        );
        metrics.insert("storage_btree.cache_memory_bytes".to_string(), memory_bytes);
        if let Ok(meta) = std::fs::metadata(self.inner.path.join("values.log")) {
            metrics.insert("storage_btree.value_log_bytes".to_string(), meta.len());
        }

        Ok(storage_traits::EngineStats {
            name: self.name(),
            disk_bytes,
            memory_bytes,
            num_keys: None,
            metrics,
        })
    }

    fn sync(&self) -> Result<()> {
        self.inner.stop_background_cleaner()?;
        self.inner.stop_background_checkpoint()?;
        self.inner.checkpoint()?;
        self.inner.tree.compact()?;
        self.inner.start_background_cleaner()?;
        self.inner.start_background_checkpoint()
    }
}

impl Drop for BtreeEngine {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

pub(crate) struct BtreeEngineInner {
    pub path: PathBuf,
    pub options: BtreeOptions,
    pub tree: Arc<BPlusTree>,
    pub wal: Arc<WalLog>,
    pub value_log: Arc<ValueLog>,
    pub pool: Arc<BufferPool>,
    pub allocator: Arc<SyncMutex<PageAllocator>>,
    pub checkpoint: Checkpoint,
    pub bg_handle: SyncMutex<Option<CheckpointThread>>,
    pub cleaner_handle: SyncMutex<Option<PageCleaner>>,
}

impl BtreeEngineInner {
    /// Run a fuzzy checkpoint and truncate completed WAL segments.
    fn checkpoint(&self) -> Result<Meta> {
        // Make all large values durable before recording the checkpoint LSN.
        self.value_log.sync()?;
        self.checkpoint.run()
    }

    fn start_background_checkpoint(&self) -> Result<()> {
        let interval = match self.options.background_checkpoint_interval {
            Some(d) if !d.is_zero() => d,
            _ => return Ok(()),
        };

        self.bg_handle.with_mut(|guard| {
            if guard.is_some() {
                return Ok(());
            }

            let handle = CheckpointThread::spawn(
                self.checkpoint.clone(),
                Arc::clone(&self.tree),
                CheckpointOptions {
                    interval,
                    run_integrity_check: self.options.checkpoint_integrity_check,
                    compact_value_log: self.options.checkpoint_compact_value_log,
                },
            );
            *guard = Some(handle);
            Ok(())
        })
    }

    fn stop_background_checkpoint(&self) -> Result<()> {
        self.bg_handle.with_mut(|guard| {
            if let Some(handle) = guard.take() {
                handle.stop()?;
            }
            Ok(())
        })
    }

    fn start_background_cleaner(&self) -> Result<()> {
        let interval = match self.options.background_page_cleaner_interval {
            Some(d) if !d.is_zero() => d,
            _ => return Ok(()),
        };

        self.cleaner_handle.with_mut(|guard| {
            if guard.is_some() {
                return Ok(());
            }

            let handle = PageCleaner::spawn(Arc::clone(&self.pool), interval);
            *guard = Some(handle);
            Ok(())
        })
    }

    fn stop_background_cleaner(&self) -> Result<()> {
        self.cleaner_handle.with_mut(|guard| {
            if let Some(handle) = guard.take() {
                handle.stop()?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[cfg(not(miri))]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use storage_traits::{Cursor, Engine, Transaction};

    use super::*;

    fn make_engine() -> (BtreeEngine, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let engine = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
        (engine, dir)
    }

    #[test]
    fn open_creates_fresh_engine() {
        let (engine, _dir) = make_engine();
        assert_eq!(engine.get(b"x").unwrap(), None);
        engine.check_integrity().unwrap();
    }

    #[test]
    fn autocommit_put_and_get() {
        let (engine, _dir) = make_engine();
        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        txn.put(b"hello", b"world").unwrap();
        txn.commit().unwrap();

        assert_eq!(
            engine.get(b"hello").unwrap(),
            Some(Bytes::from_static(b"world"))
        );
        engine.check_integrity().unwrap();
    }

    #[test]
    fn scan_without_txn_sees_committed_data() {
        let (engine, _dir) = make_engine();
        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        for i in 0u64..5 {
            let key = format!("{:02x}", i);
            txn.put(key.as_bytes(), b"v").unwrap();
        }
        txn.commit().unwrap();

        let mut cursor = engine.scan(None, None).unwrap();
        let all = cursor.next_batch(100).unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn reopen_recovers_committed_data() {
        let (engine, dir) = make_engine();
        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        txn.put(b"k", b"v").unwrap();
        txn.commit().unwrap();
        engine.sync().unwrap();
        drop(engine);

        let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
        assert_eq!(engine2.get(b"k").unwrap(), Some(Bytes::from_static(b"v")));
        engine2.check_integrity().unwrap();
    }

    #[test]
    fn large_value_goes_to_value_log() {
        let (engine, _dir) = make_engine();
        let value = vec![b'x'; 4096];
        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        txn.put(b"big", &value).unwrap();
        txn.commit().unwrap();

        assert_eq!(
            engine.get(b"big").unwrap(),
            Some(Bytes::from(value.clone()))
        );
        engine.check_integrity_with_value_log().unwrap();
    }

    #[test]
    fn stats_account_for_files() {
        let (engine, _dir) = make_engine();
        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        txn.put(b"a", &vec![b'x'; 4096]).unwrap();
        txn.commit().unwrap();
        engine.sync().unwrap();

        let stats = engine.stats().unwrap();
        assert_eq!(stats.name, "storage-btree");
        assert!(stats.disk_bytes > 0);
        assert!(stats.memory_bytes > 0);
        assert!(
            stats
                .metrics
                .contains_key("storage_btree.cache_memory_bytes")
        );
    }

    #[test]
    fn background_cleaner_flushes_pages() {
        let dir = tempfile::tempdir().unwrap();
        let options = BtreeOptions {
            background_page_cleaner_interval: Some(Duration::from_millis(50)),
            ..Default::default()
        };
        let engine = BtreeEngine::open(dir.path(), options).unwrap();

        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        txn.put(b"k", b"v").unwrap();
        txn.commit().unwrap();

        // Wait for the cleaner to flush dirty pages without an explicit sync.
        std::thread::sleep(Duration::from_millis(200));
        drop(engine);

        // Reopen and verify durability.
        let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
        assert_eq!(engine2.get(b"k").unwrap(), Some(Bytes::from_static(b"v")));
        engine2.check_integrity().unwrap();
    }

    #[test]
    fn wal_fsync_failure_makes_engine_unwritable() {
        let dir = tempfile::tempdir().unwrap();
        let options = BtreeOptions {
            wal_fault_config: Some(storage_wal::FaultConfig {
                fail_sync_every: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        };
        let engine = BtreeEngine::open(dir.path(), options).unwrap();

        // The first commit triggers an fsync, which is injected to fail.
        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        txn.put(b"k", b"v").unwrap();
        assert!(
            txn.commit().is_err(),
            "fsync failure should fail the commit"
        );

        // The engine is left in a failed state; close tolerates errors.
        let _ = engine.close();
    }

    #[test]
    fn buffered_records_lost_without_sync_are_consistent_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let options = BtreeOptions {
            wal_fault_config: Some(storage_wal::FaultConfig {
                drop_appends: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let engine = BtreeEngine::open(dir.path(), options).unwrap();

        // This commit returns Ok because the records are buffered, but the
        // bytes are dropped by the injector. We then "crash" by forgetting the
        // engine without running close()/checkpoint, exactly like a power loss.
        let mut txn = engine
            .begin(storage_traits::TxnOptions {
                read_only: false,
                isolation: storage_traits::IsolationLevel::Snapshot,
            })
            .unwrap();
        txn.put(b"k", b"v").unwrap();
        txn.commit().unwrap();
        std::mem::forget(engine);

        // Reopen: the lost transaction must not appear, and the tree must be
        // structurally consistent.
        let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
        assert_eq!(engine2.get(b"k").unwrap(), None);
        engine2.check_integrity().unwrap();
    }
}
