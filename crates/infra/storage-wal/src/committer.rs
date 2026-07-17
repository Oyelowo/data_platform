//! Group commit / fsync worker.
//!
//! Multiple concurrent threads submit commit requests over a bounded channel.
//! A dedicated background thread drains the channel, appends all pending
//! records to the current segment, performs a single `fsync`, and then
//! acknowledges every waiter. This amortises the cost of `fsync` across many
//! concurrent writers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};

use crate::record::{RECORD_HEADER_SIZE, Record};
use crate::segment::Segment;
use crate::{Error, Lsn, Result};

/// Maximum number of outstanding commit requests.
const CHANNEL_CAPACITY: usize = 4096;

/// Maximum time to wait for additional committers before flushing (1 ms).
const COMMIT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1);

/// Request sent to the commit worker.
struct CommitRequest {
    record: Record,
    /// One-shot reply channel carrying the assigned LSN once durable.
    reply: Sender<Result<Lsn>>,
}

/// Handle to the group commit worker.
pub struct Committer {
    sender: Mutex<Option<Sender<CommitRequest>>>,
    shutdown: Arc<AtomicBool>,
    handle: Mutex<Option<std::thread::JoinHandle<Result<()>>>>,
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
        let state = init_worker(dir, segment_size)?;
        let (sender, receiver) = bounded(CHANNEL_CAPACITY);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let handle = std::thread::spawn(move || {
            worker_loop_with_state(state, receiver, shutdown_clone)
        });

        Ok(Self {
            sender: Mutex::new(Some(sender)),
            shutdown,
            handle: Mutex::new(Some(handle)),
        })
    }

    /// Submit a record for durable append. Blocks until the worker has
    /// acknowledged the request (or the WAL is closed).
    pub fn append(&self, record: Record) -> Result<Lsn> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        let (tx, rx) = bounded(1);
        let sender = self.sender.lock().unwrap();
        sender
            .as_ref()
            .ok_or(Error::Closed)?
            .send(CommitRequest { record, reply: tx })
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
        // Join the worker so the directory is left in a quiesced state and we do
        // not leak a detached thread. `shutdown` is idempotent.
        let _ = self.shutdown();
    }
}

struct WorkerState {
    dir: std::path::PathBuf,
    segment_size: u64,
    next_lsn: Lsn,
    segment: Segment,
}

fn worker_loop_with_state(
    mut state: WorkerState,
    receiver: Receiver<CommitRequest>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    // Pending requests whose records have been written but not yet fsynced.
    let mut pending: Vec<(Sender<Result<Lsn>>, Lsn)> = Vec::new();

    loop {
        // Wait for the first request of the next group, or timeout and flush.
        let req = match receiver.recv_timeout(COMMIT_TIMEOUT) {
            Ok(req) => req,
            Err(RecvTimeoutError::Timeout) => {
                flush_pending(&mut state, &mut pending)?;
                if shutdown.load(Ordering::Acquire) && pending.is_empty() {
                    break;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush_pending(&mut state, &mut pending)?;
                break;
            }
        };

        let (lsn, _len) = write_record(&mut state, req.record)?;
        pending.push((req.reply, lsn));

        // Drain additional requests without blocking to build the group.
        while let Ok(req) = receiver.try_recv() {
            let (lsn, _len) = write_record(&mut state, req.record)?;
            pending.push((req.reply, lsn));
            if pending.len() >= CHANNEL_CAPACITY / 2 {
                break;
            }
        }

        flush_pending(&mut state, &mut pending)?;

        if shutdown.load(Ordering::Acquire) && receiver.is_empty() {
            break;
        }
    }

    state.segment.sync()?;
    Ok(())
}

fn init_worker(dir: std::path::PathBuf, segment_size: u64) -> Result<WorkerState> {
    std::fs::create_dir_all(&dir)?;
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
        state.segment.sync()?;
        state.next_lsn += state.segment.written();
        state.segment = Segment::open(&state.dir, state.next_lsn, state.segment_size)?;
        record.lsn = state.next_lsn;
        buf.clear();
        record.encode(&mut buf)?;
        if buf.len() as u64 > state.segment.remaining() {
            return Err(Error::InvalidArgument(
                "record is larger than the configured segment size".into(),
            ));
        }
    }

    state.segment.append(&buf)?;
    let lsn = state.next_lsn;
    state.next_lsn += buf.len() as u64;
    Ok((lsn, buf.len()))
}

fn flush_pending(
    state: &mut WorkerState,
    pending: &mut Vec<(Sender<Result<Lsn>>, Lsn)>,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    let flush_result: Result<()> = state.segment.flush().and_then(|()| state.segment.sync());

    if let Err(e) = flush_result {
        for (reply, _) in pending.drain(..) {
            let _ = reply.send(Err(Error::Closed));
        }
        return Err(e);
    }

    for (reply, lsn) in pending.drain(..) {
        // Best-effort reply; if the caller hung up, drop the result.
        let _ = reply.send(Ok(lsn));
    }

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
}
