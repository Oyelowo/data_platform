//! LSM-tree engine implementation.
//!
//! This implementation separates sequence-number allocation, WAL appends, and
//! MemTable inserts so that concurrent writers do not hold a global engine lock
//! for the whole write path.
//!
//! Concurrency model:
//!
//! * One shared [`SequenceAllocator`] hands out sequence numbers and publishes
//!   a *completed watermark* (see [`crate::sequence`] for the correctness
//!   argument).
//! * The WAL is held in an [`Arc`] and is [`Sync`], so many writers can append
//!   concurrently; the WAL committer groups `fsync`s.
//! * The current MemTable pointer lives in its own mutex, so writers can read
//!   it, insert, and check size without acquiring the heavy engine lock.
//! * The engine lock protects immutable MemTable management, version-set
//!   updates, and background-flush coordination.
//! * Readers snapshot the current MemTable, immutable queue, current Version,
//!   block cache, and path under the engine lock, then drop the lock before
//!   doing any I/O.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use bytes::Bytes;

use crate::blob::{BlobRef, BlobStore};
use crate::cache::BlockCaches;
use crate::column_family::{ColumnFamily, ColumnFamilyHandle, ColumnFamilyId, ColumnFamilySet};
use crate::compaction::pick_compaction;
use crate::compaction_merge;
use crate::cursor::LsmCursor;
use crate::immutable::sstable_path;
use crate::internal_key::{ValueType, extract_user_key};
use crate::manifest::Manifest;
use crate::memtable::MemTable;
use crate::options::LsmOptions;
use crate::recovery;
use crate::sequence::SequenceAllocator;
use crate::snapshots::SnapshotRegistry;
use crate::sstable::reader::SSTableReader;
use crate::transaction::LsmTransaction;
use crate::version_set::VersionEdit;
use crate::wal::WalRecord;
use crate::worker::{Worker, WorkerCommand};
use crate::{Error, Result, SequenceNumber};

/// Public handle to an LSM-tree engine.
pub struct LsmEngine {
    inner: LsmEngineInner,
    flush_worker: Option<Worker>,
    compaction_worker: Option<crate::compaction_worker::CompactionWorker>,
    blob_gc_worker: Option<crate::blob_gc::BlobGcWorker>,
}

impl LsmEngine {
    pub(crate) fn inner(&self) -> &LsmEngineInner {
        &self.inner
    }

    /// Open or create an LSM engine at `path`.
    pub fn open(path: impl AsRef<Path>, options: LsmOptions) -> Result<Self> {
        options.validate()?;
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;

        let mut column_families = ColumnFamilySet::with_default(options.clone());
        let blob_store = Arc::new(BlobStore::open(&path, options.blob_file_size)?);
        let last_sequence =
            recovery::recover(&path, &options, &mut column_families, &blob_store)?;

        // Remove SSTables left behind by a crashed compaction, an unfinished
        // flush, or a dropped column family.  Compute the live-file set across
        // all column families so we do not delete files belonging to a CF that
        // is not the default.
        let mut recovery_cleaner = crate::obsolete_files::ObsoleteFiles::new();
        recovery_cleaner.delete_unreferenced_files_on_disk_with_live(
            &path,
            column_families.live_file_numbers(),
        )?;

        let wal = Arc::new(storage_wal::Wal::open(
            path.join("wal"),
            storage_wal::WalOptions {
                segment_size: options.wal_segment_size,
                ..Default::default()
            },
        )?);

        let manifest_path = path.join("MANIFEST-000001");
        let manifest = Arc::new(Mutex::new(if manifest_path.exists() {
            Manifest::open(&manifest_path)?
        } else {
            Manifest::create(&manifest_path)?
        }));
        std::fs::write(path.join("CURRENT"), "MANIFEST-000001\n")?;

        let seq_allocator = SequenceAllocator::new(last_sequence);

        let state = Arc::new(Mutex::new(EngineState {
            path: path.clone(),
            options: options.clone(),
            manifest,
            column_families,
            seq_allocator: seq_allocator.clone(),
            snapshots: SnapshotRegistry::new(),
            immutable_room: Arc::new(Condvar::new()),
            compaction_sender: None,
            blob_gc_sender: None,
            compaction_idle: false,
            compaction_idle_cond: Arc::new(Condvar::new()),
            dropped_cfs: HashMap::new(),
            blob_store: Arc::clone(&blob_store),
        }));

        let (worker, flush_sender) = Worker::spawn(Arc::clone(&state));
        let (compaction_worker, compaction_sender) =
            crate::compaction_worker::CompactionWorker::spawn(Arc::clone(&state));
        let (blob_gc_worker, blob_gc_sender) =
            crate::blob_gc::BlobGcWorker::spawn(Arc::clone(&state));
        state.lock().unwrap().compaction_sender = Some(compaction_sender);
        state.lock().unwrap().blob_gc_sender = Some(blob_gc_sender);

        Ok(Self {
            inner: LsmEngineInner {
                state,
                flush_sender,
                wal,
                seq_allocator,
                blob_store,
            },
            flush_worker: Some(worker),
            compaction_worker: Some(compaction_worker),
            blob_gc_worker: Some(blob_gc_worker),
        })
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        let snapshot = self.inner.seq_allocator.completed();
        self.inner.get(key, snapshot)
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.inner.write(key, value)
    }

    pub fn delete(&self, key: &[u8]) -> Result<()> {
        self.inner.delete(key)
    }

    /// Delete all keys in the half-open range `[start, end)`.
    pub fn delete_range(&self, start: &[u8], end: &[u8]) -> Result<()> {
        self.inner.delete_range(start, end)
    }

    pub fn sync(&self) -> Result<()> {
        self.inner.sync()
    }

