//! Fuzzy checkpointing for the in-place B+ tree.
//!
//! A checkpoint captures the current root page id, the WAL LSN at which the
//! checkpoint was taken, the oldest LSN still needed for undo, and the page
//! allocator state.  It writes this information atomically to the `META` file
//! so that recovery can start scanning the WAL from `checkpoint_lsn` instead of
//! from the beginning of time.
//!
//! The checkpoint is "fuzzy": writers are not quiesced while dirty pages are
//! flushed.  Pages written during the flush reflect their on-page `page_lsn`;
//! redo during recovery skips records whose LSN is already on the page.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::sync::Mutex as SyncMutex;
use std::thread::JoinHandle;
use std::time::Duration;

use storage_format::{crc32c, read_u32_le, read_u64_le, write_u32_le, write_u64_le};

use crate::buffer::BufferPool;
use crate::error::{Error, Result};
use crate::io::{Boundary, OpenOptions, RealBackend, StorageBackend, StorageFile};
use crate::page::PageId;
use crate::recovery::{ActiveTxn, Recovery};
use crate::space::PageAllocator;
use crate::tree::BPlusTree;
use crate::wal::{Lsn, NULL_LSN, WalLog};

/// On-disk magic for the `META` file.
const META_MAGIC: u32 = 0x42_54_52_45; // "BTRE"
/// On-disk format version.
const META_VERSION: u32 = 1;

/// Metadata required to open the tree and begin recovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// Root page id at the time of the checkpoint.
    pub root_page_id: PageId,
    /// LSN of the checkpoint record; recovery scans from here.
    pub checkpoint_lsn: Lsn,
    /// Oldest LSN that may still be needed for undo.
    pub first_undo_lsn: Lsn,
    /// Allocator state at the time of the checkpoint.
    pub allocator: PageAllocator,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            root_page_id: PageId::new(1),
            checkpoint_lsn: NULL_LSN,
            first_undo_lsn: NULL_LSN,
            allocator: PageAllocator::new(PageId::new(1)),
        }
    }
}

impl Meta {
    /// Path to the `META` file inside `dir`.
    pub fn path(dir: impl AsRef<Path>) -> PathBuf {
        dir.as_ref().join("META")
    }

    /// Path to the `META.bak` backup file inside `dir`.
    pub fn backup_path(dir: impl AsRef<Path>) -> PathBuf {
        dir.as_ref().join("META.bak")
    }

