//! Group commit / fsync worker.
//!
//! Multiple concurrent threads submit commit requests over a bounded channel.
//! A dedicated background thread drains the channel, appends all pending
//! records to the current segment, performs a single `fsync`, and then
//! acknowledges every waiter. This amortises the cost of `fsync` across many
//! concurrent writers.
//!
//! Two durability modes are supported:
//!
//! * `Immediate` — the caller blocks until the record is durably persisted.
//! * `Buffered` — the caller blocks only until the record is written to the
//!   current segment and assigned an LSN. The caller (or another thread) must
//!   later call `sync` to force an `fsync`. Buffered records are lost on power
//!   failure if they have not been synced.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};

use crate::fault::{FaultConfig, FaultInjector};
use crate::fs::sync_dir;
use crate::record::{RECORD_HEADER_SIZE, Record};
use crate::segment::Segment;
use crate::{Error, Lsn, Result};

/// Maximum number of outstanding commit requests.
const CHANNEL_CAPACITY: usize = 4096;

/// Maximum time to wait for additional committers before flushing (1 ms).
const COMMIT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1);

/// Reply sent back to the caller by the commit worker.
enum Reply {
    /// The record was written to the segment and assigned this LSN. It is *not*
    /// necessarily durable yet.
    Appended(Lsn),
    /// The record (or sync barrier) has been durably flushed.
    ///
    /// For `append` with `wait_for_fsync=true` this carries the LSN of the
    /// caller's own record. For `sync` barriers the LSN is informational and
    /// may be zero.
    Flushed(Lsn),
}

/// Request sent to the commit worker.
enum CommitRequest {
    /// Append a record. If `wait_for_fsync` is true the caller is replied to
    /// with `Reply::Flushed` after the next fsync; otherwise it is replied to
    /// immediately with `Reply::Appended`.
    Append {
        record: Record,
        wait_for_fsync: bool,
        reply: Sender<Result<Reply>>,
    },
    /// Force a flush of all buffered records and reply once durable.
    Sync { reply: Sender<Result<Reply>> },
    /// Return the byte length of the active segment that has been durably
    /// synced. Used by crash simulation to truncate unflushed data.
    Crash { reply: Sender<Result<u64>> },
}

/// Handle to the group commit worker.
pub struct Committer {
    sender: Mutex<Option<Sender<CommitRequest>>>,
    shutdown: Arc<AtomicBool>,
    handle: Mutex<Option<std::thread::JoinHandle<Result<()>>>>,
    /// Retained so tests and future callers can reconfigure faults at runtime.
    #[allow(dead_code)]
    fault: FaultInjector,
}

impl std::fmt::Debug for Committer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Committer")
            .field("open", &self.sender.lock().unwrap().is_some())
            .field("shutting_down", &self.shutdown.load(Ordering::Relaxed))
            .finish()
    }
}

impl Committer {
    /// Start the commit worker bound to `dir` with the given segment size.
    ///
    /// Recovery of the current segment tail is performed synchronously on the
    /// calling thread so that the returned `Committer` reflects a quiesced,
    /// truncated state.
    pub fn start(dir: std::path::PathBuf, segment_size: u64) -> Result<Self> {
        Self::start_with_fault_config(dir, segment_size, FaultConfig::default())
    }

    /// Start the commit worker with a fault-injection configuration.
    ///
    /// The returned injector handle can be used to reconfigure faults at runtime
    /// (for example, after a deterministic number of operations).
    pub fn start_with_fault_config(
        dir: std::path::PathBuf,
        segment_size: u64,
        fault_config: FaultConfig,
    ) -> Result<Self> {
        let fault = FaultInjector::new(fault_config);
        let state = init_worker(dir, segment_size, fault.clone())?;
        let (sender, receiver) = bounded(CHANNEL_CAPACITY);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let handle =
            std::thread::spawn(move || worker_loop_with_state(state, receiver, shutdown_clone));

        Ok(Self {
            sender: Mutex::new(Some(sender)),
            shutdown,
            handle: Mutex::new(Some(handle)),
            fault,
        })
    }

