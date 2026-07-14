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

use yelang_interner::Interner;
use yelang_util::{DefId, FxHashMap};

use crate::def_collector::Definition;

/// Result of resolving a crate.
#[derive(Debug, Clone)]
pub struct ResolvedCrate {
    pub module_tree: ModuleTree,
    pub definitions: FxHashMap<DefId, Definition>,
    pub errors: Vec<ResolutionError>,
    /// Maps path spans to resolved DefIds for non-local paths.
    /// Populated during late resolution and consumed by HIR lowering.
    pub def_resolutions: FxHashMap<yelang_lexer::Span, DefId>,
}

/// The main entry point for name resolution.
pub fn resolve_crate(ast: &yelang_ast::Program, interner: &Interner) -> ResolvedCrate {
    let collector = def_collector::DefCollector::new(interner).collect(ast);

    // Merge prelude definitions into the main definitions map so that
    // downstream passes can look them up by DefId.
    let mut definitions = collector.definitions;
    if let Some(prelude) = &collector.prelude {
        for (def_id, def) in &prelude.definitions {
            definitions.insert(*def_id, def.clone());
        }
    }

    let mut resolver = scope::Resolver::new(
        interner,
        collector.module_tree,
        definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    );
    resolver.errors = collector.errors;
    // Transfer impl indexes from collector to resolver
    resolver.inherent_impls = collector.inherent_impls;
    resolver.trait_impls = collector.trait_impls;
    resolver.impl_item_names = collector.impl_item_names;

    let early = early::EarlyResolver::new(&mut resolver);
    early.resolve(ast);

    let late = late::LateResolver::new(&mut resolver);
    late.resolve(ast);

    ResolvedCrate {
        module_tree: resolver.module_tree,
        definitions: resolver.definitions,
        errors: resolver.errors,
        def_resolutions: resolver.def_resolutions,
    }
}
