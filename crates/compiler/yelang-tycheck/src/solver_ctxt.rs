/*! `impl SolverCtxt for TyCtxt`.
 *
 * Bridges the type-checker's global tables with the next-generation trait
 * solver. The solver is intentionally decoupled from `yelang-hir`; this module
 * is the single adapter that translates `TyCtxt` data into the solver's view.
 *
 * The solver-facing views are precomputed by `TyCtxt::populate_solver_caches`
 * after type collection. This lets the `SolverCtxt` methods return stable
 * slices without interior mutability or allocation on each query.
 */

use yelang_arena::DefId;
use yelang_trait_solver::solver_ctx::{
    AssocItemInfo, AssocItemKind, BuiltinTraitKind, ImplInfo, SolverCtxt, TraitDefInfo,
};
use yelang_ty::predicate::TraitRef;
use yelang_ty::ty::{GenericArgsRef, TyId};

use crate::tcx::{ImplDefData, ImplItemDefData, TraitItemDefData, TyCtxt};

impl SolverCtxt for TyCtxt {
    fn interner(&self) -> &yelang_ty::interner::Interner {
        self.interner()
    }

    fn trait_info(&self, def_id: DefId) -> Option<TraitDefInfo> {
        self.trait_defs.get(def_id).map(|tr| TraitDefInfo {
            def_id: tr.def_id,
            is_auto: false, // TODO: auto traits once declared in HIR.
            supertraits: tr.supertraits.clone(),
        })
    }

    fn impls_for_trait(&self, def_id: DefId) -> &[ImplInfo] {
        self.trait_impl_info_cache
            .get(&def_id)
            .map(|b| b.as_ref())
            .unwrap_or(&[])
    }

    fn builtin_kind(&self, def_id: DefId) -> Option<BuiltinTraitKind> {
        self.builtin_traits.get(&def_id).copied()
    }

    fn trait_assoc_items(&self, def_id: DefId) -> &[AssocItemInfo] {
        self.trait_assoc_items_cache
            .get(&def_id)
            .map(|b| b.as_ref())
            .unwrap_or(&[])
    }

    fn impl_assoc_items(&self, impl_def_id: DefId) -> &[AssocItemInfo] {
        self.impl_assoc_items_cache
            .get(&impl_def_id)
            .map(|b| b.as_ref())
            .unwrap_or(&[])
    }

    fn adt_field_tys(&self, adt_def_id: DefId) -> &[TyId] {
        self.adt_field_tys_cache
            .get(&adt_def_id)
            .map(|b| b.as_ref())
            .unwrap_or(&[])
    }
}

impl TyCtxt {
    /// Precompute the solver-facing views stored in this context.
    ///
    /// Call this once after `collect_crate_types` and after all built-in trait
    /// registrations are complete.
    pub fn populate_solver_caches(&mut self) {
        // Trait impl info.
        let trait_ids: Vec<DefId> = self.trait_impl_index.keys().copied().collect();
        for trait_id in trait_ids {
            let impl_ids = self
                .trait_impl_index
                .get(&trait_id)
                .cloned()
                .unwrap_or_default();
            let infos: Vec<_> = impl_ids
                .into_iter()
                .map(|impl_id| impl_info_from_impl(&self.impl_defs[impl_id]))
                .collect();
            self.trait_impl_info_cache
                .insert(trait_id, infos.into_boxed_slice());
        }

        // Trait assoc items.
        let trait_ids: Vec<DefId> = self.trait_defs.iter_enumerated().map(|(k, _)| k).collect();
        for trait_id in trait_ids {
            if let Some(trait_def) = self.trait_defs.get(trait_id) {
                let infos: Vec<_> = trait_def.items.iter().map(trait_item_to_assoc).collect();
                self.trait_assoc_items_cache
                    .insert(trait_id, infos.into_boxed_slice());
            }
        }

        // Impl assoc items.
        let impl_ids: Vec<DefId> = self.impl_defs.iter().map(|imp| imp.def_id).collect();
        for impl_id in impl_ids {
            if let Some(imp) = self.impl_defs.iter().find(|imp| imp.def_id == impl_id) {
                let trait_items = imp
                    .trait_ref
                    .and_then(|tr| self.trait_defs.get(tr.def_id))
                    .map(|tr| tr.items.as_slice())
                    .unwrap_or(&[]);
                let infos: Vec<_> = imp
                    .items
                    .iter()
                    .map(|item| impl_item_to_assoc(item, trait_items))
                    .collect();
                self.impl_assoc_items_cache
                    .insert(impl_id, infos.into_boxed_slice());
            }
        }

        // ADT field types.
        let adt_ids: Vec<DefId> = self.adt_defs.iter_enumerated().map(|(k, _)| k).collect();
        for adt_id in adt_ids {
            if let Some(adt) = self.adt_defs.get(adt_id) {
                let mut tys = Vec::new();
                for variant in &adt.variants {
                    for field in &variant.fields {
                        tys.push(field.ty);
                    }
                }
                self.adt_field_tys_cache
                    .insert(adt_id, tys.into_boxed_slice());
            }
        }
    }
}

fn impl_info_from_impl(imp: &ImplDefData) -> ImplInfo {
    ImplInfo {
        def_id: imp.def_id,
        trait_ref: imp.trait_ref.unwrap_or_else(|| TraitRef {
            def_id: DefId::new(0),
            args: GenericArgsRef::empty(),
        }),
        polarity: yelang_ty::ty::ImplPolarity::Positive,
        generic_param_count: imp.generics.params.len(),
        predicates: imp.generics.predicates.clone(),
    }
}

fn trait_item_to_assoc(item: &TraitItemDefData) -> AssocItemInfo {
    match item {
        TraitItemDefData::Fn { def_id, ident, sig } => AssocItemInfo {
            def_id: *def_id,
            trait_item_def_id: Some(*def_id),
            ident: ident.symbol,
            kind: AssocItemKind::Fn { sig: *sig },
        },
        TraitItemDefData::Const { def_id, ident, ty } => AssocItemInfo {
            def_id: *def_id,
            trait_item_def_id: Some(*def_id),
            ident: ident.symbol,
            kind: AssocItemKind::Const { ty: *ty },
        },
        TraitItemDefData::Type {
            def_id,
            ident,
            bounds,
            default,
        } => AssocItemInfo {
            def_id: *def_id,
            trait_item_def_id: Some(*def_id),
            ident: ident.symbol,
            kind: AssocItemKind::Type {
                bounds: bounds.clone(),
                default: *default,
            },
        },
    }
}

fn impl_item_to_assoc(item: &ImplItemDefData, trait_items: &[TraitItemDefData]) -> AssocItemInfo {
    let trait_item_def_id = trait_items
        .iter()
        .find(|tr_item| tr_item.ident().symbol == item.ident().symbol)
        .map(|tr_item| tr_item.def_id());

    match item {
        ImplItemDefData::Fn { def_id, ident, sig } => AssocItemInfo {
            def_id: *def_id,
            trait_item_def_id,
            ident: ident.symbol,
            kind: AssocItemKind::Fn { sig: *sig },
        },
        ImplItemDefData::Const { def_id, ident, ty } => AssocItemInfo {
            def_id: *def_id,
            trait_item_def_id,
            ident: ident.symbol,
            kind: AssocItemKind::Const { ty: *ty },
        },
        ImplItemDefData::Type { def_id, ident, ty } => AssocItemInfo {
            def_id: *def_id,
            trait_item_def_id,
            ident: ident.symbol,
            kind: AssocItemKind::Type {
                bounds: vec![],
                default: Some(*ty),
            },
        },
    }
}
