//! The HIR crate root.
//!
//! Items are stored out-of-band in maps keyed by `DefId` / `BodyId`,
/// matching rustc's design.

use yelang_util::FxHashMap;

use crate::ids::{BodyId, DefId};
use crate::hir::{ForeignItem, Impl, Item, Trait};

/// The root of the HIR for a single compilation unit.
#[derive(Debug, Clone)]
pub struct Crate {
    pub root_module: DefId,
    /// All items keyed by `DefId`.
    pub items: FxHashMap<DefId, Item>,
    /// All bodies keyed by `BodyId`.
    pub bodies: FxHashMap<BodyId, Body>,
    /// Trait definitions.
    pub traits: FxHashMap<DefId, Trait>,
    /// Impl blocks.
    pub impls: Vec<Impl>,
    /// Foreign items from `extern` blocks.
    pub foreign_items: FxHashMap<DefId, ForeignItem>,
}

impl Crate {
    pub fn new(root_module: DefId) -> Self {
        Self {
            root_module,
            items: FxHashMap::new(),
            bodies: FxHashMap::new(),
            traits: FxHashMap::new(),
            impls: Vec::new(),
            foreign_items: FxHashMap::new(),
        }
    }
}

// Re-export `Body` so that `crate_hir.rs` can reference it in the `Crate`
// struct above without needing an extra import everywhere.
pub use crate::hir_body::Body;
