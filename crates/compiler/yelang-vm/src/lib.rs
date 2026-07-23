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
pub mod parallel;
pub mod query_compile;
pub mod reg_instruction;
pub mod storage;
pub mod traverse;
pub mod value;
pub mod vm;

pub use compile::compile_mir;
pub use instruction::{CompiledFunction, CompiledProgram, Instruction, WindowAgg, WindowFunc};
pub use parallel::{Morsel, MorselQueue, ParallelExecutor, DEFAULT_MORSEL_SIZE};
pub use query_compile::compile_query;
pub use reg_instruction::RegInstruction;
pub use storage::{EmptyStorage, InMemoryStorage, StorageBackend};
pub use traverse::{TraverseDirection, TraverseSpec};
pub use value::Value;
pub use vm::{Vm, VmError};