    /// Read and validate `META` from `dir` using the production backend.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn read(dir: impl AsRef<Path>) -> Result<Option<Self>> {
        Self::read_with_backend(dir, &RealBackend)
    }

    /// Read and validate `META` from `dir` using the supplied backend.
    ///
    /// The primary `META` file is tried first.  If it is missing or corrupt, the
    /// `META.bak` backup written by the previous checkpoint is tried.  This
    /// makes checkpoint updates recoverable even if a crash tears the primary
    /// file after the atomic rename.
    ///
    /// Returns `Ok(None)` if neither file exists, allowing a fresh engine open.
    pub fn read_with_backend(
        dir: impl AsRef<Path>,
        backend: &dyn StorageBackend,
    ) -> Result<Option<Self>> {
        let path = Self::path(&dir);
        let backup = Self::backup_path(&dir);
        let mut last_error = None;

        for candidate in [&path, &backup] {
            if !backend.exists(candidate) {
                continue;
            }
            match backend.open(candidate, OpenOptions::new().read(true)) {
                Ok(file) => match Self::read_from_file(&file) {
                    Ok(meta) => return Ok(Some(meta)),
                    Err(e) => {
                        last_error = Some(e);
                    }
                },
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::Io(e)),
            }
        }

        if let Some(e) = last_error {
            return Err(e);
        }
        Ok(None)
    }

    fn read_from_file(file: &dyn StorageFile) -> Result<Self> {
        let len = file.len().map_err(Error::Io)?;
        if len == 0 {
            return Err(Error::Corruption("META file is empty".into()));
        }
        let mut bytes = vec![0u8; len as usize];
        file.read_at(&mut bytes, 0).map_err(Error::Io)?;
        Self::decode(&bytes)
    }

    /// Atomically write `META` to `dir` using the production backend.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn write(&self, dir: impl AsRef<Path>) -> Result<()> {
        Self::write_with_backend(self, dir, &RealBackend)
    }

    /// Atomically write `META` to `dir` using the supplied backend.
    ///
    /// A new `META` is written to a temporary file, fsynced, and renamed over the
    /// current primary.  Before the rename, any existing primary is moved to
    /// `META.bak`, giving recovery a fallback if the rename is interrupted or
    /// the primary is later corrupted.
    pub fn write_with_backend(
        &self,
        dir: impl AsRef<Path>,
        backend: &dyn StorageBackend,
    ) -> Result<()> {
        let path = Self::path(&dir);
        let tmp = dir.as_ref().join(".META.tmp");
        let backup = Self::backup_path(&dir);

        backend.pre_op(Boundary::MetaWriteTemp)?;
        {
            let bytes = self.encode();
            let file = backend.open(
                &tmp,
                OpenOptions::new().write(true).create(true).truncate(true),
            )?;
            file.write_at(&bytes, 0).map_err(Error::Io)?;
            file.sync().map_err(Error::Io)?;
        }
        if backend.exists(&path) {
            backend.rename(&path, &backup).map_err(Error::Io)?;
        }
        backend.rename(&tmp, &path).map_err(Error::Io)?;
        Self::sync_parent_dir(&path, backend)?;
        Ok(())
    }

    fn sync_parent_dir(path: &Path, backend: &dyn StorageBackend) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| Error::Corruption("META has no parent directory".into()))?;
        backend.sync_dir(parent).map_err(Error::Io)
    }

    fn encode(&self) -> Vec<u8> {
        let freelist = self.allocator.snapshot().0;
        let next = self.allocator.snapshot().1;
        let mut buf = Vec::with_capacity(4 + 4 + 8 + 8 + 8 + 8 + 4 + freelist.len() * 8 + 4);
        let mut off = 0;
        buf.resize(4 + 4 + 8 + 8 + 8 + 8 + 4 + freelist.len() * 8 + 4, 0);
        write_u32_le(&mut buf[off..off + 4], META_MAGIC);
        off += 4;
        write_u32_le(&mut buf[off..off + 4], META_VERSION);
        off += 4;
        write_u64_le(&mut buf[off..off + 8], self.root_page_id.get());
        off += 8;
        write_u64_le(&mut buf[off..off + 8], self.checkpoint_lsn.get());
        off += 8;
        write_u64_le(&mut buf[off..off + 8], self.first_undo_lsn.get());
        off += 8;
        write_u64_le(&mut buf[off..off + 8], next.get());
        off += 8;
        write_u32_le(&mut buf[off..off + 4], freelist.len() as u32);
        off += 4;
        for id in freelist {
            write_u64_le(&mut buf[off..off + 8], id.get());
            off += 8;
        }
        let checksum = crc32c(&buf[..off]);
        write_u32_le(&mut buf[off..off + 4], checksum);
        buf
    }

    fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < 44 {
            return Err(Error::Corruption("META file too short".into()));
        }
        let stored_checksum = read_u32_le(&buf[buf.len() - 4..]);
        let computed = crc32c(&buf[..buf.len() - 4]);
        if stored_checksum != computed {
            return Err(Error::Corruption("META checksum mismatch".into()));
        }

        let mut off = 0;
        let magic = read_u32(buf, &mut off)?;
        if magic != META_MAGIC {
            return Err(Error::Corruption(format!(
                "META magic mismatch: expected {META_MAGIC:#x}, got {magic:#x}"
            )));
        }
        let version = read_u32(buf, &mut off)?;
        if version != META_VERSION {
            return Err(Error::Corruption(format!(
                "META version mismatch: expected {META_VERSION}, got {version}"
            )));
        }
        let root_page_id = PageId::new(read_u64(buf, &mut off)?);
        let checkpoint_lsn = Lsn::new(read_u64(buf, &mut off)?);
        let first_undo_lsn = Lsn::new(read_u64(buf, &mut off)?);
        let next = PageId::new(read_u64(buf, &mut off)?);
        let freelist_len = read_u32(buf, &mut off)? as usize;
        if buf.len() < off + freelist_len * 8 + 4 {
            return Err(Error::Corruption("META freelist truncated".into()));
        }
        let mut freelist = Vec::with_capacity(freelist_len);
        for _ in 0..freelist_len {
            freelist.push(PageId::new(read_u64(buf, &mut off)?));
        }

        let mut allocator = PageAllocator::new(PageId::new(1));
        allocator.restore(freelist, next);

        Ok(Self {
            root_page_id,
            checkpoint_lsn,
            first_undo_lsn,
            allocator,
        })
    }
}

