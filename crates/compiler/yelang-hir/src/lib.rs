//! High-level Intermediate Representation (HIR) for Yelang.
//!
//! Lowered from AST after name resolution. All names are resolved to `DefId`s,
//! and syntax sugar (`for`, `while`, `?`, `async`, let-chains) is desugared.

pub mod crate_data;
pub mod derive;
pub mod hir;
pub mod ids;
pub mod lowering;
pub mod map;
pub mod res;
pub mod validate;
pub mod visit;

#[cfg(test)]
pub mod tests;

// Re-export the main public API.
pub use crate_data::Crate;
pub use hir::*;
pub use lowering::lower_crate;
pub use map::Map;
pub use res::ResolvedCrate;
