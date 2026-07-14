use yelang_ast::{Ident, Path, PathSegment};
use yelang_interner::Symbol;
use yelang_util::DefId;

use crate::namespaces::Namespace;
use crate::rib::Resolution;
use crate::scope::Resolver;

/// Resolve a path in the given namespace, starting from the current module.
pub fn resolve_path(
    resolver: &Resolver,
    path: &Path,
    ns: Namespace,
) -> Option<Resolution> {
    if path.segments.is_empty() {
        return None;
    }

    let first = &path.segments[0];
    let first_str = first.ident.as_str(resolver.interner);

    let mut current_module = resolver.current_module;

    let first_res = if path.is_absolute {
        resolver
            .resolve_name_in_module(resolver.module_tree.root.def_id, ns, first.ident.symbol)
    } else if first_str == "crate" {
        current_module = resolver.module_tree.root.def_id;
        None
    } else if first_str == "self" {
        current_module = resolver.current_module;
        None
    } else if first_str == "super" {
        current_module = resolver
            .module_tree
            .modules
            .get(&resolver.current_module)
            .and_then(|m| m.parent)
            .unwrap_or(resolver.module_tree.root.def_id);
        None
    } else {
        // Try local ribs first, then module.
        resolver.resolve_name(ns, first.ident.symbol).map(|res| match res {
            Resolution::Def { def_id } => Some(def_id),
            _ => None,
        }).unwrap_or_else(|| {
            resolver.resolve_name_in_module(current_module, ns, first.ident.symbol)
        })
    };

    let mut current = match first_res {
        Some(def_id) => def_id,
        None => {
            // For self/crate/super, the first segment is just the anchor.
            if first_str == "crate" || first_str == "self" || first_str == "super" {
                // Continue with subsequent segments in the anchored module.
                if path.segments.len() == 1 {
                    return None;
                }
                let second = &path.segments[1];
                let second_res = resolver
                    .resolve_name_in_module(current_module, ns, second.ident.symbol)
                    .or_else(|| {
                        resolver
                            .resolve_name_in_module(current_module, Namespace::Type, second.ident.symbol)
                    });
                match second_res {
                    Some(def_id) => {
                        // Continue from this def_id for remaining segments.
                        let mut cur = def_id;
                        for seg in &path.segments[2..] {
                            let next = resolver
                                .resolve_name_in_module(cur, ns, seg.ident.symbol)
                                .or_else(|| {
                                    resolver.resolve_name_in_module(cur, Namespace::Type, seg.ident.symbol)
                                });
                            match next {
                                Some(d) => cur = d,
                                None => return None,
                            }
                        }
                        return Some(Resolution::Def { def_id: cur });
                    }
                    None => return None,
                }
            }
            return None;
        }
    };

    for seg in &path.segments[1..] {
        let next = resolver
            .resolve_name_in_module(current, ns, seg.ident.symbol)
            .or_else(|| resolver.resolve_name_in_module(current, Namespace::Type, seg.ident.symbol));
        match next {
            Some(def_id) => current = def_id,
            None => return None,
        }
    }

    Some(Resolution::Def { def_id: current })
}

pub fn resolve_type_path(resolver: &Resolver, path: &Path) -> Option<Resolution> {
    resolve_path(resolver, path, Namespace::Type)
}

pub fn resolve_value_path(resolver: &Resolver, path: &Path) -> Option<Resolution> {
    resolve_path(resolver, path, Namespace::Value)
}
