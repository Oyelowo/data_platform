use yelang_interner::Symbol;
use yelang_lexer::Span;
use yelang_util::DefId;

use crate::module_tree::ModuleTree;
use crate::namespaces::Namespace;
use crate::{ResolutionError, Resolver};

#[derive(Debug, Clone)]
pub struct UnresolvedImport {
    pub module_id: DefId,
    pub tree: yelang_ast::UseTree,
    pub span: Span,
}

pub fn resolve_imports(resolver: &mut Resolver) {
    let imports = resolver.unresolved_imports.clone();
    for import in imports {
        resolve_import_tree(resolver, import.module_id, &import.tree, import.span);
    }
}

fn resolve_import_tree(
    resolver: &mut Resolver,
    module_id: DefId,
    tree: &yelang_ast::UseTree,
    span: Span,
) {
    match tree {
        yelang_ast::UseTree::Simple { path, .. } => {
            if let Some((ns, def_id)) = resolve_import_path(resolver, module_id, path) {
                let name = path
                    .segments
                    .last()
                    .map(|s| s.ident.symbol)
                    .unwrap_or_else(|| resolver.interner.get_or_intern("<import>"));
                add_imported_item(resolver, module_id, ns, name, def_id, span);
            }
        }
        yelang_ast::UseTree::Rename { path, alias, .. } => {
            if let Some((ns, def_id)) = resolve_import_path(resolver, module_id, path) {
                add_imported_item(resolver, module_id, ns, alias.symbol, def_id, span);
            }
        }
        yelang_ast::UseTree::Glob { path, .. } => {
            resolve_glob_import(resolver, module_id, path, span);
        }
        yelang_ast::UseTree::Nested { prefix, items, .. } => {
            for item in items {
                resolve_import_tree(resolver, module_id, item, item.span());
            }
        }
    }
}

fn resolve_import_path(
    resolver: &Resolver,
    module_id: DefId,
    path: &yelang_ast::Path,
) -> Option<(Namespace, DefId)> {
    let mut current = module_id;
    let segments = &path.segments;
    if segments.is_empty() {
        return None;
    }

    let first = &segments[0];
    let first_str = first.ident.as_str(resolver.interner);

    if first_str == "crate" {
        current = resolver.module_tree.root.def_id;
    } else if first_str == "self" {
        current = module_id;
    } else if first_str == "super" {
        current = resolver
            .module_tree
            .modules
            .get(&module_id)
            .and_then(|m| m.parent)
            .unwrap_or(resolver.module_tree.root.def_id);
    } else {
        // Look up in current module's type namespace for a submodule.
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, first.ident.symbol)
            .or_else(|| resolver.resolve_name_in_module(current, Namespace::Value, first.ident.symbol));
        match found {
            Some(def_id) => current = def_id,
            None => return None,
        }
    }

    for seg in &segments[1..] {
        let seg_str = seg.ident.as_str(resolver.interner);
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, seg.ident.symbol)
            .or_else(|| resolver.resolve_name_in_module(current, Namespace::Value, seg.ident.symbol));
        match found {
            Some(def_id) => current = def_id,
            None => return None,
        }
    }

    let last = segments.last().unwrap();
    let last_str = last.ident.as_str(resolver.interner);

    if let Some(ns) = resolver.definitions.get(&current).and_then(|d| d.namespace()) {
        return Some((ns, current));
    }

    None
}

fn resolve_glob_import(
    resolver: &mut Resolver,
    module_id: DefId,
    path: &yelang_ast::Path,
    span: Span,
) {
    let mut current = module_id;
    let segments = &path.segments;
    if segments.is_empty() {
        return;
    }

    let first = &segments[0];
    let first_str = first.ident.as_str(resolver.interner);

    if first_str == "crate" {
        current = resolver.module_tree.root.def_id;
    } else if first_str == "self" {
        current = module_id;
    } else if first_str == "super" {
        current = resolver
            .module_tree
            .modules
            .get(&module_id)
            .and_then(|m| m.parent)
            .unwrap_or(resolver.module_tree.root.def_id);
    } else {
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, first.ident.symbol)
            .or_else(|| resolver.resolve_name_in_module(current, Namespace::Value, first.ident.symbol));
        match found {
            Some(def_id) => current = def_id,
            None => return,
        }
    }

    for seg in &segments[1..] {
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, seg.ident.symbol)
            .or_else(|| resolver.resolve_name_in_module(current, Namespace::Value, seg.ident.symbol));
        match found {
            Some(def_id) => current = def_id,
            None => return,
        }
    }

    // Find the module that `current` points to.
    let target_module = resolver
        .definitions
        .get(&current)
        .and_then(|d| {
            if matches!(d.kind, crate::def_collector::DefKind::Module) {
                Some(current)
            } else {
                // If current is a type, we can't glob-import its items for the MVP.
                None
            }
        });

    if let Some(target) = target_module {
        if let Some(node) = resolver.module_tree.modules.get(&target).cloned() {
            for (ns, map) in node.items.iter() {
                for (name, def_id) in map.iter() {
                    add_imported_item(resolver, module_id, *ns, *name, *def_id, span);
                }
            }
        }
    }
}

fn add_imported_item(
    resolver: &mut Resolver,
    module_id: DefId,
    ns: Namespace,
    name: Symbol,
    def_id: DefId,
    span: Span,
) {
    if let Some(module) = resolver.module_tree.modules.get_mut(&module_id) {
        if let Some(existing) = module.add_item(ns, name, def_id) {
            let existing_span = resolver
                .definitions
                .get(&existing)
                .map(|d| d.span)
                .unwrap_or_else(Span::default);
            resolver.errors.push(ResolutionError::DuplicateDefinition {
                name,
                span,
                original_span: existing_span,
            });
        }
    }
}
