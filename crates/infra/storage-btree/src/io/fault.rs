//! Deterministic fault-injection backend.
//!
//! `FaultyBackend` wraps a real backend and injects faults according to a
//! deterministic [`FaultSchedule`]. It is intended for durability and recovery
//! testing only.

use std::collections::HashMap;
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::io::{Boundary, OpFamily, OpenOptions, StorageBackend, StorageFile};

/// A deterministic schedule of faults.
///
/// The `seed` is used to derive any randomised workload state; rules are
/// evaluated in order, and the first matching rule wins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FaultSchedule {
    /// Seed for any deterministic random generation performed by the test.
    pub seed: u64,
    /// Ordered list of rules to apply.
    pub rules: Vec<FaultRule>,
}

impl FaultSchedule {
    /// Create a schedule from a seed and a list of rules.
    pub fn new(seed: u64, rules: Vec<FaultRule>) -> Self {
        Self { seed, rules }
    }
}

/// A single fault-injection rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaultRule {
    /// Fail the `n`th call to `op` with an I/O error of the given kind.
    FailNth {
        /// Operation family to target.
        op: OpFamily,
        /// 1-based call index to fail.
        n: usize,
        /// Error kind to report.
        error: ErrorKind,
    },
    /// Fail every `period`th call to `op` after an initial `offset`.
    FailEvery {
        /// Operation family to target.
        op: OpFamily,
        /// Number of calls to skip before the first failure.
        offset: usize,
        /// Interval between subsequent failures.
        period: usize,
        /// Error kind to report.
        error: ErrorKind,
    },
    /// Corrupt the `n`th read of `op` by XORing a region with a byte pattern.
    CorruptReadNth {
        /// Operation family to target.
        op: OpFamily,
        /// 1-based read index to corrupt.
        n: usize,
        /// Offset within the read buffer to start corrupting.
        offset: u64,
        /// Number of bytes to corrupt.
        len: usize,
        /// XOR mask applied to the selected bytes.
        xor: u8,
    },
    /// Truncate the `n`th write of `op` to `truncate_to` bytes.
    PartialWriteNth {
        /// Operation family to target.
        op: OpFamily,
        /// 1-based write index to truncate.
        n: usize,
        /// Length to write instead of the full buffer.
        truncate_to: usize,
    },
    /// Drop buffered appends (power-loss simulation) for value-log appends.
    DropAppends,
}

/// Thread-safe state shared by a `FaultyBackend` and its files.
struct FaultState {
    /// Per-operation-family counters.
    counters: HashMap<OpFamily, usize>,
    /// Per-family write counter for `PartialWriteNth`.
    write_counters: HashMap<OpFamily, usize>,
    /// Per-family read counter for `CorruptReadNth`.
    read_counters: HashMap<OpFamily, usize>,
    /// Recorded sequence of operations.
    log: Vec<(Boundary, OpFamily)>,
    /// Last durably synced length for each file path.
    last_synced_len: HashMap<PathBuf, u64>,
}

impl FaultState {
    fn new() -> Self {
        Self {
            counters: HashMap::new(),
            write_counters: HashMap::new(),
            read_counters: HashMap::new(),
            log: Vec::new(),
            last_synced_len: HashMap::new(),
        }
    }

    fn bump(&mut self, boundary: Boundary) -> usize {
        let family = boundary.family();
        let count = self.counters.entry(family).or_insert(0);
        *count += 1;
        self.log.push((boundary, family));
        *count
    }
}

/// Fault-injecting storage backend.
///
/// Wraps an inner backend and applies the first matching rule from the schedule
/// at each tagged boundary. It also records the exact operation sequence and
/// can simulate power loss by truncating files to their last fsynced length.
pub struct FaultyBackend {
    inner: Arc<dyn StorageBackend>,
    state: Arc<Mutex<FaultState>>,
    schedule: FaultSchedule,
}

impl FaultyBackend {
    /// Wrap `backend` with the supplied fault schedule.
    pub fn new(backend: Arc<dyn StorageBackend>, schedule: FaultSchedule) -> Self {
        Self {
            inner: backend,
            state: Arc::new(Mutex::new(FaultState::new())),
            schedule,
        }
    }

