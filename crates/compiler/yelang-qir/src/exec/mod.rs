//! Execution interface and in-memory interpreter for QIR physical plans.

pub mod batch;
pub mod exchange;
pub mod interface;
pub mod interpreter;
pub mod primitives;
pub mod memory;
pub mod memory_executor;
pub mod morsel;
pub mod operator;
pub mod operators;
pub mod pipeline;
pub mod plan;
pub mod spill;
pub mod value;

pub use interface::{QueryExecutor, Value};
pub use memory_executor::MemoryExecutor;
pub use plan::ExecPlan;
pub use value::{ArrowSchema, RecordBatch};