fn read_u32(buf: &[u8], off: &mut usize) -> Result<u32> {
    if buf.len() < *off + 4 {
        return Err(Error::Corruption("META u32 truncated".into()));
    }
    let v = read_u32_le(&buf[*off..*off + 4]);
    *off += 4;
    Ok(v)
}

fn read_u64(buf: &[u8], off: &mut usize) -> Result<u64> {
    if buf.len() < *off + 8 {
        return Err(Error::Corruption("META u64 truncated".into()));
    }
    let v = read_u64_le(&buf[*off..*off + 8]);
    *off += 8;
    Ok(v)
}

/// Fuzzy checkpoint driver.
#[derive(Clone)]
pub struct Checkpoint {
    pool: Arc<BufferPool>,
    wal: Arc<WalLog>,
    root: Arc<dyn Fn() -> PageId + Send + Sync>,
    allocator: Arc<SyncMutex<PageAllocator>>,
    dir: PathBuf,
    backend: Arc<dyn StorageBackend>,
}

impl Checkpoint {
    /// Create a checkpoint driver bound to `dir` with a fixed root source.
    #[cfg(test)]
    pub fn new(
        dir: impl AsRef<Path>,
        pool: Arc<BufferPool>,
        wal: Arc<WalLog>,
        root_page_id: Arc<std::sync::atomic::AtomicU64>,
        allocator: Arc<SyncMutex<PageAllocator>>,
    ) -> Self {
        Self::new_with_backend(
            dir,
            pool,
            wal,
            root_page_id,
            allocator,
            Arc::new(RealBackend),
        )
    }

    #[cfg(test)]
    fn new_with_backend(
        dir: impl AsRef<Path>,
        pool: Arc<BufferPool>,
        wal: Arc<WalLog>,
        root_page_id: Arc<std::sync::atomic::AtomicU64>,
        allocator: Arc<SyncMutex<PageAllocator>>,
        backend: Arc<dyn StorageBackend>,
    ) -> Self {
        let root: Arc<dyn Fn() -> PageId + Send + Sync> =
            Arc::new(move || PageId::new(root_page_id.load(Ordering::SeqCst)));
        Self {
            pool,
            wal,
            root,
            allocator,
            dir: dir.as_ref().to_path_buf(),
            backend,
        }
    }

    /// Create a checkpoint driver that reads the current root from `tree` each
    /// time a checkpoint runs. This is the correct choice when the tree root can
    /// change between checkpoints.
    pub fn new_with_tree(
        dir: impl AsRef<Path>,
        pool: Arc<BufferPool>,
        wal: Arc<WalLog>,
        tree: Arc<BPlusTree>,
        allocator: Arc<SyncMutex<PageAllocator>>,
        backend: Arc<dyn StorageBackend>,
    ) -> Self {
        let root: Arc<dyn Fn() -> PageId + Send + Sync> = Arc::new(move || tree.root_page_id());
        Self {
            pool,
            wal,
            root,
            allocator,
            dir: dir.as_ref().to_path_buf(),
            backend,
        }
    }