    /// Create a new column family with the given options.
    pub fn create_column_family(
        &self,
        name: &str,
        options: LsmOptions,
    ) -> Result<ColumnFamilyHandle> {
        options.validate()?;
        let mut state = self.inner.state.lock().unwrap();
        let handle = state.column_families.create(name, options.clone())?;
        let cf = state.column_families.get(handle.id()).unwrap();
        let edit = crate::version_set::VersionEdit {
            cf_id: handle.id(),
            created_cfs: vec![(handle.id(), name.to_string())],
            next_file_number: cf.version_set.next_file_number(),
            last_sequence: state.seq_allocator.current(),
            ..Default::default()
        };
        state.manifest.lock().unwrap().log_edit(&edit)?;
        let _ = self.inner.flush_sender.send(WorkerCommand::Wake);
        Ok(handle)
    }

    /// Drop a column family.  The default column family cannot be dropped.
    ///
    /// The CF is removed from the active keyspace immediately.  Its SSTable
    /// files are marked obsolete and are physically deleted once no live
    /// `Version` or snapshot references them.
    pub fn drop_column_family(&self, handle: &ColumnFamilyHandle) -> Result<()> {
        let mut state = self.inner.state.lock().unwrap();
        let cf = state.column_families.remove(handle.id())?;

        // Mark every file in the CF's current version as obsolete.  Older
        // retired versions are tracked by VersionSet::live_file_numbers, so
        // files referenced by in-flight readers are not deleted yet.
        let mut zombie = DroppedColumnFamily {
            version_set: Arc::clone(&cf.version_set),
            obsolete_files: cf.obsolete_files,
        };
        let live = cf.version_set.live_file_numbers();
        let input_numbers: Vec<u64> = cf
            .version_set
            .current()
            .levels
            .iter()
            .flat_map(|level| level.iter())
            .map(|f| f.number)
            .filter(|n| live.contains(n))
            .collect();
        zombie.obsolete_files.mark_obsolete_many(input_numbers);
        state.dropped_cfs.insert(handle.id(), zombie);
        state.cleanup_dropped_cfs()?;

        let edit = crate::version_set::VersionEdit {
            cf_id: handle.id(),
            dropped_cfs: vec![handle.id()],
            next_file_number: state
                .column_families
                .default()
                .version_set
                .next_file_number(),
            last_sequence: state.seq_allocator.current(),
            ..Default::default()
        };
        state.manifest.lock().unwrap().log_edit(&edit)
    }

    /// Return a handle to an existing column family by name.
    pub fn cf_handle(&self, name: &str) -> Option<ColumnFamilyHandle> {
        self.inner.state.lock().unwrap().column_families.handle(name)
    }

    /// Write `value` under `key` in `cf`.
    pub fn put_cf(
        &self,
        cf: &ColumnFamilyHandle,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        self.inner.write_cf(cf, key, value)
    }

    /// Read `key` from `cf`.
    pub fn get_cf(
        &self,
        cf: &ColumnFamilyHandle,
        key: &[u8],
    ) -> Result<Option<Bytes>> {
        let snapshot = self.inner.seq_allocator.completed();
        self.inner.get_cf(cf, key, snapshot)
    }

    /// Delete `key` from `cf`.
    pub fn delete_cf(&self, cf: &ColumnFamilyHandle, key: &[u8]) -> Result<()> {
        self.inner.delete_cf(cf, key)
    }

    /// Delete all keys in `[start, end)` from `cf`.
    pub fn delete_range_cf(
        &self,
        cf: &ColumnFamilyHandle,
        start: &[u8],
        end: &[u8],
    ) -> Result<()> {
        self.inner.delete_range_cf(cf, start, end)
    }

    /// Return a cursor over `[start, end)` in `cf`.
    pub fn scan_cf(
        &self,
        cf: &ColumnFamilyHandle,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<LsmCursor> {
        let snapshot = self.inner.seq_allocator.completed();
        LsmCursor::new_cf(
            self.inner.clone(),
            cf.clone(),
            start.map(|s| s.to_vec()),
            end.map(|e| e.to_vec()),
            snapshot,
        )
    }

    /// Create a consistent, point-in-time checkpoint in `dir`.
    ///
    /// `dir` must not exist or must be empty.  The checkpoint is a self-contained
    /// engine directory that can be opened with [`LsmEngine::open`].
    pub fn checkpoint(&self, dir: impl AsRef<Path>) -> Result<()> {
        crate::backup::create_checkpoint(self, dir)
    }

    /// Create a named backup under `<engine>/backups/<name>`.
    pub fn create_backup(&self, name: &str) -> Result<()> {
        crate::backup::create_backup(self, name)
    }

    /// Restore the named backup to `target`.
    ///
    /// `target` must not exist or must be empty.  The restored directory is a
    /// self-contained engine and can be opened with [`LsmEngine::open`].
    pub fn restore_backup(
        &self,
        name: &str,
        target: impl AsRef<Path>,
    ) -> Result<()> {
        crate::backup::restore_backup(self, name, target)
    }

    /// Delete the named backup.
    pub fn delete_backup(&self, name: &str) -> Result<()> {
        crate::backup::delete_backup(self, name)
    }

    /// Return the names of all backups stored under the engine.
    pub fn list_backups(&self) -> Result<Vec<String>> {
        crate::backup::list_backups(self)
    }
}

impl Drop for LsmEngine {
    fn drop(&mut self) {
        // Shut down the flush worker first so no new L0 files are produced,
        // then shut down the compaction worker so any in-progress compaction
        // finishes cleanly, and finally the blob GC worker.
        if let Some(worker) = self.flush_worker.take() {
            worker.shutdown();
        }
        if let Some(worker) = self.compaction_worker.take() {
            worker.shutdown();
        }
        if let Some(worker) = self.blob_gc_worker.take() {
            worker.shutdown();
        }
    }
}

impl storage_traits::Engine for LsmEngine {
    type Error = Error;
    type Transaction = LsmTransaction;
    type Cursor = LsmCursor;

