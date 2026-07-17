//! The HIR crate root.
//!
//! Items are stored out-of-band in dense `IndexVec`s keyed by `DefId` /
//! `BodyId`, matching the allocation discipline used by name resolution.
use yelang_arena::IndexVec;

use crate::hir::{ForeignItem, Impl, Trait};
use crate::ids::{BodyId, DefId};

/// The root of the HIR for a single compilation unit.
#[derive(Debug, Clone)]
pub struct Crate {
    pub root_module: DefId,
    /// All items keyed by `DefId`.
    pub items: IndexVec<DefId, Option<Item>>,
    /// All bodies keyed by `BodyId`.
    pub bodies: IndexVec<BodyId, Body>,
    /// Trait definitions keyed by the trait's `DefId`.
    pub traits: IndexVec<DefId, Option<Trait>>,
    /// Impl blocks.
    pub impls: Vec<Impl>,
    /// Foreign items from `extern` blocks keyed by their `DefId`.
    pub foreign_items: IndexVec<DefId, Option<ForeignItem>>,
}

impl Crate {
    pub fn new(root_module: DefId) -> Self {
        Self {
            root_module,
            items: IndexVec::new(),
            bodies: IndexVec::new(),
            traits: IndexVec::new(),
            impls: Vec::new(),
            foreign_items: IndexVec::new(),
        }
    }
}

// Re-export `Body` and `Item` so that `crate_hir.rs` can reference them in the
// `Crate` struct above without needing an extra import everywhere.
pub use crate::hir_body::Body;
pub use crate::hir_item::Item;