    /// Run a fuzzy checkpoint and return the new `Meta`.
    ///
    /// The checkpoint is durable once `Meta::write` returns.  After the write
    /// succeeds, completed WAL segments are truncated.
    pub fn run(&self) -> Result<Meta> {
        // 1. Append a checkpoint marker to the WAL.  Its LSN becomes the
        //    official checkpoint LSN.
        let checkpoint_lsn = self.wal.checkpoint([])?;

        // 2. Flush all dirty pages without quiescing writers.  Pages that are
        //    dirtied concurrently are written with their current page LSN and
        //    will be recovered by redo.
        self.pool.flush_all()?;
        // Ensure flushed pages are durable before recording the checkpoint.
        self.pool.sync_disk()?;

        // 3. Capture allocator state while pages are stable on disk.
        let allocator_snapshot = self.allocator.with_mut(|alloc| alloc.snapshot());

        // 4. Determine the oldest LSN still needed for undo.  In autocommit mode
        //    there are no active multi-record transactions, so this is simply the
        //    checkpoint LSN.  When multi-record transactions are introduced this
        //    must also consider the oldest active transaction's first LSN.
        let first_undo_lsn = self
            .oldest_active_lsn(checkpoint_lsn)?
            .unwrap_or(checkpoint_lsn);

        let meta = Meta {
            root_page_id: (self.root)(),
            checkpoint_lsn,
            first_undo_lsn,
            allocator: {
                let mut alloc = PageAllocator::new(PageId::new(1));
                alloc.restore(allocator_snapshot.0, allocator_snapshot.1);
                alloc
            },
        };

        // 5. Atomically persist the metadata.
        meta.write_with_backend(&self.dir, self.backend.as_ref())?;

        // 6. Truncate WAL segments that are fully before the checkpoint.  The
        //    storage-wal layer keeps the active segment intact.
        self.wal.truncate_completed()?;

        Ok(meta)
    }

    /// Return the oldest LSN that may be needed for undo, if any.
    ///
    /// This scans the WAL from the checkpoint LSN to find active transactions.
    /// For autocommit workloads this is expected to return `None`.
    fn oldest_active_lsn(&self, checkpoint_lsn: Lsn) -> Result<Option<Lsn>> {
        let recovery = Recovery::new(self.pool.clone(), self.wal.clone(), (self.root)());
        let analysis = recovery.analyze(checkpoint_lsn)?;
        let mut oldest: Option<Lsn> = None;
        for &ActiveTxn { last_lsn } in analysis.active_txns.values() {
            if last_lsn == NULL_LSN {
                continue;
            }
            oldest = Some(oldest.map_or(last_lsn, |o| o.min(last_lsn)));
        }
        Ok(oldest)
    }
}

/// Options controlling the background checkpoint thread.
#[derive(Clone, Debug)]
pub struct CheckpointOptions {
    /// Time between checkpoints.
    pub interval: Duration,
    /// Run `BPlusTree::check_integrity_with_value_log` after each checkpoint.
    pub run_integrity_check: bool,
    /// Compact the value log after each checkpoint.
    pub compact_value_log: bool,
}

impl Default for CheckpointOptions {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            run_integrity_check: false,
            compact_value_log: false,
        }
    }
}

/// Background thread that periodically runs fuzzy checkpoints.
///
/// The thread is cooperative: it sleeps for `options.interval`, runs a
/// checkpoint, optionally compacts the value log, and optionally validates
/// structural integrity plus value-log references.  Errors terminate the
/// thread and are surfaced when the handle is stopped.
pub struct CheckpointThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<()>>>,
}

impl CheckpointThread {
    /// Spawn a background checkpoint thread.
    ///
    /// # Panics
    ///
    /// Panics if `options.interval` is zero.
    pub fn spawn(checkpoint: Checkpoint, tree: Arc<BPlusTree>, options: CheckpointOptions) -> Self {
        assert!(
            !options.interval.is_zero(),
            "checkpoint interval must be non-zero"
        );
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                std::thread::sleep(options.interval);
                if stop2.load(Ordering::Relaxed) {
                    break;
                }

                // Sync the value log first so the checkpoint metadata reflects
                // all durable large values.
                if let Some(vl) = tree.value_log() {
                    vl.sync()?;
                }

                checkpoint.run()?;

                if options.compact_value_log {
                    tree.compact_value_log()?;
                }
                if options.run_integrity_check {
                    // Online integrity checks run concurrently with writers, so a
                    // transient conflict is not a corruption.  Retry a few times
                    // before treating the failure as fatal.
                    let mut ok = false;
                    for attempt in 0..3 {
                        match tree.check_integrity_with_value_log() {
                            Ok(()) => {
                                ok = true;
                                break;
                            }
                            Err(e) if attempt == 2 => return Err(e),
                            Err(_) => std::thread::sleep(Duration::from_millis(10)),
                        }
                    }
                    if !ok {
                        return Err(Error::Corruption(
                            "online integrity check failed after retries".into(),
                        ));
                    }
                }
            }
            Ok(())
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }

    /// Signal the checkpoint thread to stop and wait for it to finish.
    ///
    /// Returns any error that terminated the thread.
    pub fn stop(mut self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| Error::Corruption("checkpoint thread panicked".into()))?
        } else {
            Ok(())
        }
    }
}

