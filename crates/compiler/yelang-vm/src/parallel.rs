//! Morsel-driven parallelism for query pipeline execution.
//!
//! Based on the Umbra/HyPer model (Leis et al., VLDB 2014):
//! - Work is divided into morsels (~10K tuples)
//! - Worker threads grab morsels from a shared queue
//! - Each pipeline processes morsels independently
//! - Near-linear scaling with core count
//!
//! ```text
//! Query plan
//!   → decompose into pipelines (at pipeline breakers)
//!     → for each pipeline:
//!       → divide input into morsels (~10K tuples)
//!         → worker threads grab morsels from shared queue
//!           → each worker processes its morsel through the pipeline
//!             → results accumulated in shared state
//! ```

use std::sync::{Arc, Mutex};

use crate::value::Value;

/// Default morsel size (~10K tuples).
pub const DEFAULT_MORSEL_SIZE: usize = 10_000;

/// A morsel: a chunk of tuples to be processed by a worker thread.
#[derive(Debug, Clone)]
pub struct Morsel {
    /// The tuples in this morsel.
    pub tuples: Vec<Value>,
    /// The morsel index (for ordering).
    pub index: usize,
}

/// A shared morsel queue that worker threads pull from.
#[derive(Debug)]
pub struct MorselQueue {
    /// The morsels, protected by a mutex.
    morsels: Mutex<Vec<Morsel>>,
    /// The next morsel index to hand out.
    next: Mutex<usize>,
}

impl MorselQueue {
    /// Create a new morsel queue from a list of tuples.
    pub fn new(tuples: Vec<Value>, morsel_size: usize) -> Self {
        let morsels: Vec<Morsel> = tuples
            .chunks(morsel_size)
            .enumerate()
            .map(|(i, chunk)| Morsel {
                tuples: chunk.to_vec(),
                index: i,
            })
            .collect();
        Self {
            morsels: Mutex::new(morsels),
            next: Mutex::new(0),
        }
    }

    /// Try to get the next morsel. Returns None if all morsels are consumed.
    pub fn next_morsel(&self) -> Option<Morsel> {
        let mut next = self.next.lock().unwrap();
        let morsels = self.morsels.lock().unwrap();
        if *next < morsels.len() {
            let morsel = morsels[*next].clone();
            *next += 1;
            Some(morsel)
        } else {
            None
        }
    }

    /// Reset the queue for re-processing.
    pub fn reset(&self) {
        *self.next.lock().unwrap() = 0;
    }

    /// Total number of morsels.
    pub fn morsel_count(&self) -> usize {
        self.morsels.lock().unwrap().len()
    }

    /// Total number of tuples across all morsels.
    pub fn tuple_count(&self) -> usize {
        self.morsels.lock().unwrap().iter().map(|m| m.tuples.len()).sum()
    }
}

/// A parallel pipeline executor using morsel-driven parallelism.
///
/// Divides input into morsels and processes them in parallel using
/// worker threads. Each worker processes its morsel through the
/// pipeline function and accumulates results.
pub struct ParallelExecutor {
    /// Number of worker threads.
    pub num_workers: usize,
    /// Morsel size (tuples per morsel).
    pub morsel_size: usize,
}

impl ParallelExecutor {
    /// Create a new parallel executor with the given number of workers.
    pub fn new(num_workers: usize) -> Self {
        Self {
            num_workers,
            morsel_size: DEFAULT_MORSEL_SIZE,
        }
    }

    /// Create with a custom morsel size.
    pub fn with_morsel_size(num_workers: usize, morsel_size: usize) -> Self {
        Self {
            num_workers,
            morsel_size,
        }
    }

    /// Execute a pipeline function over tuples in parallel.
    ///
    /// The pipeline function takes a morsel of tuples and returns
    /// processed tuples. Results from all morsels are concatenated.
    pub fn execute<F>(&self, tuples: Vec<Value>, pipeline: F) -> Vec<Value>
    where
        F: Fn(&[Value]) -> Vec<Value> + Send + Sync + 'static,
    {
        if tuples.is_empty() {
            return vec![];
        }

        let queue = Arc::new(MorselQueue::new(tuples, self.morsel_size));
        let pipeline = Arc::new(pipeline);
        let results: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));

        // Spawn worker threads.
        let handles: Vec<_> = (0..self.num_workers)
            .map(|_| {
                let queue = Arc::clone(&queue);
                let pipeline = Arc::clone(&pipeline);
                let results = Arc::clone(&results);

                std::thread::spawn(move || {
                    while let Some(morsel) = queue.next_morsel() {
                        let morsel_result = pipeline(&morsel.tuples);
                        results.lock().unwrap().extend(morsel_result);
                    }
                })
            })
            .collect();

        // Wait for all workers to finish.
        for handle in handles {
            handle.join().expect("worker thread panicked");
        }

        // Collect results.
        let results = results.lock().unwrap();
        results.clone()
    }

    /// Execute a pipeline with accumulation (for aggregates).
    ///
    /// Each worker produces a partial aggregate. The final result is
    /// obtained by merging all partial aggregates.
    pub fn execute_aggregate<F, M, R>(
        &self,
        tuples: Vec<Value>,
        partial_agg: F,
        merge: M,
    ) -> R
    where
        F: Fn(&[Value]) -> R + Send + Sync + 'static,
        M: Fn(R, R) -> R + Send + Sync + 'static,
        R: Send + Default + Clone + 'static,
    {
        if tuples.is_empty() {
            return R::default();
        }

        let queue = Arc::new(MorselQueue::new(tuples, self.morsel_size));
        let partial_agg = Arc::new(partial_agg);
        let partials: Arc<Mutex<Vec<R>>> = Arc::new(Mutex::new(Vec::new()));

        let handles: Vec<_> = (0..self.num_workers)
            .map(|_| {
                let queue = Arc::clone(&queue);
                let partial_agg = Arc::clone(&partial_agg);
                let partials = Arc::clone(&partials);

                std::thread::spawn(move || {
                    while let Some(morsel) = queue.next_morsel() {
                        let partial = partial_agg(&morsel.tuples);
                        partials.lock().unwrap().push(partial);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("worker thread panicked");
        }

        // Merge all partial aggregates.
        let partials = partials.lock().unwrap();
        partials
            .iter()
            .fold(R::default(), |acc, p| merge(acc, p.clone()))
    }
}

impl Default for ParallelExecutor {
    fn default() -> Self {
        Self::new(num_cpus())
    }
}

/// Get the number of available CPU cores.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