    fn name(&self) -> &'static str {
        "storage-kv"
    }

    fn begin(&self, opts: storage_traits::TxnOptions) -> Result<Self::Transaction> {
        let seq = self.inner.seq_allocator.completed();
        Ok(LsmTransaction::new(self.inner.clone(), opts.read_only, seq))
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.get(key)
    }

    fn scan(&self, start: Option<&[u8]>, end: Option<&[u8]>) -> Result<Self::Cursor> {
        let snapshot = self.inner.seq_allocator.completed();
        LsmCursor::new(
            self.inner.clone(),
            start.map(|s| s.to_vec()),
            end.map(|e| e.to_vec()),
            snapshot,
        )
    }

    fn stats(&self) -> Result<storage_traits::EngineStats> {
        let state = self.inner.state.lock().unwrap();
        let mut disk_bytes = 0u64;
        let mut memory_bytes = 0u64;
        let mut metrics = std::collections::HashMap::<String, u64>::new();
        for cf in state.column_families.iter() {
            let version = cf.version_set.current();
            disk_bytes += version
                .levels
                .iter()
                .flat_map(|level| level.iter())
                .map(|file| file.file_size)
                .sum::<u64>();
            memory_bytes += cf.memtable.lock().unwrap().approximate_size() as u64;
            memory_bytes += cf.immutable.approximate_size() as u64;
            memory_bytes += cf.options.block_cache_size as u64;
            memory_bytes += cf.options.compressed_block_cache_size as u64;
            for (k, v) in cf.metrics.snapshot() {
                *metrics.entry(k).or_insert(0) += v;
            }
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
        self.sync()
    }
}

/// Cloneable inner handle shared by engine, transactions, and cursors.
#[derive(Clone)]
pub struct LsmEngineInner {
    pub(crate) state: Arc<Mutex<EngineState>>,
    flush_sender: crossbeam_channel::Sender<WorkerCommand>,
    wal: Arc<storage_wal::Wal>,
    seq_allocator: SequenceAllocator,
    pub(crate) blob_store: Arc<BlobStore>,
}

impl LsmEngineInner {
    pub(crate) fn write(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.write_cf(&ColumnFamilyHandle::default(), key, value)
    }

    pub(crate) fn write_cf(
        &self,
        cf: &ColumnFamilyHandle,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let seq = self.seq_allocator.next();
        let guard = self.seq_allocator.guard(seq);

        let record = WalRecord::new_put_cf(cf.id, key, value, seq);
        let mut payload = Vec::new();
        record.encode(&mut payload);
        if let Err(e) = self
            .wal
            .append(&payload, storage_wal::Durability::Immediate)
        {
            // The write never became durable.  Releasing the sequence publishes
            // a watermark that skips this sequence, so it cannot block future
            // snapshots.
            drop(guard);
            return Err(e.into());
        }

        // Large values are written to the append-only blob log; the LSM stores
        // a 24-byte reference instead.  The blob write happens outside the
        // engine lock so other writers are not blocked by the fsync.
        let min_blob_size = {
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf.id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            cf.options.min_blob_value_size
        };

        if min_blob_size > 0 && value.len() >= min_blob_size {
            let blob_ref = self.blob_store.put(cf.id, key, value, seq)?;
            let encoded = blob_ref.encode();
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf.id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            let memtable = cf.memtable.lock().unwrap();
            let write_guard = memtable.write_guard();
            drop(memtable);
            write_guard.put_blob_ref(key, seq, &encoded);
        } else {
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf.id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            let memtable = cf.memtable.lock().unwrap();
            let write_guard = memtable.write_guard();
            drop(memtable);
            write_guard.put(key, seq, value);
        }

        // The write is now visible to readers; publish its sequence.
        guard.release();

        self.check_write_stall(cf.id)?;

        if self.should_freeze(cf.id) {
            self.maybe_freeze(cf.id)?;
        }
        Ok(())
    }

    /// Slow down or reject writes when compaction cannot keep up with the L0
    /// file backlog for the given column family.
    fn check_write_stall(&self, cf_id: ColumnFamilyId) -> Result<()> {
        let state = self.state.lock().unwrap();
        let cf = state
            .column_families
            .get(cf_id)
            .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
        let l0_files = cf.version_set.current().level0_files();
        if l0_files >= cf.options.level0_stop_writes_trigger {
            return Err(Error::Busy("L0 stop-write trigger hit".into()));
        }
        if l0_files >= cf.options.level0_slowdown_writes_trigger {
            drop(state);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }

    pub(crate) fn delete(&self, key: &[u8]) -> Result<()> {
        self.delete_cf(&ColumnFamilyHandle::default(), key)
    }

    pub(crate) fn delete_cf(&self, cf: &ColumnFamilyHandle, key: &[u8]) -> Result<()> {
        let seq = self.seq_allocator.next();
        let guard = self.seq_allocator.guard(seq);

        let record = WalRecord::new_delete_cf(cf.id, key, seq);
        let mut payload = Vec::new();
        record.encode(&mut payload);
        if let Err(e) = self
            .wal
            .append(&payload, storage_wal::Durability::Immediate)
        {
            drop(guard);
            return Err(e.into());
        }

        {
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf.id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            let memtable = cf.memtable.lock().unwrap();
            let write_guard = memtable.write_guard();
            drop(memtable);
            write_guard.delete(key, seq);
        }

        guard.release();

        self.check_write_stall(cf.id)?;

        if self.should_freeze(cf.id) {
            self.maybe_freeze(cf.id)?;
        }
        Ok(())
    }

    pub(crate) fn delete_range(&self, start: &[u8], end: &[u8]) -> Result<()> {
        self.delete_range_cf(&ColumnFamilyHandle::default(), start, end)
    }

    pub(crate) fn delete_range_cf(
        &self,
        cf: &ColumnFamilyHandle,
        start: &[u8],
        end: &[u8],
    ) -> Result<()> {
        if start >= end {
            return Err(crate::Error::InvalidArgument(
                "delete_range start must be < end".into(),
            ));
        }

        let seq = self.seq_allocator.next();
        let guard = self.seq_allocator.guard(seq);

        let record = WalRecord::new_delete_range_cf(cf.id, start, end, seq);
        let mut payload = Vec::new();
        record.encode(&mut payload);
        if let Err(e) = self
            .wal
            .append(&payload, storage_wal::Durability::Immediate)
        {
            drop(guard);
            return Err(e.into());
        }

        {
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf.id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            let memtable = cf.memtable.lock().unwrap();
            let write_guard = memtable.write_guard();
            drop(memtable);
            write_guard.delete_range(start, end, seq);
        }

        guard.release();

        self.check_write_stall(cf.id)?;

        if self.should_freeze(cf.id) {
            self.maybe_freeze(cf.id)?;
        }
        Ok(())
    }

    /// Apply all writes buffered inside a transaction as a single visible unit.
    ///
    /// Each operation receives a monotonically increasing sequence number, but
    /// no sequence is released until every operation has been appended to the
    /// WAL and inserted into its MemTable.  This keeps the published snapshot
    /// watermark below the batch while it is being applied, so readers observe
    /// either none or all of the transaction's writes.
    pub(crate) fn apply_transaction_ops(&self, ops: &[crate::transaction::WriteOp]) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }

