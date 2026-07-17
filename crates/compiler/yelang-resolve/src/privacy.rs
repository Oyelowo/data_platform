use yelang_arena::DefId;
use yelang_ast::Visibility;
use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::{
    def_collector::{DefKind, Definition},
    module_tree::ModuleTree,
    scope::Resolver,
};

/// Check if `def_id` is accessible from `use_module`.
pub fn check_accessibility(
    resolver: &Resolver,
    def_id: DefId,
    use_module: DefId,
    name: Symbol,
    span: Span,
) -> bool {
    let Some(def) = resolver.definitions.get(&def_id) else {
        return true;
    };
    let def_module = def.parent.unwrap_or(resolver.module_tree.root.def_id);

    // Check if the module containing the definition is accessible
    if !is_module_accessible(&resolver.module_tree, def_module, use_module) {
        return false;
    }

    // Check the item's specific visibility
    is_item_visible(resolver, def, def_module, use_module)
}

/// Check if module `target` is accessible from module `from`.
/// A module is accessible from itself, its descendants, and its ancestors.
/// For non-ancestor/descendant access, the module must be `pub` and its parent accessible.
fn is_module_accessible(module_tree: &ModuleTree, target: DefId, from: DefId) -> bool {
    if target == from {
        return true;
    }

    // Check if `from` is a descendant of `target`
    if is_ancestor_of(module_tree, target, from) {
        return true;
    }

    // Check if `from` is the direct parent (defining module) of `target`.
    // In Rust, a parent module can access its private child modules.
    if let Some(node) = module_tree.modules.get(&target) {
        if let Some(parent) = node.parent {
            if parent == from {
                return true;
            }
            // For non-parent, non-descendant access, the target must be pub
            // and its parent must be accessible from `from`.
            if matches!(node.visibility, Visibility::Public(_)) {
                return is_module_accessible(module_tree, parent, from);
            }
        } else {
            // Root module is always accessible
            return true;
        }
    }

    false
}

/// Check if `ancestor` is an ancestor of `descendant` in the module tree.
fn is_ancestor_of(module_tree: &ModuleTree, ancestor: DefId, descendant: DefId) -> bool {
    let mut current = descendant;
    while let Some(node) = module_tree.modules.get(&current) {
        if let Some(parent) = node.parent {
            if parent == ancestor {
                return true;
            }
            current = parent;
        } else {
            break;
        }
    }
    false
}

/// Check if an item is visible from a given module.
fn is_item_visible(
    resolver: &Resolver,
    def: &Definition,
    def_module: DefId,
    use_module: DefId,
) -> bool {
    match &def.visibility {
        Visibility::Private | Visibility::PublicSelf(_) => {
            // Same module or descendant
            def_module == use_module || is_descendant_of(resolver, use_module, def_module)
        }
        Visibility::Public(_) => {
            // Public if module chain allows (already checked by is_module_accessible)
            true
        }
        Visibility::PublicCrate(_) => {
            // Same crate - for now always true since we only support single crate
            true
        }
        Visibility::PublicSuper(_) => {
            // Parent module or descendant of parent
            if let Some(def_parent) = resolver
                .module_tree
                .modules
                .get(&def_module)
                .and_then(|n| n.parent)
            {
                def_parent == use_module || is_descendant_of(resolver, use_module, def_parent)
            } else {
                // Root module has no parent, so pub(super) is same as private
                def_module == use_module || is_descendant_of(resolver, use_module, def_module)
            }
        }
        Visibility::PublicIn { path, .. } => {
            // Resolve the path relative to the defining module, then check if use_module is inside it
            if let Some(target_module) = resolve_visibility_path(resolver, path, def_module) {
                target_module == use_module || is_descendant_of(resolver, use_module, target_module)
            } else {
                false
            }
        }
    }
}

/// Check if `module` is a descendant of `ancestor`.
fn is_descendant_of(resolver: &Resolver, module: DefId, ancestor: DefId) -> bool {
    let mut current = module;
    while let Some(node) = resolver.module_tree.modules.get(&current) {
        if let Some(parent) = node.parent {
            if parent == ancestor {
                return true;
            }
            current = parent;
        } else {
            break;
        }
    }
    false
}

/// Resolve a path in `pub(in path)` to a module DefId.
/// `def_module` is the module where the item with this visibility is defined.
fn resolve_visibility_path(
    resolver: &Resolver,
    path: &yelang_ast::Path,
    def_module: DefId,
) -> Option<DefId> {
    use crate::namespaces::Namespace;

    if path.segments.is_empty() {
        return None;
    }

    let first = &path.segments[0];
    let first_str = first.ident.as_str(resolver.interner);
    let first_span = first.ident.span();

    let mut current = resolver.module_tree.root.def_id;

    if first_str == "crate" {
        current = resolver.module_tree.root.def_id;
    } else if first_str == "self" {
        current = def_module;
    } else if first_str == "super" {
        current = resolver
            .module_tree
            .modules
            .get(&def_module)
            .and_then(|n| n.parent)
            .unwrap_or(resolver.module_tree.root.def_id);
    } else {
        // Start from the defining module
        current = def_module;
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, first.ident.symbol, first_span)
            .or_else(|| {
                resolver.resolve_name_in_module(
                    current,
                    Namespace::Value,
                    first.ident.symbol,
                    first_span,
                )
            });
        match found {
            Some(id) => current = id,
            None => return None,
        }
    }

    for seg in &path.segments[1..] {
        let seg_span = seg.ident.span();
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, seg.ident.symbol, seg_span)
            .or_else(|| {
                resolver.resolve_name_in_module(
                    current,
                    Namespace::Value,
                    seg.ident.symbol,
                    seg_span,
                )
            });
        match found {
            Some(id) => current = id,
            None => return None,
        }
    }

    // Verify it's a module
    if let Some(def) = resolver.definitions.get(&current) {
        if matches!(def.kind, DefKind::Module) {
            return Some(current);
        }
    }

    None
}
