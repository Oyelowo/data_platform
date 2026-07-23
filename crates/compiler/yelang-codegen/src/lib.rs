//! Code generation for Yelang.
//!
//! Two code generation paths:
//!
//! 1. **MIR → machine code** (for non-query code):
//!    Standard lowering from MIR basic blocks to IR (LLVM/Cranelift/custom).
//!
//! 2. **QIR → machine code** (for query pipelines):
//!    Neumann's produce/consume model. Each physical operator emits code
//!    that pushes tuples through fused pipelines. Operator boundaries
//!    disappear in the generated code.
//!
//! ```text
//! Physical plan
//!   → produce/consume traversal
//!     → IR emission (LLVM / Cranelift / custom)
//!       → machine code
//!
//! Pipeline 1: Scan + Filter + build HashTable
//! Pipeline 2: Scan + Aggregate (pipeline breaker)
//! Pipeline 3: probe HashTable + output
//! ```

pub mod emit;
pub mod pipeline;
pub mod produce_consume;

pub use emit::{IrEmitter, IrValue};
pub use pipeline::{Pipeline, PipelineBreaker};
pub use produce_consume::ProduceConsume;