        // Allocate one sequence per operation and hold every guard until the
        // very end.  The watermark cannot advance past the first sequence of the
        // batch until all guards are released, which prevents partial reads.
        let mut guards = Vec::with_capacity(ops.len());
        let mut sequences = Vec::with_capacity(ops.len());
        for _ in ops {
            let seq = self.seq_allocator.next();
            guards.push(self.seq_allocator.guard(seq));
            sequences.push(seq);
        }

        // Append every record to the WAL first.  If any append fails we drop the
        // remaining guards, which publishes a gap in the sequence space and
        // makes the (partial) batch invisible.
        for (op, &seq) in ops.iter().zip(sequences.iter()) {
            let record = match op {
                crate::transaction::WriteOp::Put { cf, key, value } => {
                    WalRecord::new_put_cf(*cf, key, value, seq)
                }
                crate::transaction::WriteOp::Delete { cf, key } => {
                    WalRecord::new_delete_cf(*cf, key, seq)
                }
                crate::transaction::WriteOp::DeleteRange { cf, start, end } => {
                    if start >= end {
                        drop(guards);
                        return Err(Error::InvalidArgument(
                            "delete_range start must be < end".into(),
                        ));
                    }
                    WalRecord::new_delete_range_cf(*cf, start, end, seq)
                }
            };
            let mut payload = Vec::new();
            record.encode(&mut payload);
            if let Err(e) = self
                .wal
                .append(&payload, storage_wal::Durability::Immediate)
            {
                drop(guards);
                return Err(e.into());
            }
        }

        // Large values in the batch are written to the blob log outside the
        // engine lock.  A `None` entry means the corresponding operation either
        // is not a put or stores its value inline.
        let mut blob_refs: Vec<Option<crate::blob::BlobRef>> = Vec::with_capacity(ops.len());
        for (op, &seq) in ops.iter().zip(sequences.iter()) {
            if let crate::transaction::WriteOp::Put { cf, key, value } = op {
                let min_blob_size = {
                    let state = self.state.lock().unwrap();
                    let cf = state
                        .column_families
                        .get(*cf)
                        .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
                    cf.options.min_blob_value_size
                };
                if min_blob_size > 0 && value.len() >= min_blob_size {
                    blob_refs.push(Some(self.blob_store.put(*cf, key, value, seq)?));
                } else {
                    blob_refs.push(None);
                }
            } else {
                blob_refs.push(None);
            }
        }

        // Insert every operation into the appropriate MemTable.  Because the
        // sequence numbers increase with operation order, the final memtable
        // state reflects the order in which the application issued the writes.
        for ((op, &seq), blob_ref) in ops.iter().zip(sequences.iter()).zip(blob_refs) {
            let state = self.state.lock().unwrap();
            let cf_id = match op {
                crate::transaction::WriteOp::Put { cf, .. }
                | crate::transaction::WriteOp::Delete { cf, .. }
                | crate::transaction::WriteOp::DeleteRange { cf, .. } => *cf,
            };
            let cf = state
                .column_families
                .get(cf_id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            let memtable = cf.memtable.lock().unwrap();
            let write_guard = memtable.write_guard();
            drop(memtable);
            match op {
                crate::transaction::WriteOp::Put { key, value, .. } => {
                    if let Some(blob_ref) = blob_ref {
                        write_guard.put_blob_ref(key, seq, &blob_ref.encode());
                    } else {
                        write_guard.put(key, seq, value);
                    }
                }
                crate::transaction::WriteOp::Delete { key, .. } => {
                    write_guard.delete(key, seq);
                }
                crate::transaction::WriteOp::DeleteRange { start, end, .. } => {
                    write_guard.delete_range(start, end, seq);
                }
            }
            drop(state);
            self.check_write_stall(cf_id)?;
            if self.should_freeze(cf_id) {
                self.maybe_freeze(cf_id)?;
            }
        }

        // All writes are now durable and visible; release the watermark.
        for guard in guards {
            guard.release();
        }

        Ok(())
    }

    /// Cheap outside-the-lock check for whether the given CF's MemTable has
    /// grown past the configured limit.
    fn should_freeze(&self, cf_id: ColumnFamilyId) -> bool {
        let state = self.state.lock().unwrap();
        let cf = match state.column_families.get(cf_id) {
            Some(cf) => cf,
            None => return false,
        };
        cf.memtable.lock().unwrap().approximate_size() >= cf.options.write_buffer_size
    }

