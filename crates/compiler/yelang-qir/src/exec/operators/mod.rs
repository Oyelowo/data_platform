//! Physical operators of the push-based vectorized engine.
//! Phase J7. Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §7, §9.

pub mod distinct;
pub mod exchange;
pub mod filter;
pub mod group_join;
pub mod hash_aggregate;
pub mod hash_join;
pub mod hash_table;
pub mod limit;
pub mod merge_join;
pub mod nested_loop;
pub mod project;
pub mod scan;
pub mod sort;
pub mod traversal;
pub mod window;
