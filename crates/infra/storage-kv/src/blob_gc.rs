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
        let (interval_ms, ratio, blob_store, snapshot) = {
            let state = state.lock().unwrap();
            let interval_ms = state.options.blob_gc_interval_ms;
            let ratio = state.options.blob_gc_ratio;
            let snapshot = state
                .snapshots
                .oldest()
                .unwrap_or_else(|| state.seq_allocator.completed());
            let blob_store = Arc::clone(&state.blob_store);
            (interval_ms, ratio, blob_store, snapshot)
        };

        // A disabled worker waits for an explicit wake or shutdown.
        if interval_ms == 0 {
            match receiver.recv() {
                Ok(BlobGcCommand::Wake) => continue,
                Ok(BlobGcCommand::Shutdown) | Err(_) => break,
            }
        }

        let mut owner = BlobGcOwner::new(Arc::clone(&state), Arc::clone(&blob_store));
        let options = GcOptions {
            min_live_ratio: ratio,
        };

        match blob_store.gc_once(&mut owner, &options, snapshot) {
            Ok(stats) => {
                if stats.scanned_files > 0 || stats.deleted_files > 0 {
                    let logger = state.lock().unwrap().options.logger();
                    logger.log(
                        crate::logger::LogLevel::Info,
                        &format!(
                            "blob GC pass completed: scanned={}, rewritten={} ({} bytes), deleted={} ({} bytes reclaimed)",
                            stats.scanned_files,
                            stats.rewritten_records,
                            stats.rewritten_bytes,
                            stats.deleted_files,
                            stats.space_reclaimed,
                        ),
                    );
                }
            }
            Err(e) => {
                let logger = state.lock().unwrap().options.logger();
                logger.log(
                    crate::logger::LogLevel::Error,
                    &format!("background blob GC failed: {}", e),
                );
            }
        }

        // Wait for the next scheduled pass or an explicit wake.
        match receiver.recv_timeout(Duration::from_millis(interval_ms)) {
            Ok(BlobGcCommand::Wake) => continue,
            Ok(BlobGcCommand::Shutdown) | Err(_) => break,
        }
    }
}
