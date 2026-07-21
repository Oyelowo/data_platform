//! THIR → LIR extraction.
//!
//! This module lowers typed THIR expressions into QIR logical plans using the
//! lang-item `Queryable`/`Aggregate` traits and the `@intrinsic` hooks defined
//! in `stdlib/core/src/query.ye`. It replaces the legacy HIR-based lowering in
//! `crate::lir::lower::queryable`.

pub mod aggregate;
pub mod context;
pub mod convert;
pub mod extract;
pub mod inline;
pub mod intrinsic;
pub mod query_syntax;

pub use context::ExtractCtxt;
pub use extract::lower_thir_body;
