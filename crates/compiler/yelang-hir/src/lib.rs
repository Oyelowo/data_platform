//! High-level Intermediate Representation (HIR) for Yelang.
//!
//! Lowered from AST after name resolution. All names are resolved to `DefId`s,
//! and syntax sugar (`for`, `while`, `?`, `async`, let-chains) is desugared.

pub mod crate_hir;
pub mod derive;
pub mod hir;
pub mod hir_body;
pub mod hir_expr;
pub mod hir_item;
pub mod hir_pat;
pub mod hir_struct;
pub mod hir_ty;
pub mod ids;
pub mod lowering;
pub mod lowering_err;
pub mod map;
pub mod res;
pub mod visitor;

pub mod lowering_body;
pub mod lowering_expr;
pub mod lowering_item;
pub mod lowering_pat;
pub mod lowering_ty;

#[cfg(test)]
pub mod tests;

// Re-export the main public API.
pub use crate_hir::Crate;
pub use lowering::lower_crate;
pub use map::Map;
pub use res::ResolvedCrate;
