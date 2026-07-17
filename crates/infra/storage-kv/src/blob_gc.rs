//! Independent background blob garbage-collection worker.
//!
//! The blob GC worker periodically scans non-current blob files and rewrites
//! live records into the current blob file.  This keeps the value log from
//! growing forever as large values are overwritten or deleted.
//!
//! The worker shares the engine-state lock only to read configuration and to
//! compute the safe snapshot for liveness checks.  The expensive scan/rewrite
//! work is done outside the lock by [`crate::blob::BlobStore::gc_once`].

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::blob::GcOptions;
use crate::engine::{BlobGcOwner, EngineState};

/// Command sent to the blob GC worker thread.
#[derive(Debug, Clone, Copy)]
pub enum BlobGcCommand {
    /// Look for blob files that need garbage collection.
    Wake,
    /// Flush everything and stop the worker thread.
    Shutdown,
}

/// Handle to the background blob GC worker.
pub struct BlobGcWorker {
    sender: Sender<BlobGcCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

impl BlobGcWorker {
    /// Spawn a background worker that runs blob GC passes.
    pub fn spawn(state: Arc<Mutex<EngineState>>) -> (Self, Sender<BlobGcCommand>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let handle = thread::spawn(move || worker_loop(state, receiver));
        (
            Self {
                sender: sender.clone(),
                handle: Some(handle),
            },
            sender,
        )
    }

    /// Ask the worker to look for blob GC work.
    #[allow(dead_code)]
    pub fn schedule(&self) {
        let _ = self.sender.send(BlobGcCommand::Wake);
    }

    /// Shut down the worker, waiting for any in-progress GC pass to finish.
    pub fn shutdown(mut self) {
        let _ = self.sender.send(BlobGcCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(state: Arc<Mutex<EngineState>>, receiver: Receiver<BlobGcCommand>) {
    loop {
        let (interval_ms, force_threshold, blob_store) = {
            let state = state.lock().unwrap();
            let interval_ms = state.options.blob_gc_interval_ms;
            let force_threshold = state.options.blob_gc_force_threshold;
            let blob_store = Arc::clone(&state.blob_store);
            (interval_ms, force_threshold, blob_store)
        };

        // A disabled worker waits for an explicit wake or shutdown.
        if interval_ms == 0 {
            match receiver.recv() {
                Ok(BlobGcCommand::Wake) => continue,
                Ok(BlobGcCommand::Shutdown) | Err(_) => break,
            }
        }

        // Sleep until the next scheduled pass, but wake early if an explicit
        // wake arrives or if the force threshold is exceeded.  Polling in small
        // chunks lets us react to garbage-ratio spikes without putting the
        // write path to sleep.
        let reason = wait_for_next_pass(&receiver, interval_ms, &blob_store, force_threshold);

        match reason {
            WakeReason::Shutdown => break,
            WakeReason::Explicit | WakeReason::Interval | WakeReason::ForceThreshold => {}
        }

        // Read the GC options and live snapshots *after* waking so the liveness
        // check includes all writes that contributed to the blob garbage ratio.
        let (ratio, threads, snapshots) = {
            let state = state.lock().unwrap();
            let mut snapshots = state.snapshots.all();
            // The current completed watermark is also a valid snapshot: it must
            // see all writes that have been published, so current values must be
            // preserved even if no explicit snapshot is registered.
            snapshots.push(state.seq_allocator.completed());
            snapshots.sort_unstable();
            snapshots.dedup();
            (
                state.options.blob_gc_ratio,
                state.options.blob_gc_threads,
                snapshots,
            )
        };

        let mut owner = BlobGcOwner::new(Arc::clone(&state), Arc::clone(&blob_store));
        let options = GcOptions {
            min_live_ratio: ratio,
            threads,
        };

        // Run at least one pass whenever we wake.  If the configured force
        // threshold is exceeded and the last pass made progress, keep running
        // back-to-back until the ratio drops or no more work can be done.
        // This prevents runaway loops when garbage is pinned by snapshots or
        // lives in the current file.
        let mut first = true;
        while first || should_force_pass(&blob_store, force_threshold) {
            first = false;
            let did_work = run_gc_pass(&state, &blob_store, &mut owner, &options, &snapshots);
            if !did_work {
                break;
            }
        }

        // If we were explicitly woken, go straight back to sleep without
        // waiting for the full interval again.
        if matches!(reason, WakeReason::Explicit) {
            continue;
        }
    }
}

fn should_force_pass(blob_store: &crate::blob::BlobStore, force_threshold: f64) -> bool {
    force_threshold > 0.0 && blob_store.force_gc_needed(force_threshold)
}

/// Reason the worker decided to run a scheduled GC pass.
enum WakeReason {
    /// The regular interval fired.
    Interval,
    /// An explicit `Wake` command was received.
    Explicit,
    /// The force-GC threshold was exceeded.
    ForceThreshold,
    /// The worker was asked to shut down.
    Shutdown,
}

/// Block until the next scheduled GC pass should run.
fn wait_for_next_pass(
    receiver: &Receiver<BlobGcCommand>,
    interval_ms: u64,
    blob_store: &crate::blob::BlobStore,
    force_threshold: f64,
) -> WakeReason {
    let target = Duration::from_millis(interval_ms);
    let poll = Duration::from_millis(100);
    let mut elapsed = Duration::ZERO;

    while elapsed < target {
        // The loop body advances `elapsed` at the bottom.
        if force_threshold > 0.0 && blob_store.force_gc_needed(force_threshold) {
            return WakeReason::ForceThreshold;
        }
        let wait = poll.min(target - elapsed);
        match receiver.recv_timeout(wait) {
            Ok(BlobGcCommand::Wake) => return WakeReason::Explicit,
            Ok(BlobGcCommand::Shutdown)
            | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return WakeReason::Shutdown;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                elapsed += wait;
            }
        }
    }
    WakeReason::Interval
}

/// Run one GC pass and return true if it did any work.
fn run_gc_pass(
    state: &Arc<Mutex<EngineState>>,
    blob_store: &crate::blob::BlobStore,
    owner: &mut crate::engine::BlobGcOwner,
    options: &GcOptions,
    snapshots: &[crate::SequenceNumber],
) -> bool {
    match blob_store.gc_once(owner, options, snapshots) {
        Ok(stats) => {
            let did_work =
                stats.scanned_files > 0 || stats.deleted_files > 0 || stats.rewritten_records > 0;
            if did_work {
                let logger = state.lock().unwrap().options.logger();
                logger.log(
                    crate::logger::LogLevel::Info,
                    &format!(
                        "blob GC pass completed: scanned={}, rewritten={} ({} bytes), dead={} ({} bytes), deleted={} ({} bytes reclaimed)",
                        stats.scanned_files,
                        stats.rewritten_records,
                        stats.rewritten_bytes,
                        stats.dead_records,
                        stats.dead_bytes,
                        stats.deleted_files,
                        stats.space_reclaimed,
                    ),
                );
            }
            did_work
        }
        Err(e) => {
            let logger = state.lock().unwrap().options.logger();
            logger.log(
                crate::logger::LogLevel::Error,
                &format!("background blob GC failed: {}", e),
            );
            false
        }
    }
}
