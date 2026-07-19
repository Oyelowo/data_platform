//! Introspection of the real `.ye` standard library.
//!
//! This module replaces the earlier hard-coded `QueryableMethod` and
//! `aggregate_classes` registries by reading the lang-item traits and impls
//! that were actually loaded from `stdlib/core/src/*.ye`.

use yelang_arena::{DefId, FxHashMap};
use yelang_hir::hir::core::{ImplItemKind, ItemKind};
use yelang_hir::hir::expr::Expr;
use yelang_hir::res::Res;
use yelang_resolve::lang_items::LangItem;
use yelang_tycheck::tcx::TyCtxt;

use crate::errors::LoweringError;
use crate::expr::AggregateClass;
use crate::logical::queryable::QueryableMethod;

/// Build a map from `Queryable` trait method `DefId` to the LIR operator it
/// represents. The mapping is driven solely by the method name in the trait
/// definition loaded from `stdlib/core/src/query.ye`.
pub fn build_queryable_method_table(tcx: &TyCtxt) -> FxHashMap<DefId, QueryableMethod> {
    let mut table = FxHashMap::default();
    let Some(queryable_trait) = tcx.lang_item(LangItem::Queryable) else {
        return table;
    };

    let Some(trait_def) = tcx.trait_def(queryable_trait) else {
        return table;
    };

    for item in &trait_def.items {
        let name = tcx.resolve_symbol(item.ident().symbol);
        let method = match name {
            Some("filter") => QueryableMethod::Filter,
            Some("map") => QueryableMethod::Map,
            Some("flat_map") => QueryableMethod::FlatMap,
            Some("take") => QueryableMethod::Take,
            Some("skip") => QueryableMethod::Skip,
            Some("order_by") => QueryableMethod::OrderBy,
            Some("distinct") => QueryableMethod::Distinct,
            Some("group_by") => QueryableMethod::GroupBy,
            Some("aggregate") => QueryableMethod::Aggregate,
            Some("sum") => QueryableMethod::Sum,
            Some("product") => QueryableMethod::Product,
            Some("avg") => QueryableMethod::Avg,
            Some("count") => QueryableMethod::Count,
            Some("execute") => QueryableMethod::Execute,
            _ => continue,
        };
        table.insert(item.def_id(), method);
    }

    table
}

/// Build a map from aggregate marker type `DefId` to its `AggregateClass` by
/// inspecting the bodies of `Aggregate::class()` in each `impl Aggregate for Marker`.
///
/// This is the principled replacement for a hard-coded registry: the compiler
/// reads the classification directly from the stdlib trait impls.
pub fn build_aggregate_class_table(tcx: &TyCtxt) -> Result<FxHashMap<DefId, AggregateClass>, LoweringError> {
    let mut table = FxHashMap::default();
    let Some(aggregate_trait) = tcx.lang_item(LangItem::Aggregate) else {
        return Ok(table);
    };

    // Build a map from enum variant DefId -> (enum name, variant name) so we can
    // recognise `AggregateClass::Distributive` etc. without hard-coding DefIds.
    let mut variant_names: FxHashMap<DefId, (String, String)> = FxHashMap::default();
    for (_, item) in tcx.crate_hir().items.iter_enumerated() {
        let Some(item) = item.as_ref() else { continue };
        if let ItemKind::Enum { def, .. } = &item.kind {
            let enum_name = tcx
                .resolve_symbol(item.ident.symbol)
                .unwrap_or("<unknown>")
                .to_string();
            for variant in &def.variants {
                let variant_name = tcx
                    .resolve_symbol(variant.ident.symbol)
                    .unwrap_or("<unknown>")
                    .to_string();
                variant_names.insert(variant.def_id, (enum_name.clone(), variant_name));
            }
        }
    }

    for imp in &tcx.crate_hir().impls {
        let Some(tr) = &imp.of_trait else { continue };
        let Res::Def { def_id: trait_def_id } = tr.path else { continue };
        if trait_def_id != aggregate_trait {
            continue;
        }

        // The marker type being implemented.
        let marker_def_id = match tcx.crate_hir().ty(imp.self_ty) {
            Some(yelang_hir::hir::ty::Ty::Path { res: Res::Def { def_id }, .. }) => *def_id,
            _ => continue,
        };

        for item in &imp.items {
            let name = tcx.resolve_symbol(item.ident.symbol);
            if name != Some("class") {
                continue;
            }
            let ImplItemKind::Fn { body, .. } = &item.kind else { continue };
            let Some(body) = tcx.crate_hir().body(*body) else { continue };
            let Some(expr) = tcx.crate_hir().expr(body.value) else { continue };

            let class = match expr {
                Expr::Path { res: Res::Def { def_id } } => {
                    let Some((enum_name, variant_name)) = variant_names.get(def_id) else {
                        continue;
                    };
                    if enum_name != "AggregateClass" {
                        continue;
                    }
                    match variant_name.as_str() {
                        "Distributive" => AggregateClass::Distributive,
                        "Algebraic" => AggregateClass::Algebraic,
                        "Holistic" => AggregateClass::Holistic,
                        _ => continue,
                    }
                }
                _ => continue,
            };

            table.insert(marker_def_id, class);
        }
    }

    Ok(table)
}

/// Convenience: build both stdlib introspection tables.
pub fn build_tables(tcx: &TyCtxt) -> Result<(FxHashMap<DefId, QueryableMethod>, FxHashMap<DefId, AggregateClass>), LoweringError> {
    let methods = build_queryable_method_table(tcx);
    let classes = build_aggregate_class_table(tcx)?;
    Ok((methods, classes))
}

/// Resolve an aggregate marker type `DefId` to its `AggregateClass`, falling
/// back to the compiled-in classification for the three built-in markers if
/// introspection did not find them (e.g. when the stdlib is not loaded).
pub fn aggregate_class(tcx: &TyCtxt, tables: &FxHashMap<DefId, AggregateClass>, def_id: DefId) -> Option<AggregateClass> {
    if let Some(&class) = tables.get(&def_id) {
        return Some(class);
    }

    // Fallback using the type name. This keeps the standalone QIR unit tests
    // working even without the full stdlib prelude.
    let name = tcx.crate_hir()
        .items
        .get(def_id)
        .and_then(|i| i.as_ref())
        .map(|i| i.ident.symbol);
    if let Some(name) = name.and_then(|s| tcx.resolve_symbol(s)) {
        match name {
            "Sum" | "Count" => return Some(AggregateClass::Distributive),
            "Avg" => return Some(AggregateClass::Algebraic),
            _ => {}
        }
    }

    None
}
