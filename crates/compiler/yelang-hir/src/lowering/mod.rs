//! AST -> HIR lowering.

pub mod body;
pub mod context;
pub mod err;
pub mod expr;
pub mod item;
pub mod pat;
pub mod ty;

pub use context::{LoweringContext, lower_crate};
