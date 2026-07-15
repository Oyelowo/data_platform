use yelang_ast::Path;

use crate::namespaces::Namespace;
use crate::rib::Resolution;
use crate::scope::Resolver;

/// Resolve a path in the given namespace, starting from the current module.
pub fn resolve_path(resolver: &Resolver, path: &Path, ns: Namespace) -> Option<Resolution> {
    if path.segments.is_empty() {
        return None;
    }

    // Try standard path resolution first.
    let result = resolve_path_standard(resolver, path, ns);

    // If standard path resolution fails, try associated item resolution.
    if result.is_none() && (path.qself.is_some() || path.segments.len() >= 2) {
        crate::associated::resolve_associated_item(resolver, path, ns)
    } else {
        result
    }
}

fn resolve_path_standard(resolver: &Resolver, path: &Path, ns: Namespace) -> Option<Resolution> {
    let first = &path.segments[0];
    let first_str = first.ident.as_str(resolver.interner);

    let mut current_module = resolver.current_module;

    // Handle the first segment.
    let first_res = if path.is_absolute {
        resolver
            .resolve_name_in_module(resolver.module_tree.root.def_id, ns, first.ident.symbol)
            .map(|def_id| Resolution::Def { def_id })
    } else if first.ident.origin == yelang_ast::tokenizer::IdentOrigin::Crate {
        // `$crate` expands to a path anchored at the macro's defining crate. In
        // single-crate mode this is the crate root; multi-crate builds can refine
        // this via `ident.crate_ref` once crate ids flow through expansion.
        current_module = resolver.module_tree.root.def_id;
        None
    } else if first_str == "crate" {
        current_module = resolver.module_tree.root.def_id;
        None
    } else if first_str == "self" {
        // `self` can be either a local variable binding (e.g. method receiver)
        // or a module-relative path anchor (`self::foo`). For single-segment
        // paths, try the local scope first; otherwise fall through to the
        // anchor logic below.
        if path.segments.len() == 1 {
            if let Some(res) = resolver.resolve_name(ns, first.ident.symbol) {
                return Some(res);
            }
        }
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
        // For value paths, also try the type namespace (e.g. modules are types).
        resolver.resolve_name(ns, first.ident.symbol).or_else(|| {
            resolver
                .resolve_name_in_module(current_module, ns, first.ident.symbol)
                .or_else(|| {
                    resolver.resolve_name_in_module(
                        current_module,
                        Namespace::Type,
                        first.ident.symbol,
                    )
                })
                .map(|def_id| Resolution::Def { def_id })
        })
    };

    match first_res {
        Some(Resolution::Local { .. }) if path.segments.len() == 1 => {
            // Single-segment local variable path – return immediately.
            first_res
        }
        Some(Resolution::Def { def_id }) => {
            // Continue resolving remaining segments through the module tree.
            let mut current = def_id;
            for seg in &path.segments[1..] {
                let next = resolver
                    .resolve_name_in_module(current, ns, seg.ident.symbol)
                    .or_else(|| {
                        resolver.resolve_name_in_module(current, Namespace::Type, seg.ident.symbol)
                    });
                match next {
                    Some(d) => current = d,
                    None => return None,
                }
            }
            Some(Resolution::Def { def_id: current })
        }
        _ => {
            // First segment was an anchor (crate/self/super) with no resolution yet.
            if first_str == "crate" || first_str == "self" || first_str == "super" {
                if path.segments.len() == 1 {
                    return None;
                }
                let second = &path.segments[1];
                let second_res = resolver
                    .resolve_name_in_module(current_module, ns, second.ident.symbol)
                    .or_else(|| {
                        resolver.resolve_name_in_module(
                            current_module,
                            Namespace::Type,
                            second.ident.symbol,
                        )
                    });
                match second_res {
                    Some(def_id) => {
                        let mut cur = def_id;
                        for seg in &path.segments[2..] {
                            let next = resolver
                                .resolve_name_in_module(cur, ns, seg.ident.symbol)
                                .or_else(|| {
                                    resolver.resolve_name_in_module(
                                        cur,
                                        Namespace::Type,
                                        seg.ident.symbol,
                                    )
                                });
                            match next {
                                Some(d) => cur = d,
                                None => return None,
                            }
                        }
                        Some(Resolution::Def { def_id: cur })
                    }
                    None => None,
                }
            } else {
                None
            }
        }
    }
}

pub fn resolve_type_path(resolver: &Resolver, path: &Path) -> Option<Resolution> {
    resolve_path(resolver, path, Namespace::Type)
}

pub fn resolve_value_path(resolver: &Resolver, path: &Path) -> Option<Resolution> {
    resolve_path(resolver, path, Namespace::Value)
}