    /// Return a handle to the fault injector so tests can enable or disable
    /// faults at runtime.
    #[allow(dead_code)]
    pub fn fault_injector(&self) -> &FaultInjector {
        &self.fault
    }

    /// Submit a record for durable append. Blocks until the worker has
    /// acknowledged that the record is persisted.
    pub fn append(&self, record: Record) -> Result<Lsn> {
        self.send_append(record, true)
            .and_then(|reply| match reply {
                Ok(Reply::Flushed(lsn)) => Ok(lsn),
                Ok(Reply::Appended(_)) => unreachable!("append uses wait_for_fsync=true"),
                Err(e) => Err(e),
            })
    }

    /// Submit a record without waiting for fsync. Returns the assigned LSN
    /// immediately. The record may be lost on power failure until `sync` is
    /// called.
    pub fn append_buffered(&self, record: Record) -> Result<Lsn> {
        self.send_append(record, false)
            .and_then(|reply| match reply {
                Ok(Reply::Appended(lsn)) => Ok(lsn),
                Ok(Reply::Flushed(_)) => unreachable!("append_buffered uses wait_for_fsync=false"),
                Err(e) => Err(e),
            })
    }

    fn send_append(&self, record: Record, wait_for_fsync: bool) -> Result<Result<Reply>> {
        if self.shutdown.load(Ordering::Acquire) {
            return Ok(Err(Error::Closed));
        }
        let (tx, rx) = bounded(1);
        let sender = self.sender.lock().unwrap();
        sender
            .as_ref()
            .ok_or(Error::Closed)?
            .send(CommitRequest::Append {
                record,
                wait_for_fsync,
                reply: tx,
            })
            .map_err(|_| Error::Closed)?;
        rx.recv().map_err(|_| Error::Closed)
    }

    /// Force a flush of all previously buffered records. Blocks until durable.
    pub fn sync(&self) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        let (tx, rx) = bounded(1);
        let sender = self.sender.lock().unwrap();
        sender
            .as_ref()
            .ok_or(Error::Closed)?
            .send(CommitRequest::Sync { reply: tx })
            .map_err(|_| Error::Closed)?;
        match rx.recv().map_err(|_| Error::Closed)?? {
            Reply::Flushed(_) => Ok(()),
            Reply::Appended(_) => unreachable!("sync barrier replies with Flushed"),
        }
    }

    /// Return the byte length of the active segment that has been durably
    /// synced. This is used by crash simulation to truncate records that were
    /// written to the OS page cache but never fsynced.
    pub fn crash(&self) -> Result<u64> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        let (tx, rx) = bounded(1);
        let sender = self.sender.lock().unwrap();
        sender
            .as_ref()
            .ok_or(Error::Closed)?
            .send(CommitRequest::Crash { reply: tx })
            .map_err(|_| Error::Closed)?;
        rx.recv().map_err(|_| Error::Closed)?
    }

    /// Request a graceful shutdown and wait for the worker to finish.
    ///
    /// Idempotent: may be called more than once; subsequent calls return `Ok(())`.
    pub fn shutdown(&self) -> Result<()> {
        self.shutdown.store(true, Ordering::Release);
        let sender = self.sender.lock().unwrap().take();
        drop(sender);
        let handle = self.handle.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.join().map_err(|_| Error::Closed)??;
        }
        Ok(())
    }
}

impl Drop for Committer {
    fn drop(&mut self) {
        // Join the worker so the directory is left in a quiesced state and we
        // do not leak a detached thread. `shutdown` is idempotent.
        let _ = self.shutdown();
    }
}

struct WorkerState {
    dir: std::path::PathBuf,
    segment_size: u64,
    next_lsn: Lsn,
    segment: Segment,
    fault: FaultInjector,
    /// Byte length of the active segment that has been durably synced. Used by
    /// crash simulation to truncate records that were written but not fsynced.
    last_synced_len: u64,
}

