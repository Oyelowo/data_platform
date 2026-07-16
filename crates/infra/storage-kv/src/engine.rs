//! LSM-tree engine implementation.
//!
//! This first implementation uses synchronous flush and compaction to keep the
//! recovery and concurrency story simple and verifiable. Background workers are
//! a planned performance optimization.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bytes::Bytes;

use crate::compaction::pick_compaction;
use crate::cursor::LsmCursor;
use crate::flush::flush_memtable;
use crate::immutable::{ImmutableMemTables, sstable_path};
use crate::internal_key::{extract_user_key, parse_internal_key, ValueType};
use crate::manifest::Manifest;
use crate::memtable::MemTable;
use crate::options::LsmOptions;
use crate::recovery;
use crate::sstable::builder::{SSTableBuilder, SSTableBuilderOptions};
use crate::sstable::reader::SSTableReader;
use crate::transaction::LsmTransaction;
use crate::version::FileMetaData;
use crate::version_set::{VersionEdit, VersionSet};
use crate::wal::WalRecord;
use crate::worker::{Worker, WorkerCommand};
use crate::{Error, Result, SequenceNumber};

/// Public handle to an LSM-tree engine.
pub struct LsmEngine {
    inner: LsmEngineInner,
    worker: Option<Worker>,
}

impl LsmEngine {
    /// Open or create an LSM engine at `path`.
    pub fn open(path: impl AsRef<Path>, options: LsmOptions) -> Result<Self> {
        options.validate()?;
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;

        let version_set = Arc::new(VersionSet::new(options.num_levels));
        let (recovered_mem, last_sequence) =
            recovery::recover(&path, &options, &version_set)?;

        let wal = storage_wal::Wal::open(
            path.join("wal"),
            storage_wal::WalOptions {
                segment_size: options.wal_segment_size,
                ..Default::default()
            },
        )?;

        let manifest_path = path.join("MANIFEST-000001");
        let manifest = Arc::new(Mutex::new(if manifest_path.exists() {
            Manifest::open(&manifest_path)?
        } else {
            Manifest::create(&manifest_path)?
        }));
        std::fs::write(path.join("CURRENT"), "MANIFEST-000001\n")?;

        let state = Arc::new(Mutex::new(EngineState {
            path: path.clone(),
            options,
            wal,
            manifest,
            version_set,
            memtable: Arc::new(recovered_mem),
            immutable: ImmutableMemTables::new(options.max_write_buffer_number.saturating_sub(1).max(1)),
            active_flushes: 0,
            last_sequence,
        }));

        let (worker, flush_sender) = Worker::spawn(Arc::clone(&state));

        Ok(Self {
            inner: LsmEngineInner {
                state,
                flush_sender,
            },
            worker: Some(worker),
        })
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        let seq = self.inner.last_sequence();
        self.inner.get(key, seq)
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.inner.write(key, value, self.inner.next_sequence())
    }

    pub fn delete(&self, key: &[u8]) -> Result<()> {
        self.inner.delete(key, self.inner.next_sequence())
    }

    pub fn sync(&self) -> Result<()> {
        self.inner.sync()
    }
}