impl Drop for CheckpointThread {
    fn drop(&mut self) {
        // Ask the thread to stop; do not join here because the caller may not
        // own the handle and dropping should not block.  The next checkpoint
        // loop iteration observes the flag and exits cleanly.
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferPool;
    use crate::disk::PagedFile;
    use crate::space::PageAllocator;
    use crate::sync::Mutex as SyncMutex;

    #[test]
    fn meta_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut alloc = PageAllocator::new(PageId::new(1));
        alloc.allocate();
        alloc.allocate();
        alloc.free(PageId::new(1));
        let meta = Meta {
            root_page_id: PageId::new(7),
            checkpoint_lsn: Lsn::new(42),
            first_undo_lsn: Lsn::new(10),
            allocator: alloc,
        };
        meta.write(dir.path()).unwrap();
        let read = Meta::read(dir.path()).unwrap().unwrap();
        assert_eq!(read, meta);
    }

    #[test]
    fn checkpoint_writes_meta_and_truncates_wal() {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(64, 512, disk, alloc.clone()).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());

        // Append a few records so the WAL has something to truncate.
        wal.append(crate::wal::Record {
            header: crate::wal::RecordHeader::new(
                crate::wal::RecordType::SetRoot,
                crate::txn::NULL_TXN_ID,
                crate::wal::NULL_LSN,
                crate::page::NULL_PAGE_ID,
                crate::wal::NULL_LSN,
            ),
            payload: crate::wal::RecordPayload::SetRoot {
                new_root_page_id: PageId::new(5),
            },
        })
        .unwrap();

        let root = Arc::new(std::sync::atomic::AtomicU64::new(PageId::new(5).get()));
        let cp = Checkpoint::new(dir.path(), pool, wal.clone(), root, alloc);
        let meta = cp.run().unwrap();

        assert_eq!(meta.root_page_id, PageId::new(5));
        assert!(meta.checkpoint_lsn > NULL_LSN);
        let read = Meta::read(dir.path()).unwrap().unwrap();
        assert_eq!(read, meta);

        // After truncation, recovery from the checkpoint LSN must still see the
        // records written before it (they are in the active segment).
        let recovery = Recovery::new(cp.pool, cp.wal, meta.root_page_id);
        let recovered_root = recovery.recover(meta.checkpoint_lsn).unwrap();
        assert_eq!(recovered_root, PageId::new(5));
    }

    #[test]
    #[cfg(not(miri))]
    fn checkpoint_thread_runs_checkpoints_and_stops() {
        use crate::tree::BPlusTree;
        use crate::valuelog::ValueLog;

        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(64, 512, disk, alloc.clone()).unwrap());
        let wal = Arc::new(WalLog::open(dir.path(), storage_wal::WalOptions::default()).unwrap());
        pool.set_wal(Arc::clone(&wal));
        let value_log = Arc::new(ValueLog::open(dir.path()).unwrap());

        let tree = Arc::new(
            BPlusTree::new(pool.clone(), 16)
                .unwrap()
                .with_wal(wal.clone())
                .with_value_log(value_log),
        );

        // Insert enough data to survive recovery and give the value log work.
        tree.insert(b"aaa", &[1u8; 32]).unwrap();
        tree.insert(b"bbb", &[2u8; 32]).unwrap();

        let root = Arc::new(std::sync::atomic::AtomicU64::new(tree.root_page_id().get()));
        let checkpoint = Checkpoint::new(dir.path(), pool, wal, root, alloc);

        let handle = CheckpointThread::spawn(
            checkpoint,
            tree.clone(),
            CheckpointOptions {
                interval: Duration::from_millis(10),
                run_integrity_check: true,
                compact_value_log: true,
            },
        );

        // Allow at least one checkpoint cycle to run.
        std::thread::sleep(Duration::from_millis(60));
        handle.stop().unwrap();

        // A durable checkpoint should have been written.
        assert!(Meta::read(dir.path()).unwrap().is_some());
        tree.check_integrity_with_value_log().unwrap();
    }
}