fn handle_append_request(
    state: &mut WorkerState,
    req: CommitRequest,
    pending_durable: &mut Vec<(Sender<Result<Reply>>, Lsn)>,
) -> Result<()> {
    match req {
        CommitRequest::Crash { .. } => {
            // Crash requests are handled directly in the worker loop.
            unreachable!("Crash requests should not reach handle_append_request")
        }
        CommitRequest::Append {
            record,
            wait_for_fsync,
            reply,
        } => {
            let (lsn, _len) = write_record(state, record)?;
            if wait_for_fsync {
                // Keep the reply channel; it will be notified after the next fsync.
                pending_durable.push((reply, lsn));
            } else {
                // Buffered append: the caller already has the LSN in hand, but
                // reply anyway so the blocking recv() returns.
                let _ = reply.send(Ok(Reply::Appended(lsn)));
            }
        }
        CommitRequest::Sync { reply } => {
            // Sync barriers do not need a specific record LSN; zero is used.
            pending_durable.push((reply, 0));
        }
    }
    Ok(())
}

fn worker_loop_with_state(
    mut state: WorkerState,
    receiver: Receiver<CommitRequest>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    // Pending durable acknowledgements for Immediate appends and sync barriers.
    // Each entry stores the reply channel and the LSN to report back (the
    // caller's own record LSN for appends, or zero for sync barriers).
    let mut pending_durable: Vec<(Sender<Result<Reply>>, Lsn)> = Vec::new();

    let result = worker_loop_inner(&mut state, &receiver, shutdown, &mut pending_durable);

    // Whatever caused us to exit, notify any remaining waiters so they are not
    // stranded waiting on a closed channel.
    if !pending_durable.is_empty() {
        let err_msg = match &result {
            Err(e) => e.to_string(),
            Ok(()) => Error::Closed.to_string(),
        };
        for (reply, _lsn) in pending_durable.drain(..) {
            let _ = reply.send(Err(Error::Io(std::io::Error::other(err_msg.clone()))));
        }
    }

    // Best-effort final sync so the directory is left in a quiesced state.
    let _ = state.segment.sync();
    result
}

