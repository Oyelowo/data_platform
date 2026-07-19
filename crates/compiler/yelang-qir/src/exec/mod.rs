//! Execution interface and in-memory interpreter for QIR physical plans.

pub mod exchange;
pub mod interface;
pub mod kernels;
pub mod memory;
pub mod morsel;
pub mod operator;
pub mod pipeline;
pub mod plan;
pub mod spill;
pub mod value;

pub use interface::{QueryExecutor, Value};
pub use memory::MemoryExecutor;
pub use plan::ExecPlan;
pub use value::{ArrowSchema, RecordBatch};
