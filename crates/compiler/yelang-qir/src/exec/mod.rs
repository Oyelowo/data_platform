//! Execution interface and in-memory interpreter for QIR physical plans.

pub mod interface;
pub mod kernels;
pub mod memory;

pub use interface::{QueryExecutor, Value};
pub use memory::MemoryExecutor;
