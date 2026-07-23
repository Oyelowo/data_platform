//! Query optimization.
//!
//! Two phases:
//! 1. **Logical optimization** (`logical/`): Plan → Plan transformations
//!    that preserve query semantics (decorrelation, pushdown, simplify,
//!    join reordering, projection pruning).
//! 2. **Physical planning** (`../physical/`): Plan → PhysPlan transformations
//!    that choose execution algorithms and insert Exchange nodes.

pub mod logical;
mod optimizer;

pub use logical::decorrelate;
pub use optimizer::{default_rules, ApplyOrder, OptRule, Optimizer};
