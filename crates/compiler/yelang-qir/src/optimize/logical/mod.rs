//! Logical optimization passes.
//!
//! These passes transform the logical plan (Plan → Plan) without
//! changing the query semantics. They run before physical planning.

pub mod decorrelate;
pub mod join_reorder;
pub mod prune;
pub mod pushdown;
pub mod simplify;

pub use decorrelate::decorrelate;
pub use join_reorder::JoinReorder;
pub use prune::PruneUnusedFields;
pub use pushdown::PushDownFilter;
pub use simplify::{EliminateTrivialFilter, EliminateTrivialLimit, MergeAdjacentFilters};
