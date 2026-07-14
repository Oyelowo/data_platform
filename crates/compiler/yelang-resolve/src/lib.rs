pub mod def_collector;
pub mod early;
pub mod error;
pub mod imports;
pub mod late;
pub mod module_tree;
pub mod namespaces;
pub mod path;
pub mod privacy;
pub mod rib;
pub mod scope;

#[cfg(test)]
pub mod tests;

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
use crate::module_tree::ModuleTree;

/// Result of resolving a crate.
#[derive(Debug, Clone)]
pub struct ResolvedCrate {
    pub module_tree: ModuleTree,
    pub definitions: FxHashMap<DefId, Definition>,
    pub errors: Vec<ResolutionError>,
}

/// The main entry point for name resolution.
pub fn resolve_crate(ast: &yelang_ast::Program, interner: &Interner) -> ResolvedCrate {
    let collector = def_collector::DefCollector::new(interner).collect(ast);
    let mut resolver = scope::Resolver::new(interner, collector.module_tree, collector.definitions);
    resolver.errors = collector.errors;

    let early = early::EarlyResolver::new(&mut resolver);
    early.resolve(ast);

    let late = late::LateResolver::new(&mut resolver);
    late.resolve(ast);

    ResolvedCrate {
        module_tree: resolver.module_tree,
        definitions: resolver.definitions,
        errors: resolver.errors,
    }
}
