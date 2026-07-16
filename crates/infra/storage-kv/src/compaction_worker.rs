//! Independent background compaction worker.
//!
//! The compaction worker runs on its own thread and repeatedly drains compaction
//! jobs produced by the leveled picker.  It shares the engine-state lock with the
//! flush worker and foreground writers, but it only holds the lock while picking
//! a job and applying the resulting version edit; the expensive merge is run
//! outside the lock (this is implemented by having `LsmEngineInner::maybe_compact`
//! release the lock around the merge phase).

use std::sync::{Arc, Mutex};
use std::thread;

use crate::engine::EngineState;

/// Command sent to the compaction worker thread.
#[derive(Debug, Clone, Copy)]
pub enum CompactionCommand {
    /// Process any pending compaction jobs.
    Wake,
    /// Flush everything and stop the worker thread.
    Shutdown,
}

/// Handle to the background compaction worker.
pub struct CompactionWorker {
    sender: crossbeam_channel::Sender<CompactionCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

impl CompactionWorker {
    /// Spawn a background worker that drains compaction jobs.
    pub fn spawn(
        state: Arc<Mutex<EngineState>>,
    ) -> (Self, crossbeam_channel::Sender<CompactionCommand>) {
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

    /// Ask the worker to look for compaction work.
    #[allow(dead_code)]
    pub fn schedule(&self) {
        let _ = self.sender.send(CompactionCommand::Wake);
    }

    /// Shut down the worker, waiting for any in-progress compaction to finish.
    pub fn shutdown(mut self) {
        let _ = self.sender.send(CompactionCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    state: Arc<Mutex<EngineState>>,
    receiver: crossbeam_channel::Receiver<CompactionCommand>,
) {
    loop {
        // Mark the worker as busy while it is draining jobs.  This prevents
        // `LsmEngineInner::sync` from returning in the brief window between two
        // back-to-back compaction jobs in the same cascade.
        {
            let mut state = state.lock().unwrap();
            state.compaction_idle = false;
        }

        // Drain all currently scheduled compaction jobs.
        let did_work = match crate::engine::LsmEngineInner::maybe_compact(&state) {
            Ok(v) => v,
            Err(e) => {
                let logger = state.lock().unwrap().options.logger();
                logger.log(
                    crate::logger::LogLevel::Error,
                    &format!("background compaction failed: {}", e),
                );
                false
            }
        };
        if did_work {
            continue;
        }

        // No more work.  Publish idle before blocking on the channel so that
        // `sync` can observe a stable quiescent state.
        {
            let mut state = state.lock().unwrap();
            state.compaction_idle = true;
            state.compaction_idle_cond.notify_all();
        }

        match receiver.recv() {
            Ok(CompactionCommand::Wake) => continue,
            Ok(CompactionCommand::Shutdown) | Err(_) => break,
        }
    }
}