    /// Freeze the current MemTable for the given column family when it reaches
    /// `write_buffer_size`.
    ///
    /// Returns `true` if a MemTable was queued for background flush. If the
    /// immutable queue is full, the writer stalls until the background worker
    /// frees a slot (RocksDB's `max_write_buffer_number` backpressure).
    fn maybe_freeze(&self, cf_id: ColumnFamilyId) -> Result<bool> {
        let freeze_lock = {
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf_id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            Arc::clone(&cf.freeze_lock)
        };
        // Serialize freezers for this CF for the whole freeze+stall: the
        // immutable-queue order must remain version order even when a freezer
        // has to wait for the background worker to drain a full queue.
        let _freeze_lock = freeze_lock.lock().unwrap();

        // Seal the current MemTable under the engine lock.  We keep the engine
        // lock until the old table is queued so that readers cannot observe a
        // state where the table is neither mutable nor immutable.  The per-CF
        // memtable lock is released while we wait for in-flight writers on the
        // old table to finish, so new writers can proceed on the new table.
        let mut state = self.state.lock().unwrap();
        let cf = state
            .column_families
            .get(cf_id)
            .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
        let mut memtable_guard = cf.memtable.lock().unwrap();

        if memtable_guard.approximate_size() < cf.options.write_buffer_size {
            return Ok(false);
        }

        let old_mem = std::mem::replace(&mut *memtable_guard, Arc::new(MemTable::new()));
        let file_number = cf.version_set.new_file_number();
        drop(memtable_guard);

        // Wait for any writers that cloned `old_mem` before the swap to finish
        // inserting.  No new writer can obtain `old_mem` because it is no
        // longer reachable from the engine state.  We still hold the engine
        // lock so readers cannot see the new table without also seeing the old
        // one queued.
        while !old_mem.is_quiesced() {
            std::thread::yield_now();
        }

        // `old_mem` is now sealed.  Wait until the immutable queue has room.
        // `Condvar::wait` releases the state lock while sleeping so the flush
        // worker can make progress.  The queue must only ever be drained by the
        // single background flusher, in FIFO order: flushing the just-frozen
        // table synchronously here would publish the newest versions to L0
        // while older versions are still queued, and point reads — which search
        // the queue before L0 — would then return stale data.
        while state.column_families.get(cf_id).unwrap().immutable.is_full() {
            let room = Arc::clone(&state.immutable_room);
            state = room.wait(state).unwrap();
        }

        state
            .column_families
            .get_mut(cf_id)
            .unwrap()
            .immutable
            .push(file_number, old_mem);
        drop(state);
        let _ = self.flush_sender.send(WorkerCommand::Wake);
        Ok(true)
    }

    /// Freeze the current MemTable for the given column family even if it has
    /// not reached `write_buffer_size`.
    ///
    /// Used by checkpoint/backup to ensure all buffered data is flushed to
    /// SSTables before the snapshot is taken.  Empty MemTables are ignored.
    pub(crate) fn force_freeze(&self, cf_id: ColumnFamilyId) -> Result<()> {
        let freeze_lock = {
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf_id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            Arc::clone(&cf.freeze_lock)
        };
        let _freeze_lock = freeze_lock.lock().unwrap();

        let mut state = self.state.lock().unwrap();
        let cf = state
            .column_families
            .get(cf_id)
            .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
        let mut memtable_guard = cf.memtable.lock().unwrap();

        if memtable_guard.approximate_size() == 0 {
            return Ok(());
        }

        let old_mem = std::mem::replace(&mut *memtable_guard, Arc::new(MemTable::new()));
        let file_number = cf.version_set.new_file_number();
        drop(memtable_guard);

        while !old_mem.is_quiesced() {
            std::thread::yield_now();
        }

        while state.column_families.get(cf_id).unwrap().immutable.is_full() {
            let room = Arc::clone(&state.immutable_room);
            state = room.wait(state).unwrap();
        }

        state
            .column_families
            .get_mut(cf_id)
            .unwrap()
            .immutable
            .push(file_number, old_mem);
        drop(state);
        let _ = self.flush_sender.send(WorkerCommand::Wake);
        Ok(())
    }

