//! Cranelift JIT backend for the Yelang VM.
//!
//! The Yelang VM ([`yelang_vm`]) interprets bytecode. For hot query pipelines
//! we want to compile that bytecode to native machine code. This crate
//! implements the Umbra **"flying start"** model:
//!
//! 1. Code starts out being interpreted.
//! 2. [`profiling::Profiler`] counts executions per function.
//! 3. Once a function crosses the hotness threshold
//!    ([`profiling::Profiler::should_jit`]), it is compiled to native code by
//!    [`backend::JitBackend`] via Cranelift.
//! 4. Subsequent calls run the native version; cold paths keep interpreting.
//!
//! # Scope
//!
//! The JIT lowers the **integer / boolean numeric subset** of the bytecode
//! (arithmetic, comparisons, bitwise ops, locals, control flow, return) to
//! native `i64` code — the workload that dominates aggregate and arithmetic
//! query pipelines. Functions using richer values (floats, strings, arrays,
//! structs, calls, query operators) are detected by
//! [`compile::is_jittable`] and transparently fall back to the interpreter,
//! so execution always makes progress.
//!
//! # Example
//!
//! ```no_run
//! use yelang_jit::backend::JitBackend;
//! use yelang_vm::{CompiledFunction, Instruction, Value};
//!
//! // fn() -> i64 { 6 * 7 }
//! let func = CompiledFunction {
//!     name: None,
//!     instructions: vec![
//!         Instruction::PushConst(Value::Int(6)),
//!         Instruction::PushConst(Value::Int(7)),
//!         Instruction::Mul,
//!         Instruction::Return,
//!     ],
//!     num_locals: 0,
//!     num_args: 0,
//! };
//!
//! let mut jit = JitBackend::new().unwrap();
//! let result = jit.execute(&func).unwrap();
//! assert_eq!(result, Value::Int(42));
//! ```

pub mod backend;
pub mod compile;
pub mod profiling;

pub use backend::JitBackend;
pub use compile::{is_jittable, JitError};
pub use profiling::Profiler;
