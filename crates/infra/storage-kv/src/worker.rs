//! Background worker for flush.

use std::sync::{Arc, Mutex};
use std::thread;

use crate::compaction_worker::CompactionCommand;
use crate::engine::EngineState;
use crate::flush::flush_memtable;

/// Signal sent to the background worker.
#[derive(Debug, Clone, Copy)]
pub enum WorkerCommand {
    /// Wake up and process any pending immutable MemTables.
    Wake,
    /// Flush everything and stop the worker thread.
    Shutdown,
}

/// Handle to the background worker thread.
pub struct Worker {
    sender: crossbeam_channel::Sender<WorkerCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Worker {
    /// Spawn a background worker that drains the immutable MemTable queue.
    ///
    /// Returns the worker handle and a sender that can be used to wake it.
    pub fn spawn(
        state: Arc<Mutex<EngineState>>,
    ) -> (Self, crossbeam_channel::Sender<WorkerCommand>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let handle = thread::spawn(move || {
            worker_loop(state, receiver);
        });
        (
            Self {
                sender: sender.clone(),
                handle: Some(handle),
            },
            sender,
        )
    }

    /// Shut down the worker, flushing all pending MemTables first.
    pub fn shutdown(mut self) {
        let _ = self.sender.send(WorkerCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    state: Arc<Mutex<EngineState>>,
    receiver: crossbeam_channel::Receiver<WorkerCommand>,
) {
    loop {
        // Process all currently queued immutable MemTables across all column
        // families.  We always flush the globally oldest frozen MemTable first
        // (smallest reserved file number) so that per-CF FIFO order and global
        // file-number order are both preserved.
        loop {
            let work = {
                let mut state = state.lock().unwrap();
                let mut best: Option<(
                    crate::column_family::ColumnFamilyId,
                    crate::FileNumber,
                    Arc<crate::memtable::MemTable>,
                )> = None;
                for cf in state.column_families.iter() {
                    match cf.immutable.front() {
                        Some((num, mem))
                            if best.as_ref().is_none_or(|(_, best_num, _)| num < *best_num) =>
                        {
                            best = Some((cf.id, num, mem));
                        }
                        _ => {}
                    }
                }
                let (cf_id, file_number, mem) = match best {
                    Some(v) => v,
                    None => break,
                };
                // Clone engine-wide fields before borrowing the CF mutably.
                let path = state.path.clone();
                let manifest = Arc::clone(&state.manifest);
                let smallest_snapshot = state
                    .snapshots
                    .oldest()
                    .unwrap_or_else(|| state.seq_allocator.completed());
                let logger = state.options.logger();
                let cf = state.column_families.get_mut(cf_id).unwrap();
                cf.active_flushes += 1;
                FlushWork {
                    cf_id,
                    file_number,
                    mem,
                    path,
                    options: cf.options.clone(),
                    version_set: Arc::clone(&cf.version_set),
                    manifest,
                    smallest_snapshot,
                    metrics: Arc::clone(&cf.metrics),
                    logger,
                }
            };

            let result = flush_memtable(
                &work.path,
                &work.options,
                &work.version_set,
                &work.manifest,
                &work.mem,
                work.file_number,
                &work.metrics,
                work.cf_id,
                work.smallest_snapshot,
            );

            {
                let mut state = state.lock().unwrap();
                let cf = state.column_families.get_mut(work.cf_id).unwrap();
                cf.active_flushes -= 1;
                if let Err(e) = result {
                    work.logger.log(
                        crate::logger::LogLevel::Error,
                        &format!("background flush failed: {}", e),
                    );
                    // Wake stalled writers so they can re-evaluate the queue;
                    // the failed MemTable stays queued and will be retried on
                    // the next Wake.
                    state.immutable_room.notify_all();
                    break;
                }
                // Flush succeeded and the SSTable is now visible in the
                // VersionSet. Remove the MemTable from the immutable queue.
                let popped = cf.immutable.pop();
                debug_assert!(
                    popped
                        .map(|(n, p)| n == work.file_number && Arc::ptr_eq(&p, &work.mem))
                        .unwrap_or(false),
                    "immutable queue changed during flush"
                );
                // A slot freed up: wake writers stalled on a full queue.
                state.immutable_room.notify_all();
            }

            // A new L0 file may have triggered a compaction; wake the
            // independent compaction worker.
            if let Some(ref sender) = state.lock().unwrap().compaction_sender {
                let _ = sender.send(CompactionCommand::Wake);
            }
        }

        match receiver.recv() {
            Ok(WorkerCommand::Wake) => continue,
            Ok(WorkerCommand::Shutdown) | Err(_) => break,
        }
    }
}

struct FlushWork {
    cf_id: crate::column_family::ColumnFamilyId,
    file_number: crate::FileNumber,
    mem: Arc<crate::memtable::MemTable>,
    path: std::path::PathBuf,
    options: crate::options::LsmOptions,
    version_set: Arc<crate::version_set::VersionSet>,
    manifest: Arc<std::sync::Mutex<crate::manifest::Manifest>>,
    smallest_snapshot: crate::SequenceNumber,
    metrics: Arc<crate::metrics::Metrics>,
    logger: Arc<dyn crate::logger::Logger>,
}