    /// Run compaction jobs until none are needed across all column families.
    ///
    /// Returns `true` if at least one compaction job was executed.  The engine
    /// lock is released around the merge phase so reads and writes can proceed
    /// concurrently with the I/O-heavy part of compaction.
    pub(crate) fn maybe_compact(state: &Arc<Mutex<EngineState>>) -> Result<bool> {
        let mut did_work = false;
        loop {
            // Pick a job and reserve output file numbers while holding the lock.
            let picked = {
                let state = state.lock().unwrap();
                let mut picked = None;
                for cf in state.column_families.iter() {
                    let version = cf.version_set.current();
                    let job = match pick_compaction(&version, &cf.version_set, &cf.options) {
                        Some(j) => j,
                        None => continue,
                    };
                    let output_level = job.level + 1;
                    let target_size =
                        compaction_merge::target_file_size(output_level, &cf.options).max(1);
                    let estimated_outputs =
                        (job.input_size() / target_size) + job.num_input_files() as u64 + 10;
                    let output_numbers: Vec<u64> = (0..estimated_outputs)
                        .map(|_| cf.version_set.new_file_number())
                        .collect();
                    let smallest_snapshot = state
                        .snapshots
                        .oldest()
                        .unwrap_or_else(|| state.seq_allocator.current());
                    picked = Some((
                        cf.id,
                        job,
                        output_numbers,
                        version,
                        state.path.clone(),
                        cf.options.clone(),
                        cf.caches.clone(),
                        Arc::clone(&cf.metrics),
                        smallest_snapshot,
                        Arc::clone(&state.manifest),
                        state.seq_allocator.clone(),
                    ));
                    break;
                }
                picked
            };

            let (
                cf_id,
                job,
                output_numbers,
                version,
                path,
                options,
                caches,
                metrics,
                smallest_snapshot,
                manifest,
                seq_allocator,
            ) = match picked {
                Some(v) => v,
                None => break,
            };

            let output_level = job.level + 1;

            {
                let mut state = state.lock().unwrap();
                state.column_families.get_mut(cf_id).unwrap().active_compactions += 1;
            }

            // Run the merge outside the engine lock.
            let merge_result = compaction_merge::run_compaction_merge(
                &path,
                &options,
                &version,
                &job,
                &output_numbers,
                Some(caches),
                metrics,
                smallest_snapshot,
            );

            let output_files = match merge_result {
                Ok(files) => files,
                Err(e) => {
                    let mut state = state.lock().unwrap();
                    state.column_families.get_mut(cf_id).unwrap().active_compactions -= 1;
                    return Err(e);
                }
            };

            // Apply the version edit under the lock.
            {
                let mut state = state.lock().unwrap();
                let cf = state.column_families.get_mut(cf_id).unwrap();
                let mut edit = VersionEdit {
                    cf_id,
                    last_sequence: seq_allocator.current(),
                    next_file_number: cf.version_set.next_file_number(),
                    ..Default::default()
                };
                for (level_offset, files) in job.inputs.iter().enumerate() {
                    let level = job.level + level_offset;
                    for file in files {
                        edit.deleted_files.push((level, file.number));
                    }
                }
                for file in output_files {
                    edit.new_files.push((output_level, file));
                }
                manifest.lock().unwrap().log_edit(&edit)?;
                cf.version_set.apply(edit)?;

                // Schedule input files for deferred deletion.  They are deleted
                // only after the manifest is durable and no live `Version`
                // references them.
                let input_numbers: Vec<u64> = job
                    .inputs
                    .iter()
                    .flat_map(|v| v.iter())
                    .map(|f| f.number)
                    .collect();
                let version_set = Arc::clone(&cf.version_set);
                cf.obsolete_files.mark_obsolete_many(input_numbers);
                cf.obsolete_files.delete_unreferenced(&path, &version_set)?;

                state.column_families.get_mut(cf_id).unwrap().active_compactions -= 1;
            }

            did_work = true;
        }
        Ok(did_work)
    }

    pub(crate) fn get(&self, key: &[u8], snapshot: SequenceNumber) -> Result<Option<Bytes>> {
        self.get_cf(&ColumnFamilyHandle::default(), key, snapshot)
    }

    pub(crate) fn get_cf(
        &self,
        cf: &ColumnFamilyHandle,
        key: &[u8],
        snapshot: SequenceNumber,
    ) -> Result<Option<Bytes>> {
        let (memtable, immutable, version, caches, path) = {
            let state = self.state.lock().unwrap();
            let cf = state
                .column_families
                .get(cf.id)
                .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
            let memtable = cf.memtable.lock().unwrap().clone();
            let immutable = cf.immutable.snapshot();
            let version = cf.version_set.current();
            let caches = cf.caches.clone();
            let path = state.path.clone();
            (memtable, immutable, version, caches, path)
        };
        self.get_with_parts(key, snapshot, &memtable, &immutable, &version, &caches, &path)
    }

    /// Read `key` from the default column family using a fully pinned snapshot
    /// view instead of the current engine state.  Transactions use this to keep
    /// reading from the MemTables and `Version` they captured at begin time.
    pub(crate) fn get_with_view(
        &self,
        key: &[u8],
        snapshot: SequenceNumber,
        view: &crate::transaction::CfSnapshotView,
    ) -> Result<Option<Bytes>> {
        self.get_with_parts(
            key,
            snapshot,
            &view.memtable,
            &view.immutable,
            &view.version,
            &view.caches,
            &view.path,
        )
    }