    fn apply_fault(&self, boundary: Boundary) -> IoResult<()> {
        let mut state = self.state.lock().expect("fault state lock poisoned");
        let count = state.bump(boundary);
        for rule in &self.schedule.rules {
            match rule {
                FaultRule::FailNth { op, n, error } if boundary.family() == *op && count == *n => {
                    return Err(IoError::new(
                        *error,
                        format!("injected fault at {boundary:?}"),
                    ));
                }
                FaultRule::FailEvery {
                    op,
                    offset,
                    period,
                    error,
                } if boundary.family() == *op
                    && count >= *offset
                    && *period > 0
                    && (count - *offset).is_multiple_of(*period) =>
                {
                    return Err(IoError::new(
                        *error,
                        format!("injected fault at {boundary:?}"),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl StorageBackend for FaultyBackend {
    fn open(&self, path: &Path, opts: OpenOptions) -> IoResult<Box<dyn StorageFile>> {
        let file = self.inner.open(path, opts)?;
        Ok(Box::new(FaultyFile {
            path: path.to_path_buf(),
            inner: file,
            state: Arc::clone(&self.state),
        }))
    }

    fn rename(&self, from: &Path, to: &Path) -> IoResult<()> {
        self.apply_fault(Boundary::MetaRename)?;
        self.inner.rename(from, to)
    }

    fn remove(&self, path: &Path) -> IoResult<()> {
        self.inner.remove(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn sync_dir(&self, path: &Path) -> IoResult<()> {
        self.apply_fault(Boundary::MetaDirSync)?;
        self.inner.sync_dir(path)
    }

    fn pre_op(&self, boundary: Boundary) -> IoResult<()> {
        self.apply_fault(boundary)
    }

    fn corrupt_read(&self, boundary: Boundary, buf: &mut [u8], offset: u64) -> IoResult<()> {
        let family = boundary.family();
        let mut state = self.state.lock().expect("fault state lock poisoned");
        let count = state.read_counters.entry(family).or_insert(0);
        *count += 1;
        let count = *count;
        for rule in &self.schedule.rules {
            if let FaultRule::CorruptReadNth {
                op,
                n,
                offset: corrupt_offset,
                len,
                xor,
            } = rule
                && family == *op
                && count == *n
            {
                let corrupt_end = corrupt_offset.saturating_add(*len as u64);
                let read_end = offset + buf.len() as u64;
                if *corrupt_offset < read_end && offset < corrupt_end {
                    let start_in_buf = corrupt_offset.saturating_sub(offset) as usize;
                    let end_in_buf = (corrupt_end.min(read_end) - offset) as usize;
                    for b in &mut buf[start_in_buf..end_in_buf] {
                        *b ^= *xor;
                    }
                }
                return Ok(());
            }
        }
        Ok(())
    }

    fn truncate_write(&self, boundary: Boundary, buf_len: usize) -> IoResult<usize> {
        let family = boundary.family();
        let mut state = self.state.lock().expect("fault state lock poisoned");
        let count = state.write_counters.entry(family).or_insert(0);
        *count += 1;
        let count = *count;
        for rule in &self.schedule.rules {
            if let FaultRule::PartialWriteNth { op, n, truncate_to } = rule
                && family == *op
                && count == *n
            {
                return Ok(*truncate_to);
            }
        }
        Ok(buf_len)
    }

    fn drop_append(&self, boundary: Boundary) -> bool {
        if boundary.family() != OpFamily::ValueLogAppend {
            return false;
        }
        self.schedule
            .rules
            .iter()
            .any(|r| matches!(r, FaultRule::DropAppends))
    }

    fn operation_log(&self) -> Vec<(Boundary, OpFamily)> {
        self.state
            .lock()
            .expect("fault state lock poisoned")
            .log
            .clone()
    }

    fn crash(&self) {
        let paths: Vec<(PathBuf, u64)> = {
            let state = self.state.lock().expect("fault state lock poisoned");
            state
                .last_synced_len
                .iter()
                .map(|(p, &len)| (p.clone(), len))
                .collect()
        };
        for (path, len) in paths {
            if let Ok(file) = self
                .inner
                .open(&path, OpenOptions::new().read(true).write(true))
            {
                let _ = file.set_len(len);
            }
        }
    }
}

/// A file opened through a `FaultyBackend`.
struct FaultyFile {
    path: PathBuf,
    inner: Box<dyn StorageFile>,
    state: Arc<Mutex<FaultState>>,
}

impl StorageFile for FaultyFile {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> IoResult<()> {
        self.inner.read_at(buf, offset)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> IoResult<()> {
        self.inner.write_at(buf, offset)
    }

    fn sync(&self) -> IoResult<()> {
        self.inner.sync()?;
        let len = self.inner.len()?;
        let mut state = self.state.lock().expect("fault state lock poisoned");
        state.last_synced_len.insert(self.path.clone(), len);
        Ok(())
    }

    fn set_len(&self, len: u64) -> IoResult<()> {
        self.inner.set_len(len)
    }

    fn len(&self) -> IoResult<u64> {
        self.inner.len()
    }
}