impl Drop for LsmEngine {
    fn drop(&mut self) {
        // Shut down the background worker, waiting for all pending flushes.
        if let Some(worker) = self.worker.take() {
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
        let seq = self.inner.last_sequence();
        Ok(LsmTransaction::new(self.inner.clone(), opts.read_only, seq))
    }

    fn get(&self, key: &[u8]) -> Result<Option<Bytes>> {
        self.get(key)
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> Result<Self::Cursor> {
        let snapshot = self.inner.last_sequence();
        Ok(LsmCursor::new(
            self.inner.clone(),
            start.map(|s| s.to_vec()),
            end.map(|e| e.to_vec()),
            snapshot,
        ))
    }

    fn stats(&self) -> Result<storage_traits::EngineStats> {
        Ok(storage_traits::EngineStats::default())
    }

    fn sync(&self) -> Result<()> {
        self.sync()
    }
}

/// Cloneable inner handle shared by engine, transactions, and cursors.
#[derive(Clone)]
pub struct LsmEngineInner {
    state: Arc<Mutex<EngineState>>,
    flush_sender: crossbeam_channel::Sender<WorkerCommand>,
}

impl LsmEngineInner {
    fn next_sequence(&self) -> SequenceNumber {
        let mut state = self.state.lock().unwrap();
        state.last_sequence = state.last_sequence.wrapping_add(1);
        state.last_sequence
    }

    fn last_sequence(&self) -> SequenceNumber {
        self.state.lock().unwrap().last_sequence
    }

    pub(crate) fn write(&self, key: &[u8], value: &[u8], sequence: SequenceNumber) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let record = WalRecord::new_put(key, value, sequence);
        Self::append_wal(&mut state, &record)?;
        Arc::get_mut(&mut state.memtable)
            .expect("single owner")
            .put(key, sequence, value);
        let queued = Self::maybe_freeze(&mut state)?;
        drop(state);
        if queued {
            let _ = self.flush_sender.send(WorkerCommand::Wake);
        }
        Ok(())
    }

    pub(crate) fn delete(&self, key: &[u8], sequence: SequenceNumber) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let record = WalRecord::new_delete(key, sequence);
        Self::append_wal(&mut state, &record)?;
        Arc::get_mut(&mut state.memtable)
            .expect("single owner")
            .delete(key, sequence);
        let queued = Self::maybe_freeze(&mut state)?;
        drop(state);
        if queued {
            let _ = self.flush_sender.send(WorkerCommand::Wake);
        }
        Ok(())
    }

    fn append_wal(state: &mut EngineState, record: &WalRecord) -> Result<()> {
        let mut payload = Vec::new();
        record.encode(&mut payload);
        state.wal.append(&payload, storage_wal::Durability::Immediate)?;
        Ok(())
    }

    /// Freeze the current MemTable when it reaches `write_buffer_size`.
    ///
    /// Returns `true` if a MemTable was queued for background flush. If the
    /// immutable queue is already full, the MemTable is flushed synchronously
    /// under the lock to apply backpressure.
    fn maybe_freeze(state: &mut EngineState) -> Result<bool> {
        if state.memtable.approximate_size() >= state.options.write_buffer_size {
            let old_mem = std::mem::replace(&mut state.memtable, Arc::new(MemTable::new()));
            if state.immutable.is_full() {
                // Backpressure path: flush synchronously when the worker can't
                // keep up. This avoids unbounded memory growth.
                flush_memtable(
                    &state.path,
                    &state.options,
                    &state.version_set,
                    &state.manifest,
                    &old_mem,
                    state.last_sequence,
                )?;
                Self::maybe_compact(state)?;
                Ok(false)
            } else {
                state.immutable.push(old_mem);
                Ok(true)
            }
        } else {
            Ok(false)
        }
    }

