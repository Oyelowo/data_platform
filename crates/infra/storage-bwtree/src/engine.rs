//! Bw-Tree engine implementation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;

use crate::cursor::BwTreeCursor;
use crate::error::{Error, Result};
use crate::mapping_table::MappingTable;
use crate::options::BwTreeOptions;
use crate::overflow::OverflowStore;
use crate::page::{NULL_PID, Pid};
use crate::recovery::{self, Meta};
use crate::transaction::{BwTreeTransaction, WriteOp};
use crate::tree::Tree;
use crate::wal_record::WalRecord;

/// Persistent latch-free Bw-Tree key-value engine.
pub struct BwTreeEngine {
    inner: Arc<EngineInner>,
}

impl BwTreeEngine {
    /// Open or create a Bw-Tree engine at `path`.
    pub fn open(path: impl AsRef<Path>, options: BwTreeOptions) -> Result<Self> {
        let options = options.validate()?;
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;

        let meta = recovery::read_meta(&path)?;
        let mapping_table = Arc::new(MappingTable::new());
        if let Some(ref m) = meta {
            // Restore high-water PID. Root is rebuilt from WAL replay.
            mapping_table.reserve_next_pid(m.next_pid);
        }
        let overflow = Arc::new(OverflowStore::open(&path)?);
        let tree = Tree::new(Arc::clone(&mapping_table), Arc::clone(&overflow), &options);
        let wal_dir = path.join("wal");
        let wal = storage_wal::Wal::open(
            &wal_dir,
            storage_wal::WalOptions {
                segment_size: (options.page_size as u64 * 256).max(2 * 1024 * 1024),
                ..Default::default()
            },
        )?;

        let root = replay_wal(&wal, &tree, meta.as_ref().map_or(0, |m| m.wal_lsn))?;

        Ok(Self {
            inner: Arc::new(EngineInner {
                path,
                options,
                mapping_table,
                tree,
                root: AtomicU64::new(root),
                overflow,
                wal,
            }),
        })
    }

    /// Return the engine options.
    pub fn options(&self) -> &BwTreeOptions {
        &self.inner.options
    }
}

impl storage_traits::Engine for BwTreeEngine {
    type Error = Error;
    type Transaction = BwTreeTransaction;
    type Cursor = BwTreeCursor;

    fn name(&self) -> &'static str {
        "storage-bwtree"
    }

    fn begin(&self, opts: storage_traits::TxnOptions) -> Result<Self::Transaction> {
        let snapshot_root = self.inner.root.load(Ordering::Acquire);
        Ok(BwTreeTransaction::new(
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
        BwTreeCursor::new(
            Arc::clone(&self.inner),
            root,
            start.map(Bytes::copy_from_slice),
            end.map(Bytes::copy_from_slice),
        )
    }

    fn stats(&self) -> Result<storage_traits::EngineStats> {
        let meta_size = std::fs::metadata(self.inner.path.join("META"))
            .map(|m| m.len())
            .unwrap_or(0);
        let overflow_size = std::fs::metadata(self.inner.path.join("overflow.dat"))
            .map(|m| m.len())
            .unwrap_or(0);
        let wal_size = std::fs::read_dir(self.inner.path.join("wal"))?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok().map(|m| m.len()))
            .sum::<u64>();
        Ok(storage_traits::EngineStats {
            name: self.name(),
            disk_bytes: meta_size + overflow_size + wal_size,
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
pub(crate) struct EngineInner {
    pub path: PathBuf,
    pub options: BwTreeOptions,
    pub mapping_table: Arc<MappingTable>,
    pub tree: Tree,
    pub root: AtomicU64,
    pub overflow: Arc<OverflowStore>,
    pub wal: storage_wal::Wal,
}

impl EngineInner {
    /// Apply buffered transaction operations durably.
    pub(crate) fn apply_ops(&self, ops: &[WriteOp]) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }

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
            let lsn = self.wal.append(record.encode(), storage_wal::Durability::Immediate)?;
            match op {
                WriteOp::Put { key, value } => {
                    root = self.tree.insert(root, key, value, lsn)?;
                }
                WriteOp::Delete { key } => {
                    root = self.tree.delete(root, key, lsn)?;
                }
            }
        }

        self.root.store(root, Ordering::Release);
        Ok(())
    }

    pub(crate) fn sync(&self) -> Result<()> {
        self.overflow.sync()?;
        let root = self.root.load(Ordering::Acquire);
        let meta = Meta {
            root_pid: root,
            next_pid: self.mapping_table.next_pid(),
            wal_lsn: 0,
        };
        recovery::write_meta(&self.path, &meta)?;
        // WAL records are already durable because each append uses
        // `Durability::Immediate`. In the first version the WAL is not
        // truncated because there is no mapping-table checkpoint.
        Ok(())
    }
}

impl Drop for EngineInner {
    fn drop(&mut self) {
        let _ = self.sync();
    }
}

fn replay_wal(wal: &storage_wal::Wal, tree: &Tree, start_lsn: u64) -> Result<Pid> {
    let mut root = NULL_PID;
    for record in wal.iter(start_lsn)? {
        let record = record?;
        let op = WalRecord::decode(&record.payload)?;
        match op {
            WalRecord::Put { key, value } => {
                root = tree.insert(root, &key, &value, record.lsn)?;
            }
            WalRecord::Delete { key } => {
                root = tree.delete(root, &key, record.lsn)?;
            }
        }
    }
    Ok(root)
}
