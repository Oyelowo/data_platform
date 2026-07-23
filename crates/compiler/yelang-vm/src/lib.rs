//! Yelang bytecode virtual machine.
//!
//! Stack-based VM that executes compiled bytecode. Two execution paths:
//!
//! 1. **Regular code**: MIR → bytecode → VM execution
//! 2. **Query pipelines**: QIR → bytecode → VM execution (with query ops)
//!
//! The VM supports all Yelang value types, function calls, iteration,
//! and query operations (scan, filter, join, aggregate, sort, limit).
//!
//! For hot query pipelines, the produce/consume model (yelang-codegen)
//! can JIT-compile to machine code via Cranelift. The VM handles cold
//! paths and provides the "flying start" (Umbra model).

pub mod compile;
pub mod instruction;
pub mod join;
pub mod parallel;
pub mod parallel_query;
pub mod query_compile;
pub mod reg_instruction;
pub mod reg_vm;
pub mod storage;
pub mod traverse;
pub mod value;
pub mod vm;

pub use compile::compile_mir;
pub use instruction::{CompiledFunction, CompiledProgram, Instruction, WindowAgg, WindowFunc};
pub use join::{JoinAlgorithm, JoinKind, JoinSpec};
pub use parallel::{Morsel, MorselQueue, ParallelExecutor, DEFAULT_MORSEL_SIZE};
pub use parallel_query::{
    execute_aggregate_parallel, execute_query_parallel, execute_query_parallel_with_morsel_size,
    execute_reg_vm_parallel,
};
pub use query_compile::compile_query;
pub use reg_instruction::RegInstruction;
pub use reg_vm::{RegCallFrame, RegFunction, RegProgram, RegVm, RegVmError};
pub use storage::{DistributedSimStorage, EmptyStorage, InMemoryStorage, SimulatedTableStorage, StorageBackend};
pub use traverse::{TraverseDirection, TraverseSpec};
pub use value::Value;
pub use vm::{Vm, VmError};
