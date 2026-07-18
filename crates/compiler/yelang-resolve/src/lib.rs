pub mod associated;
pub mod def_collector;
pub mod early;
pub mod error;
pub mod imports;
pub mod lang_items;
pub mod late;
pub mod module_tree;
pub mod namespaces;
pub mod path;
pub mod prelude;
pub mod privacy;
pub mod rib;
pub mod scope;

#[cfg(test)]
pub mod tests;

pub use associated::*;
pub use def_collector::*;
pub use early::*;
pub use error::*;
pub use imports::*;
pub use late::*;
pub use module_tree::*;
pub use namespaces::*;
pub use path::*;
pub use rib::*;
pub use scope::*;

use yelang_arena::{DefId, FxHashMap, IndexVec};
use yelang_interner::{Interner, Symbol};

/// Result of resolving a crate.
#[derive(Debug, Clone)]
pub struct ResolvedCrate {
    pub module_tree: ModuleTree,
    pub definitions: IndexVec<DefId, def_collector::Definition>,
    pub errors: Vec<ResolutionError>,
    /// Maps path spans to resolved DefIds for non-local paths.
    /// Populated during late resolution and consumed by HIR lowering.
    pub def_resolutions: FxHashMap<yelang_lexer::Span, DefId>,
    /// Maps enum `DefId` to a map of (variant name symbol -> variant `DefId`).
    /// Used by downstream passes (e.g., built-in derive lowering) that synthesize
    /// enum variant references without re-running name resolution.
    pub enum_variants: FxHashMap<DefId, FxHashMap<Symbol, DefId>>,
    /// The built-in prelude. Kept available so that downstream phases (e.g.
    /// built-in derive expansion) can look up prelude types and traits directly
    /// without relying on them being present in any module's namespace table.
    pub prelude: Option<crate::prelude::Prelude>,
    /// Registry of language items discovered during name resolution.
    pub lang_items: crate::lang_items::LangItems,
    /// Maps a generic parameter's source span to its `DefId`.
    pub generic_param_defs: FxHashMap<yelang_lexer::Span, DefId>,
    /// Maps a parent item's `DefId` to the ordered list of its generic param `DefId`s.
    pub generic_params: FxHashMap<DefId, Vec<DefId>>,
}

/// The main entry point for name resolution.
pub fn resolve_crate(ast: &yelang_ast::Program, interner: &Interner) -> ResolvedCrate {
    let collector = def_collector::DefCollector::new(interner).collect(ast);

    let mut resolver = scope::Resolver::new(
        interner,
        collector.module_tree,
        collector.definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    );
    resolver.errors = collector.errors;
    // Transfer impl indexes and generic param maps from collector to resolver
    resolver.inherent_impls = collector.inherent_impls;
    resolver.trait_impls = collector.trait_impls;
    resolver.impl_item_names = collector.impl_item_names;
    resolver.generic_param_defs = collector.generic_param_defs;
    resolver.generic_params = collector.generic_params;

    let early = early::EarlyResolver::new(&mut resolver);
    early.resolve(ast);

    let late = late::LateResolver::new(&mut resolver);
    late.resolve(ast);

    ResolvedCrate {
        module_tree: resolver.module_tree,
        definitions: resolver.definitions,
        errors: resolver.errors,
        def_resolutions: resolver.def_resolutions,
        enum_variants: resolver.enum_variants,
        prelude: resolver.prelude,
        lang_items: resolver.lang_items,
        generic_param_defs: resolver.generic_param_defs,
        generic_params: resolver.generic_params,
    }
}
