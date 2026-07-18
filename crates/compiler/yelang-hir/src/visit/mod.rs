//! HIR traversal and transformation utilities.
//!
//! This module provides three complementary ways to walk the HIR:
//!
//! - [`Visitor`](visitor::Visitor): read-only traversal.
//! - [`MutVisitor`](mut_visitor::MutVisitor): in-place mutation of HIR nodes.
//! - [`Folder`](folder::Folder): functional HIR → HIR rewrite that allocates
//!   new nodes in the arena and returns new IDs.

pub mod folder;
pub mod mut_visitor;
pub mod visitor;

pub use folder::{Folder, fold_crate};
pub use mut_visitor::{MutVisitor, walk_crate_mut};
pub use visitor::{Visitor, walk_crate};