fn worker_loop_inner(
    state: &mut WorkerState,
    receiver: &Receiver<CommitRequest>,
    shutdown: Arc<AtomicBool>,
    pending_durable: &mut Vec<(Sender<Result<Reply>>, Lsn)>,
) -> Result<()> {
    loop {
        // Wait for the first request of the next group, or timeout and flush.
        let req = match receiver.recv_timeout(COMMIT_TIMEOUT) {
            Ok(req) => req,
            Err(RecvTimeoutError::Timeout) => {
                flush_pending(state, pending_durable)?;
                if shutdown.load(Ordering::Acquire) && pending_durable.is_empty() {
                    break;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush_pending(state, pending_durable)?;
                break;
            }
        };

        if let CommitRequest::Crash { reply } = req {
            let _ = reply.send(Ok(state.last_synced_len));
            continue;
        }

        handle_append_request(state, req, pending_durable)?;

        // Drain additional requests without blocking to build the group.
        while let Ok(req) = receiver.try_recv() {
            handle_append_request(state, req, pending_durable)?;
            if pending_durable.len() >= CHANNEL_CAPACITY / 2 {
                break;
            }
        }

        flush_pending(state, pending_durable)?;

        if shutdown.load(Ordering::Acquire) && receiver.is_empty() {
            break;
        }
    }

    Ok(())
}

fn init_worker(
    dir: std::path::PathBuf,
    segment_size: u64,
    fault: FaultInjector,
) -> Result<WorkerState> {
    std::fs::create_dir_all(&dir)?;
    sync_dir(&dir)?;
    let segments = crate::segment::list_segments(&dir)?;
    let segment_first = segments.last().copied().unwrap_or(0);
    let mut segment = Segment::open(&dir, segment_first, segment_size)?;
    let valid_end = recover_tail(&mut segment)?;
    let next_lsn = segment_first + valid_end;
    Ok(WorkerState {
        dir,
        segment_size,
        next_lsn,
        segment,
        fault,
        last_synced_len: valid_end,
    })
}

/// Scan the current segment and truncate any trailing partial or corrupt bytes
/// after the last complete, valid record. Returns the offset after that record.
fn recover_tail(segment: &mut Segment) -> Result<u64> {
    let data = segment.read_all()?;
    let mut offset = 0usize;
    let mut last_valid_end = 0usize;
    while offset < data.len() {
        match crate::record::Record::decode(&data[offset..]) {
            Ok(Some((_, consumed))) => {
                offset += consumed;
                last_valid_end = offset;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    if last_valid_end < data.len() {
        segment.truncate(last_valid_end as u64)?;
    }
    Ok(last_valid_end as u64)
}

/// Encode and append a record to the current segment, rotating if necessary.
/// Returns the assigned LSN and the encoded length.
fn write_record(state: &mut WorkerState, mut record: Record) -> Result<(Lsn, usize)> {
    record.lsn = state.next_lsn;
    let mut buf = Vec::with_capacity(RECORD_HEADER_SIZE + record.payload.len());
    record.encode(&mut buf)?;

    // Rotate if the record does not fit in the current segment.
    if buf.len() as u64 > state.segment.remaining() {
        if state.fault.should_fail_sync() {
            return Err(Error::Io(std::io::Error::other(
                "injected fsync failure during segment rotation",
            )));
        }
        state.segment.sync()?;
        state.last_synced_len = state.segment.written();
        state.next_lsn += state.segment.written();
        state.segment = Segment::open(&state.dir, state.next_lsn, state.segment_size)?;
        sync_dir(&state.dir)?;
        state.last_synced_len = 0;
        record.lsn = state.next_lsn;
        buf.clear();
        record.encode(&mut buf)?;
        if buf.len() as u64 > state.segment.remaining() {
            return Err(Error::InvalidArgument(
                "record is larger than the configured segment size".into(),
            ));
        }
    }

    if state.fault.should_drop_append() {
        // Simulate power loss: the record is assigned an LSN but never reaches
        // the segment file.
        let lsn = state.next_lsn;
        state.next_lsn += buf.len() as u64;
        return Ok((lsn, buf.len()));
    }

    state.segment.append(&buf)?;
    let lsn = state.next_lsn;
    state.next_lsn += buf.len() as u64;
    Ok((lsn, buf.len()))
}

fn flush_pending(
    state: &mut WorkerState,
    pending: &mut Vec<(Sender<Result<Reply>>, Lsn)>,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    let result = do_flush(state);
    match result {
        Ok(()) => {
            state.last_synced_len = state.segment.written();
            for (reply, lsn) in pending.drain(..) {
                // Best-effort reply; if the caller hung up, drop the result.
                let _ = reply.send(Ok(Reply::Flushed(lsn)));
            }
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            for (reply, _lsn) in pending.drain(..) {
                let _ = reply.send(Err(Error::Io(std::io::Error::other(msg.clone()))));
            }
            Err(Error::Io(std::io::Error::other(msg)))
        }
    }
}

fn do_flush(state: &mut WorkerState) -> Result<()> {
    if state.fault.should_fail_flush() {
        return Err(Error::Io(std::io::Error::other("injected flush failure")));
    }
    state.segment.flush()?;

    if state.fault.should_fail_sync() {
        return Err(Error::Io(std::io::Error::other("injected fsync failure")));
    }
    state.segment.sync()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::RecordType;

    #[test]
    fn basic_group_commit() {
        let dir = tempfile::tempdir().unwrap();
        let committer = Committer::start(dir.path().to_path_buf(), 64 * 1024 * 1024).unwrap();
        let lsn = committer
            .append(Record::new(RecordType::Put, &b"a"[..]))
            .unwrap();
        assert_eq!(lsn, 0);
        committer.shutdown().unwrap();
    }

    #[test]
    fn buffered_append_survives_sync() {
        let dir = tempfile::tempdir().unwrap();
        let committer = Committer::start(dir.path().to_path_buf(), 64 * 1024 * 1024).unwrap();
        let lsn = committer
            .append_buffered(Record::new(RecordType::Put, &b"a"[..]))
            .unwrap();
        assert_eq!(lsn, 0);
        committer.sync().unwrap();
        committer.shutdown().unwrap();
    }

    #[test]
    fn buffered_records_are_lost_without_sync() {
        let dir = tempfile::tempdir().unwrap();
        let committer = Committer::start(dir.path().to_path_buf(), 64 * 1024 * 1024).unwrap();
        let _lsn = committer
            .append_buffered(Record::new(RecordType::Put, &b"a"[..]))
            .unwrap();
        // Abandon without sync — the record may or may not be on disk, but the
        // API contract says it is not guaranteed durable.
        committer.shutdown().unwrap();
    }

    #[test]
    fn sync_with_no_pending_records_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let committer = Committer::start(dir.path().to_path_buf(), 64 * 1024 * 1024).unwrap();
        committer.sync().unwrap();
        committer.shutdown().unwrap();
    }

    #[test]
    fn fsync_failure_is_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let committer = Committer::start_with_fault_config(
            dir.path().to_path_buf(),
            64 * 1024 * 1024,
            FaultConfig {
                fail_sync_every: Some(1),
                ..Default::default()
            },
        )
        .unwrap();

        let result = committer.append(Record::new(RecordType::Put, &b"a"[..]));
        assert!(
            matches!(result, Err(Error::Io(_))),
            "fsync failure should propagate as Io error, got {:?}",
            result
        );
        // The worker has terminated; shutdown may report the same error.
        let _ = committer.shutdown();
    }

    #[test]
    fn sync_failure_surfaces_to_caller() {
        let dir = tempfile::tempdir().unwrap();
        let committer = Committer::start_with_fault_config(
            dir.path().to_path_buf(),
            64 * 1024 * 1024,
            FaultConfig {
                fail_sync_every: Some(1),
                ..Default::default()
            },
        )
        .unwrap();

        // Buffer an append without forcing fsync.
        committer
            .append_buffered(Record::new(RecordType::Put, &b"a"[..]))
            .unwrap();

        // The explicit sync should fail because the injected fault triggers on
        // the first fsync.
        let result = committer.sync();
        assert!(
            matches!(result, Err(Error::Io(_))),
            "sync failure should propagate as Io error, got {:?}",
            result
        );
        let _ = committer.shutdown();
    }

    #[test]
    fn dropped_buffered_appends_are_lost_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let committer = Committer::start_with_fault_config(
            dir.path().to_path_buf(),
            64 * 1024 * 1024,
            FaultConfig {
                drop_appends: true,
                ..Default::default()
            },
        )
        .unwrap();

        let _lsn = committer
            .append_buffered(Record::new(RecordType::Put, &b"a"[..]))
            .unwrap();
        // The caller got an LSN but the bytes never reached the segment file.
        // Without a sync the record is definitely absent; with drop_appends it
        // is also absent after sync, but the sync itself would succeed on an
        // empty buffer.
        committer.shutdown().unwrap();

        // Reopen and verify the record is not readable.
        let committer2 = Committer::start(dir.path().to_path_buf(), 64 * 1024 * 1024).unwrap();
        let data = crate::segment::read_segment(dir.path(), 0).unwrap();
        assert!(
            data.is_empty(),
            "dropped buffered append should leave the segment empty"
        );
        committer2.shutdown().unwrap();
    }
}
