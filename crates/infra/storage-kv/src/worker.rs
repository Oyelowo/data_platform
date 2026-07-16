//! Background worker for flush and compaction.

use std::sync::{Arc, Mutex};
use std::thread;

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
        // Process all currently queued immutable MemTables.
        loop {
            let (mem, path, options, version_set, manifest, last_sequence) = {
                let mut state = state.lock().unwrap();
                let mem = match state.immutable.pop() {
                    Some(m) => m,
                    None => break,
                };
                state.active_flushes += 1;
                (
                    mem,
                    state.path.clone(),
                    state.options,
                    Arc::clone(&state.version_set),
                    Arc::clone(&state.manifest),
                    state.last_sequence,
                )
            };

            let result = flush_memtable(
                &path,
                &options,
                &version_set,
                &manifest,
                &mem,
                last_sequence,
            );

            {
                let mut state = state.lock().unwrap();
                state.active_flushes -= 1;
                if let Err(e) = result {
                    // TODO: surface errors via an error channel / logging.
                    eprintln!("background flush failed: {}", e);
                    // Put the MemTable back so we don't lose it.
                    state.immutable.push(mem);
                    break;
                }
            }

            // Run compaction if the version set now needs it.
            if let Err(e) = crate::engine::LsmEngineInner::maybe_compact(&mut state.lock().unwrap()) {
                eprintln!("background compaction failed: {}", e);
            }
        }

        match receiver.recv() {
            Ok(WorkerCommand::Wake) => continue,
            Ok(WorkerCommand::Shutdown) | Err(_) => break,
        }
    }
}
