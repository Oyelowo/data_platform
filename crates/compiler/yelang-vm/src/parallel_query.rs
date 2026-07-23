//! Morsel-driven parallel query execution.
//!
//! This module connects the morsel machinery in [`crate::parallel`] to query
//! pipelines. A query's input is divided into morsels (~10K tuples each), and
//! worker threads pull morsels from a shared queue and push them through a
//! pipeline independently — the Umbra/HyPer model. Two combinators are
//! provided:
//!
//! - [`execute_query_parallel`] runs a *pipeline* over each morsel and
//!   concatenates the per-morsel outputs (map/filter/project style operators).
//! - [`execute_aggregate_parallel`] runs a *partial aggregate* over each
//!   morsel and folds the partials with a `merge` function (the PartialMerge
//!   pattern).
//!
//! [`execute_reg_vm_parallel`] wires the same morsel parallelism to the
//! register VM: each morsel is handed to a freshly built [`RegProgram`] and
//! executed on its own [`RegVm`], so a compiled pipeline can run across cores
//! without any shared mutable state.

use crate::parallel::ParallelExecutor;
use crate::reg_vm::{RegProgram, RegVm};
use crate::value::Value;

/// Execute a query pipeline in parallel over `tuples`.
///
/// The input is divided into morsels; each worker thread applies `pipeline_fn`
/// to the morsels it pulls from the shared queue, and all per-morsel outputs
/// are concatenated into the result. `num_workers` is clamped to at least one.
///
/// An empty input yields an empty result without spawning work.
pub fn execute_query_parallel<F>(tuples: Vec<Value>, pipeline_fn: F, num_workers: usize) -> Vec<Value>
where
    F: Fn(&[Value]) -> Vec<Value> + Send + Sync + 'static,
{
    let executor = ParallelExecutor::new(num_workers.max(1));
    executor.execute(tuples, pipeline_fn)
}

/// Execute a query pipeline in parallel with a custom morsel size.
///
/// Like [`execute_query_parallel`], but lets the caller control how many
/// tuples make up a single morsel (smaller morsels → finer load balancing,
/// larger morsels → less scheduling overhead).
pub fn execute_query_parallel_with_morsel_size<F>(
    tuples: Vec<Value>,
    pipeline_fn: F,
    num_workers: usize,
    morsel_size: usize,
) -> Vec<Value>
where
    F: Fn(&[Value]) -> Vec<Value> + Send + Sync + 'static,
{
    let executor = ParallelExecutor::with_morsel_size(num_workers.max(1), morsel_size.max(1));
    executor.execute(tuples, pipeline_fn)
}

/// Execute a parallel aggregate using the PartialMerge pattern.
///
/// Each worker computes a partial aggregate (`partial_agg`) over the morsels it
/// processes; the partials are then combined left-to-right with `merge`,
/// starting from `R::default()`. `num_workers` is clamped to at least one.
///
/// An empty input yields `R::default()`.
pub fn execute_aggregate_parallel<F, M, R>(
    tuples: Vec<Value>,
    partial_agg: F,
    merge: M,
    num_workers: usize,
) -> R
where
    F: Fn(&[Value]) -> R + Send + Sync + 'static,
    M: Fn(R, R) -> R + Send + Sync + 'static,
    R: Send + Default + Clone + 'static,
{
    let executor = ParallelExecutor::new(num_workers.max(1));
    executor.execute_aggregate(tuples, partial_agg, merge)
}

/// Execute a compiled register-VM pipeline in parallel over `tuples`.
///
/// The input is divided into morsels. For each morsel, `program_for` builds a
/// [`RegProgram`] whose entry function processes that morsel: the morsel's
/// tuples are passed to the entry function as a single array argument in
/// register `0`. The entry function's return value is flattened into the
/// output:
///
/// - `Value::Array` / `Value::QueryResult` → its elements/rows,
/// - any other value → a single-element vector,
/// - a VM error → an empty vector (the morsel contributes nothing).
///
/// Because each morsel gets its own [`RegVm`] and [`RegProgram`], there is no
/// shared mutable state between workers.
pub fn execute_reg_vm_parallel<F>(
    tuples: Vec<Value>,
    program_for: F,
    num_workers: usize,
) -> Vec<Value>
where
    F: Fn(&[Value]) -> RegProgram + Send + Sync + 'static,
{
    execute_query_parallel(
        tuples,
        move |morsel| {
            let program = program_for(morsel);
            let mut vm = RegVm::new();
            let input = Value::Array(morsel.to_vec());
            match vm.execute_with_args(&program, &[input]) {
                Ok(Value::Array(elems)) => elems,
                Ok(Value::QueryResult(rows)) => rows,
                Ok(other) => vec![other],
                Err(_) => Vec::new(),
            }
        },
        num_workers,
    )
}
