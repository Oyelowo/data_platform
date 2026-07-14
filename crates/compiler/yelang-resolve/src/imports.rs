use yelang_lexer::Span;
use yelang_util::DefId;

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
            } else {
                let name = path
                    .segments
                    .last()
                    .map(|s| s.ident.symbol)
                    .unwrap_or_else(|| resolver.interner.get_or_intern("<import>"));
                resolver.errors.push(ResolutionError::NotFound { name, span });
            }
        }
        yelang_ast::UseTree::Rename { path, alias, .. } => {
            if let Some((ns, def_id)) = resolve_import_path(resolver, module_id, path) {
                add_imported_item(resolver, module_id, ns, alias.symbol, def_id, span);
            } else {
                let name = path
                    .segments
                    .last()
                    .map(|s| s.ident.symbol)
                    .unwrap_or_else(|| resolver.interner.get_or_intern("<import>"));
                resolver.errors.push(ResolutionError::NotFound { name, span });
            }
        }
        yelang_ast::UseTree::Glob { path, .. } => {
            resolve_glob_import(resolver, module_id, path, span);
        }
        yelang_ast::UseTree::Nested { prefix, items, .. } => {
            for item in items {
                let item_with_prefix = prepend_prefix_to_use_tree(prefix, item.clone());
                resolve_import_tree(resolver, module_id, &item_with_prefix, item.span());
            }
        }
    }
}

/// Prepend a path prefix to every path inside a UseTree.
fn prepend_prefix_to_use_tree(
    prefix: &yelang_ast::Path,
    tree: yelang_ast::UseTree,
) -> yelang_ast::UseTree {
    match tree {
        yelang_ast::UseTree::Simple { path, span } => {
            let mut new_path = prefix.clone();
            new_path.segments.extend(path.segments);
            yelang_ast::UseTree::Simple { path: new_path, span }
        }
        yelang_ast::UseTree::Rename { path, alias, span } => {
            let mut new_path = prefix.clone();
            new_path.segments.extend(path.segments);
            yelang_ast::UseTree::Rename { path: new_path, alias, span }
        }
        yelang_ast::UseTree::Glob { path, span } => {
            let mut new_path = prefix.clone();
            new_path.segments.extend(path.segments);
            yelang_ast::UseTree::Glob { path: new_path, span }
        }
        yelang_ast::UseTree::Nested { prefix: inner_prefix, items, span } => {
            let mut new_prefix = prefix.clone();
            new_prefix.segments.extend(inner_prefix.segments);
            yelang_ast::UseTree::Nested {
                prefix: new_prefix,
                items,
                span,
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
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, first.ident.symbol)
            .or_else(|| resolver.resolve_name_in_module(current, Namespace::Value, first.ident.symbol));
        match found {
            Some(def_id) => current = def_id,
            None => return None,
        }
    }

    for seg in &segments[1..] {
        let found = resolver
            .resolve_name_in_module(current, Namespace::Type, seg.ident.symbol)
            .or_else(|| resolver.resolve_name_in_module(current, Namespace::Value, seg.ident.symbol));
        match found {
            Some(def_id) => current = def_id,
            None => return None,
        }
    }

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

    let target_module = resolver
        .definitions
        .get(&current)
        .and_then(|d| {
            if matches!(d.kind, crate::def_collector::DefKind::Module) {
                Some(current)
            } else {
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
    name: yelang_interner::Symbol,
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