    /// Read `key` from a column family using a fully pinned snapshot view.
    pub(crate) fn get_cf_with_view(
        &self,
        key: &[u8],
        snapshot: SequenceNumber,
        view: &crate::transaction::CfSnapshotView,
    ) -> Result<Option<Bytes>> {
        self.get_with_parts(
            key,
            snapshot,
            &view.memtable,
            &view.immutable,
            &view.version,
            &view.caches,
            &view.path,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn get_with_parts(
        &self,
        key: &[u8],
        snapshot: SequenceNumber,
        memtable: &Arc<MemTable>,
        immutable: &[Arc<MemTable>],
        version: &crate::version::Version,
        caches: &BlockCaches,
        path: &Path,
    ) -> Result<Option<Bytes>> {
        if let Some((ty, val)) = memtable.get_with_type(key, snapshot) {
            return resolve_value(ty, val, &self.blob_store);
        }

        for mem in immutable {
            if let Some((ty, val)) = mem.get_with_type(key, snapshot) {
                return resolve_value(ty, val, &self.blob_store);
            }
        }

        for file in version.levels[0].iter().rev() {
            let file_path = sstable_path(path, file.number);
            let mut reader =
                SSTableReader::open(file_path, file.number, Some(caches.clone()))?;
            if let Some(result) = reader.get_with_type(key, snapshot)? {
                return resolve_typed_result(result, &self.blob_store);
            }
        }

        for level in 1..version.levels.len() {
            // Levels 1+ are non-overlapping and sorted by user key, so binary
            // search to the single file that may contain the key.
            if let Some(file) = version.pick_level_file(level, key) {
                let smallest_user = extract_user_key(&file.smallest);
                let largest_user = extract_user_key(&file.largest);
                if key < smallest_user || key > largest_user {
                    continue;
                }
                let file_path = sstable_path(path, file.number);
                let mut reader =
                    SSTableReader::open(file_path, file.number, Some(caches.clone()))?;
                if let Some(result) = reader.get_with_type(key, snapshot)? {
                    return resolve_typed_result(result, &self.blob_store);
                }
            }
        }

        Ok(None)
    }

    pub(crate) fn sync(&self) -> Result<()> {
        // Wake the workers so any queued immutable MemTables are flushed and any
        // pending compaction jobs are picked up.
        let _ = self.flush_sender.send(WorkerCommand::Wake);
        if let Some(ref sender) = self.state.lock().unwrap().compaction_sender {
            let _ = sender.send(crate::compaction_worker::CompactionCommand::Wake);
        }
        loop {
            let state = self.state.lock().unwrap();
            let all_quiet = state.compaction_idle
                && state.column_families.iter().all(|cf| {
                    cf.immutable.is_empty()
                        && cf.active_flushes == 0
                        && cf.active_compactions == 0
                })
                && self.seq_allocator.is_quiesced()
                && self.seq_allocator.completed() == self.seq_allocator.current();
            if all_quiet {
                break;
            }
            let cond = Arc::clone(&state.compaction_idle_cond);
            // Wait for the worker to become idle, or poll at 1 ms so that
            // conditions not signalled on this condvar (flush completion,
            // sequence release) are still re-checked promptly.
            let (state, _) = cond
                .wait_timeout(state, std::time::Duration::from_millis(1))
                .unwrap();
            drop(state);
        }
        let mut state = self.state.lock().unwrap();
        // Try to clean up any files left behind by dropped column families.
        state.cleanup_dropped_cfs()?;
        // WAL already fsyncs every append. Force manifest sync for metadata.
        state.manifest.lock().unwrap().sync()
    }
}

/// Metadata kept for a column family after it has been dropped.  Its files
/// cannot be deleted until no live `Version` (and therefore no in-flight
/// reader) references them.
pub(crate) struct DroppedColumnFamily {
    pub(crate) version_set: Arc<crate::version_set::VersionSet>,
    pub(crate) obsolete_files: crate::obsolete_files::ObsoleteFiles,
}

pub(crate) struct EngineState {
    pub(crate) path: PathBuf,
    /// Global engine options used as defaults and for shared resources such as
    /// the WAL.  Each column family carries its own copy for per-CF tuning.
    pub(crate) options: LsmOptions,
    pub(crate) manifest: Arc<Mutex<Manifest>>,
    pub(crate) column_families: ColumnFamilySet,
    pub(crate) seq_allocator: SequenceAllocator,
    /// Live read snapshots that compaction must not invalidate.
    pub(crate) snapshots: SnapshotRegistry,
    /// Signalled by the flush worker whenever a queued MemTable has been
    /// flushed and popped, waking writers stalled on a full immutable queue.
    pub(crate) immutable_room: Arc<Condvar>,
    /// Sender used by the flush worker to wake the compaction worker.  Filled
    /// in after both workers are spawned.
    pub(crate) compaction_sender: Option<crossbeam_channel::Sender<crate::compaction_worker::CompactionCommand>>,
    /// Sender used to wake or shut down the blob GC worker.  Filled in after
    /// the worker is spawned.
    pub(crate) blob_gc_sender: Option<crossbeam_channel::Sender<crate::blob_gc::BlobGcCommand>>,
    /// `true` when the compaction worker has drained all pending jobs and is
    /// blocked waiting for the next command.  Protected by the engine lock and
    /// signalled by `compaction_idle_cond`.
    pub(crate) compaction_idle: bool,
    /// Signalled whenever `compaction_idle` transitions to `true`.
    pub(crate) compaction_idle_cond: Arc<Condvar>,
    /// Column families that have been dropped but still have SSTable files
    /// awaiting deletion once no live `Version` references them.
    pub(crate) dropped_cfs: HashMap<ColumnFamilyId, DroppedColumnFamily>,
    /// Shared append-only value log for WiscKey-style blob separation.
    /// Retained in `EngineState` so background workers (future blob GC) can
    /// reach it through the same `Arc<Mutex<EngineState>>` they already hold.
    #[allow(dead_code)]
    pub(crate) blob_store: Arc<BlobStore>,
}

impl EngineState {
    /// Return a reference to the default column family.
    pub(crate) fn default_cf(&self) -> &ColumnFamily {
        self.column_families.default()
    }

    /// Try to delete files of dropped column families that are no longer
    /// referenced by a retired `Version` held by an in-flight reader.  The
    /// current `Version` of a dropped CF is treated as obsolete, so files are
    /// deleted as soon as no snapshot still references them.
    pub(crate) fn cleanup_dropped_cfs(&mut self) -> Result<()> {
        let path = self.path.clone();
        let mut fully_cleaned = Vec::new();
        for (id, zombie) in self.dropped_cfs.iter_mut() {
            let live = zombie.version_set.live_file_numbers();
            let current = zombie.version_set.current_file_numbers();
            // Only retired versions (snapshots) can keep files alive now.
            let retired: std::collections::HashSet<_> = live.difference(&current).copied().collect();
            let to_delete: Vec<u64> = zombie
                .obsolete_files
                .pending()
                .filter(|n| !retired.contains(n))
                .collect();
            for number in to_delete {
                let _ = std::fs::remove_file(crate::immutable::sstable_path(&path, number));
                zombie.obsolete_files.remove(number);
            }
            if zombie.obsolete_files.pending_count() == 0 {
                fully_cleaned.push(*id);
            }
        }
        for id in fully_cleaned {
            self.dropped_cfs.remove(&id);
        }
        Ok(())
    }
}

/// Owner used by the blob GC worker to check liveness and durably rewrite live
/// blob references into new blob files.
pub(crate) struct BlobGcOwner {
    state: Arc<Mutex<EngineState>>,
    blob_store: Arc<BlobStore>,
    pending: HashMap<ColumnFamilyId, MemTable>,
}

impl BlobGcOwner {
    pub(crate) fn new(state: Arc<Mutex<EngineState>>, blob_store: Arc<BlobStore>) -> Self {
        Self {
            state,
            blob_store,
            pending: HashMap::new(),
        }
    }
}

impl crate::blob::BlobOwner for BlobGcOwner {
    fn is_blob_live(
        &self,
        cf_id: ColumnFamilyId,
        key: &[u8],
        blob_ref: BlobRef,
        snapshot: SequenceNumber,
    ) -> bool {
        match self.lookup_blob_ref(cf_id, key, snapshot) {
            Ok(Some(r)) => r == blob_ref,
            _ => false,
        }
    }

    fn rewrite_blob(
        &mut self,
        cf_id: ColumnFamilyId,
        key: &[u8],
        value: &[u8],
        seq: SequenceNumber,
    ) -> Result<()> {
        let new_ref = self.blob_store.put(cf_id, key, value, seq)?;
        let mem = self.pending.entry(cf_id).or_insert_with(MemTable::new);
        mem.put_blob_ref(key, seq, &new_ref.encode());
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock().unwrap();
        let path = state.path.clone();
        let manifest = Arc::clone(&state.manifest);
        let last_sequence = state.seq_allocator.current();
        let to_flush: Vec<(ColumnFamilyId, MemTable, crate::FileNumber, Arc<crate::version_set::VersionSet>, Arc<crate::metrics::Metrics>, crate::options::LsmOptions)> = self
            .pending
            .drain()
            .filter_map(|(cf_id, mem)| {
                let cf = state.column_families.get_mut(cf_id)?;
                Some((
                    cf_id,
                    mem,
                    cf.version_set.new_file_number(),
                    Arc::clone(&cf.version_set),
                    Arc::clone(&cf.metrics),
                    cf.options.clone(),
                ))
            })
            .collect();
        drop(state);
        for (cf_id, mem, file_number, version_set, metrics, options) in to_flush {
            crate::flush::flush_memtable(
                &path,
                &options,
                &version_set,
                &manifest,
                &mem,
                last_sequence,
                file_number,
                &metrics,
                cf_id,
            )?;
        }
        Ok(())
    }
}

impl BlobGcOwner {
    fn lookup_blob_ref(
        &self,
        cf_id: ColumnFamilyId,
        key: &[u8],
        snapshot: SequenceNumber,
    ) -> Result<Option<BlobRef>> {
        let state = self.state.lock().unwrap();
        let cf = state
            .column_families
            .get(cf_id)
            .ok_or_else(|| Error::InvalidArgument("column family not found".into()))?;
        let memtable = cf.memtable.lock().unwrap().clone();
        let immutable = cf.immutable.snapshot();
        let version = cf.version_set.current();
        let caches = cf.caches.clone();
        let path = state.path.clone();
        drop(state);

        if let Some((ty, val)) = memtable.get_with_type(key, snapshot) {
            return Ok(extract_blob_ref(ty, val));
        }
        for mem in immutable {
            if let Some((ty, val)) = mem.get_with_type(key, snapshot) {
                return Ok(extract_blob_ref(ty, val));
            }
        }
        for file in version.levels[0].iter().rev() {
            let file_path = sstable_path(&path, file.number);
            let mut reader =
                SSTableReader::open(file_path, file.number, Some(caches.clone()))?;
            if let Some(Some((ty, val))) = reader.get_with_type(key, snapshot)? {
                return Ok(extract_blob_ref(ty, Some(val)));
            }
        }
        for level in 1..version.levels.len() {
            if let Some(file) = version.pick_level_file(level, key) {
                let smallest_user = extract_user_key(&file.smallest);
                let largest_user = extract_user_key(&file.largest);
                if key < smallest_user || key > largest_user {
                    continue;
                }
                let file_path = sstable_path(&path, file.number);
                let mut reader =
                    SSTableReader::open(file_path, file.number, Some(caches.clone()))?;
                if let Some(Some((ty, val))) = reader.get_with_type(key, snapshot)? {
                    return Ok(extract_blob_ref(ty, Some(val)));
                }
            }
        }
        Ok(None)
    }
}

fn extract_blob_ref(ty: ValueType, value: Option<Bytes>) -> Option<BlobRef> {
    match ty {
        ValueType::BlobRef => BlobRef::decode(&value?),
        _ => None,
    }
}

/// Resolve a typed value read from a MemTable or SSTable.
///
/// Inline values are returned as-is, deletion tombstones become `None`, and
/// blob references are decoded and fetched from the blob log.
fn resolve_value(
    ty: ValueType,
    value: Option<Bytes>,
    blob_store: &BlobStore,
) -> Result<Option<Bytes>> {
    match ty {
        ValueType::Deletion => Ok(None),
        ValueType::Value => Ok(value),
        ValueType::BlobRef => {
            let bytes = value.ok_or_else(|| {
                Error::Blob("blob reference entry missing reference bytes".into())
            })?;
            let blob_ref = BlobRef::decode(&bytes)
                .ok_or_else(|| Error::Blob("bad blob reference in LSM entry".into()))?;
            blob_store.get(blob_ref).map(Some)
        }
        ValueType::RangeDeletion => Ok(None),
    }
}

/// Resolve the `Option<Option<(ValueType, Bytes)>>` result produced by
/// `SSTableReader::get_with_type`.
fn resolve_typed_result(
    result: Option<(ValueType, Bytes)>,
    blob_store: &BlobStore,
) -> Result<Option<Bytes>> {
    match result {
        None => Ok(None),
        Some((ty, val)) => resolve_value(ty, Some(val), blob_store),
    }
}