    pub(crate) fn maybe_compact(state: &mut EngineState) -> Result<()> {
        loop {
            let version = state.version_set.current();
            let job = match pick_compaction(&version, &state.options) {
                Some(j) => j,
                None => break,
            };
            let output_level = job.level + 1;
            // Collect input files.
            let mut inputs: Vec<FileMetaData> = Vec::new();
            for file in job.inputs.iter().flat_map(|v| v.iter()) {
                inputs.push(file.clone());
            }

            // Run compaction: read all entries, sort by raw internal-key bytes,
            // and keep only the newest version of each user key.
            let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for file in &inputs {
                let path = sstable_path(&state.path, file.number);
                let mut reader = SSTableReader::open(path)?;
                let mut iter = reader.iter()?;
                iter.seek_to_first()?;
                while iter.valid() {
                    entries.push((iter.key().to_vec(), iter.value().to_vec()));
                    iter.next()?;
                }
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));

            let mut deduped: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for (key, value) in entries {
                if let Some(last) = deduped.last_mut() {
                    let last_user = extract_user_key(&last.0).to_vec();
                    let user_key = extract_user_key(&key).to_vec();
                    if last_user == user_key {
                        // Newer sequence sorts later in raw-byte order.
                        *last = (key, value);
                        continue;
                    }
                }
                deduped.push((key, value));
            }

            let builder_opts = SSTableBuilderOptions {
                block_size: state.options.block_size,
                block_restart_interval: state.options.block_restart_interval,
                bloom_bits_per_key: state.options.bloom_bits_per_key,
            };

            let mut current_builder: Option<(u64, SSTableBuilder)> = None;
            let mut output_files: Vec<FileMetaData> = Vec::new();
            let mut last_user_key: Option<Vec<u8>> = None;

            for (key, value) in deduped {
                let user_key = extract_user_key(&key).to_vec();
                if Some(&user_key) == last_user_key.as_ref() {
                    continue;
                }
                last_user_key = Some(user_key);

                let (_, ty) = parse_internal_key(&key).unwrap();
                if output_level + 1 >= state.options.num_levels && ty == ValueType::Deletion {
                    continue;
                }

                if current_builder.is_none() {
                    let file_number = state.version_set.new_file_number();
                    let path = sstable_path(&state.path, file_number);
                    current_builder = Some((file_number, SSTableBuilder::open(path, builder_opts)?));
                }
                let (_num, builder) = current_builder.as_mut().unwrap();
                builder.add(&key, &value)?;

                if builder.current_size_estimate() >= state.options.target_file_size_base as usize {
                    let (n, b) = current_builder.take().unwrap();
                    let built = b.finish()?;
                    output_files.push(FileMetaData {
                        number: n,
                        file_size: built.file_size,
                        smallest: built.smallest_key,
                        largest: built.largest_key,
                    });
                    last_user_key = None;
                }
            }

            if let Some((n, b)) = current_builder {
                let built = b.finish()?;
                output_files.push(FileMetaData {
                    number: n,
                    file_size: built.file_size,
                    smallest: built.smallest_key,
                    largest: built.largest_key,
                });
            }

            // Apply version edit.
            let mut edit = VersionEdit {
                last_sequence: state.last_sequence,
                next_file_number: state.version_set.next_file_number(),
                ..Default::default()
            };
            for file in &inputs {
                edit.deleted_files.push((job.level, file.number));
            }
            for file in output_files {
                edit.new_files.push((output_level, file));
            }
            state.manifest.lock().unwrap().log_edit(&edit)?;
            state.version_set.apply(edit)?;

            // Delete input files.
            for file in &inputs {
                let path = sstable_path(&state.path, file.number);
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    pub(crate) fn get(&self, key: &[u8], snapshot: SequenceNumber) -> Result<Option<Bytes>> {
        let state = self.state.lock().unwrap();

        if let Some(v) = state.memtable.get(key, snapshot) {
            return Ok(v);
        }

        for mem in state.immutable.iter_newest_first() {
            if let Some(v) = mem.get(key, snapshot) {
                return Ok(v);
            }
        }

        let version = state.version_set.current();
        drop(state);

        for file in version.levels[0].iter().rev() {
            let path = sstable_path(&self.path(), file.number);
            let mut reader = SSTableReader::open(path)?;
            if let Some(v) = reader.get(key)? {
                return Ok(v);
            }
        }

        for level in 1..version.levels.len() {
            if let Some(file) = version.pick_level_file(level, key) {
                let path = sstable_path(&self.path(), file.number);
                let mut reader = SSTableReader::open(path)?;
                if let Some(v) = reader.get(key)? {
                    return Ok(v);
                }
            }
        }

        Ok(None)
    }

    /// Returns all live user entries in `[start, end)` ordered by user key,
    /// using `snapshot` for MVCC visibility.
    pub(crate) fn scan_entries(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        snapshot: SequenceNumber,
    ) -> Vec<(Bytes, Bytes)> {
        let mut candidates: std::collections::BTreeMap<Vec<u8>, (SequenceNumber, Option<Bytes>)> =
            std::collections::BTreeMap::new();

        let add_entries = |candidates: &mut std::collections::BTreeMap<
            Vec<u8>,
            (SequenceNumber, Option<Bytes>),
        >,
                           entries: &[(Vec<u8>, Bytes)]| {
            for (ikey, value) in entries {
                let user_key = extract_user_key(ikey).to_vec();
                let (seq, ty) = parse_internal_key(ikey).unwrap();
                if seq > snapshot {
                    continue;
                }
                candidates
                    .entry(user_key)
                    .and_modify(|(es, ev)| {
                        if seq > *es {
                            *es = seq;
                            *ev = match ty {
                                ValueType::Value => Some(value.clone()),
                                ValueType::Deletion => None,
                            };
                        }
                    })
                    .or_insert((seq, match ty {
                        ValueType::Value => Some(value.clone()),
                        ValueType::Deletion => None,
                    }));
            }
        };

        {
            let state = self.state.lock().unwrap();
            add_entries(&mut candidates, &state.memtable.iter());
            for mem in state.immutable.iter_newest_first() {
                add_entries(&mut candidates, &mem.iter());
            }

            let version = state.version_set.current();
            drop(state);

            for level in 0..version.levels.len() {
                for file in &version.levels[level] {
                    let path = sstable_path(&self.path(), file.number);
                    let mut reader = match SSTableReader::open(path) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    let mut iter = match reader.iter() {
                        Ok(i) => i,
                        Err(_) => continue,
                    };
                    let _ = iter.seek_to_first();
                    while iter.valid() {
                        let ikey = iter.key();
                        let user_key = extract_user_key(ikey).to_vec();
                        let (seq, ty) = parse_internal_key(ikey).unwrap();
                        if seq <= snapshot {
                            candidates
                                .entry(user_key)
                                .and_modify(|(es, ev)| {
                                    if seq > *es {
                                        *es = seq;
                                        *ev = match ty {
                                            ValueType::Value => Some(Bytes::copy_from_slice(iter.value())),
                                            ValueType::Deletion => None,
                                        };
                                    }
                                })
                                .or_insert((seq, match ty {
                                    ValueType::Value => Some(Bytes::copy_from_slice(iter.value())),
                                    ValueType::Deletion => None,
                                }));
                        }
                        let _ = iter.next();
                    }
                }
            }
        }

        candidates
            .into_iter()
            .filter_map(|(key, (_, value))| {
                if let Some(s) = start && key.as_slice() < s {
                    return None;
                }
                if let Some(e) = end && key.as_slice() >= e {
                    return None;
                }
                value.map(|v| (Bytes::from(key), v))
            })
            .collect()
    }

    pub(crate) fn sync(&self) -> Result<()> {
        // Wake the worker so any queued immutable MemTables are flushed.
        let _ = self.flush_sender.send(WorkerCommand::Wake);
        loop {
            let state = self.state.lock().unwrap();
            if state.immutable.is_empty() && state.active_flushes == 0 {
                break;
            }
            drop(state);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let state = self.state.lock().unwrap();
        // WAL already fsyncs every append. Force manifest sync for metadata.
        state.manifest.lock().unwrap().sync()
    }

    fn path(&self) -> PathBuf {
        self.state.lock().unwrap().path.clone()
    }
}

pub(crate) struct EngineState {
    pub(crate) path: PathBuf,
    pub(crate) options: LsmOptions,
    pub(crate) wal: storage_wal::Wal,
    pub(crate) manifest: Arc<Mutex<Manifest>>,
    pub(crate) version_set: Arc<VersionSet>,
    pub(crate) memtable: Arc<MemTable>,
    pub(crate) immutable: ImmutableMemTables,
    pub(crate) active_flushes: usize,
    pub(crate) last_sequence: SequenceNumber,
}
