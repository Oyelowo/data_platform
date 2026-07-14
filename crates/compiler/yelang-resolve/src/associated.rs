use yelang_interner::Symbol;
use yelang_util::DefId;
use yelang_ast::Path;

use crate::{
    namespaces::Namespace,
    rib::Resolution,
    scope::Resolver,
};

/// Resolve an associated item path like `Type::item` or `<Type as Trait>::item`.
/// Returns the DefId of the resolved item if found.
pub fn resolve_associated_item(
    resolver: &Resolver,
    path: &Path,
    _ns: Namespace,
) -> Option<Resolution> {
    if let Some(qself) = &path.qself {
        // <Type as Trait>::item
        resolve_qualified_associated_item(resolver, qself, path)
    } else if path.segments.len() >= 2 {
        // Type::item (possibly with module prefix like module::Type::item)
        resolve_inherent_associated_item(resolver, path)
    } else {
        None
    }
}

fn resolve_inherent_associated_item(
    resolver: &Resolver,
    path: &Path,
) -> Option<Resolution> {
    if path.segments.len() < 2 {
        return None;
    }

    let item_name = path.segments.last().unwrap().ident.symbol;
    let type_segment = path.segments[path.segments.len() - 2].ident.symbol;

    // If the path uses `Self`, map to the actual type being implemented.
    let self_symbol = resolver.interner.get_or_intern("Self");
    let type_name = if type_segment == self_symbol {
        resolver.self_type?
    } else {
        type_segment
    };

    // Look up inherent impls for this type
    if let Some(impl_ids) = resolver.inherent_impls.get(&type_name) {
        for impl_id in impl_ids {
            if let Some(items) = resolver.impl_item_names.get(impl_id) {
                if let Some(&item_def_id) = items.get(&item_name) {
                    return Some(Resolution::Def { def_id: item_def_id });
                }
            }
        }
    }

    // Fallback: search trait impls for this type (e.g. Foo::show where impl Show for Foo)
    search_trait_impls_for_type(resolver, type_name, item_name)
}

fn resolve_qualified_associated_item(
    resolver: &Resolver,
    qself: &yelang_ast::QSelf,
    path: &Path,
) -> Option<Resolution> {
    let item_name = path.segments.last()?.ident.symbol;

    let type_name = extract_type_name(&qself.ty)?;
    let trait_name = if let Some(trait_path) = &qself.as_trait {
        trait_path.segments.last()?.ident.symbol
    } else {
        // <Type>::item — look up in inherent impls directly using qself type
        if let Some(impl_ids) = resolver.inherent_impls.get(&type_name) {
            for impl_id in impl_ids {
                if let Some(items) = resolver.impl_item_names.get(impl_id) {
                    if let Some(&item_def_id) = items.get(&item_name) {
                        return Some(Resolution::Def { def_id: item_def_id });
                    }
                }
            }
        }
        // Also check trait impls as fallback (e.g. <Foo>::show where impl Show for Foo)
        return search_trait_impls_for_type(resolver, type_name, item_name);
    };

    // Look up trait impls for (trait_name, type_name)
    let key = (trait_name, type_name);
    if let Some(impl_ids) = resolver.trait_impls.get(&key) {
        for impl_id in impl_ids {
            if let Some(items) = resolver.impl_item_names.get(impl_id) {
                if let Some(&item_def_id) = items.get(&item_name) {
                    return Some(Resolution::Def { def_id: item_def_id });
                }
            }
        }
    }

    None
}

/// Search all trait impls for a given type, looking for an item with the given name.
/// Used for unqualified trait method access like `Foo::show` where `impl Show for Foo`.
fn search_trait_impls_for_type(
    resolver: &Resolver,
    type_name: Symbol,
    item_name: Symbol,
) -> Option<Resolution> {
    for ((_trait_name, impl_type_name), impl_ids) in &resolver.trait_impls {
        if *impl_type_name == type_name {
            for impl_id in impl_ids {
                if let Some(items) = resolver.impl_item_names.get(impl_id) {
                    if let Some(&item_def_id) = items.get(&item_name) {
                        return Some(Resolution::Def { def_id: item_def_id });
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn extract_type_name(ty: &yelang_ast::Type) -> Option<Symbol> {
    match &ty.kind {
        yelang_ast::TypeKind::Named(path) => {
            path.segments.first().map(|s| s.ident.symbol)
        }
        yelang_ast::TypeKind::Ref { ty, .. } => extract_type_name(ty),
        _ => None,
    }
}
