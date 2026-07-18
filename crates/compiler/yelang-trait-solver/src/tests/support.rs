/*! Test support for the trait solver.
 *
 * Provides a `TestCtxt` implementing `SolverCtxt` so solver tests do not
 * depend on `yelang-tycheck` or `yelang-hir`.
 */

use yelang_arena::{DefId, FxHashMap};
use yelang_interner::Symbol;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{
    NormalizesToPredicate, ParamEnv, Predicate, ProjectionPredicate, TraitPredicate, TraitRef,
};
use yelang_ty::ty::{AdtDef, ImplPolarity, ProjectionTy, Ty, TyId};

use crate::solver_ctx::{
    AssocItemInfo, AssocItemKind, BuiltinTraitKind, ImplInfo, SolverCtxt, TraitDefInfo,
};

pub struct TestCtxt<'a> {
    interner: &'a Interner,
    traits: FxHashMap<DefId, TraitDefInfo>,
    impls: FxHashMap<DefId, Vec<ImplInfo>>,
    builtins: FxHashMap<DefId, BuiltinTraitKind>,
    trait_assoc_items: FxHashMap<DefId, Vec<AssocItemInfo>>,
    impl_assoc_items: FxHashMap<DefId, Vec<AssocItemInfo>>,
    adt_fields: FxHashMap<DefId, Vec<TyId>>,
}

impl<'a> TestCtxt<'a> {
    pub fn new(interner: &'a Interner) -> Self {
        Self {
            interner,
            traits: FxHashMap::default(),
            impls: FxHashMap::default(),
            builtins: FxHashMap::default(),
            trait_assoc_items: FxHashMap::default(),
            impl_assoc_items: FxHashMap::default(),
            adt_fields: FxHashMap::default(),
        }
    }

    pub fn add_trait(&mut self, def_id: DefId, is_auto: bool) {
        self.traits.insert(
            def_id,
            TraitDefInfo {
                def_id,
                is_auto,
                supertraits: Vec::new(),
            },
        );
    }

    pub fn add_trait_supertraits(&mut self, def_id: DefId, supertraits: Vec<TraitRef>) {
        if let Some(info) = self.traits.get_mut(&def_id) {
            info.supertraits = supertraits;
        }
    }

    pub fn add_builtin(&mut self, def_id: DefId, kind: BuiltinTraitKind) {
        self.builtins.insert(def_id, kind);
    }

    pub fn add_impl(
        &mut self,
        def_id: DefId,
        trait_def_id: DefId,
        self_ty: TyId,
        generic_param_count: usize,
        predicates: Vec<Predicate>,
    ) {
        self.add_impl_with_polarity(
            def_id,
            trait_def_id,
            self_ty,
            generic_param_count,
            predicates,
            ImplPolarity::Positive,
        );
    }

    pub fn add_negative_impl(
        &mut self,
        def_id: DefId,
        trait_def_id: DefId,
        self_ty: TyId,
        generic_param_count: usize,
        predicates: Vec<Predicate>,
    ) {
        self.add_impl_with_polarity(
            def_id,
            trait_def_id,
            self_ty,
            generic_param_count,
            predicates,
            ImplPolarity::Negative,
        );
    }

    fn add_impl_with_polarity(
        &mut self,
        def_id: DefId,
        trait_def_id: DefId,
        self_ty: TyId,
        generic_param_count: usize,
        predicates: Vec<Predicate>,
        polarity: ImplPolarity,
    ) {
        let trait_ref = self.trait_ref(trait_def_id, &[self_ty]);
        self.impls.entry(trait_def_id).or_default().push(ImplInfo {
            def_id,
            trait_ref,
            polarity,
            generic_param_count,
            predicates,
        });
    }

    pub fn add_trait_assoc_item(&mut self, trait_def_id: DefId, item: AssocItemInfo) {
        self.trait_assoc_items
            .entry(trait_def_id)
            .or_default()
            .push(item);
    }

    pub fn add_impl_assoc_item(&mut self, impl_def_id: DefId, item: AssocItemInfo) {
        self.impl_assoc_items
            .entry(impl_def_id)
            .or_default()
            .push(item);
    }

    pub fn set_adt_fields(&mut self, adt_def_id: DefId, fields: Vec<TyId>) {
        self.adt_fields.insert(adt_def_id, fields);
    }

    pub fn add_trait_assoc_type(&mut self, trait_def_id: DefId, item_def_id: DefId, ident: Symbol) {
        self.add_trait_assoc_item(
            trait_def_id,
            AssocItemInfo {
                def_id: item_def_id,
                trait_item_def_id: None,
                ident,
                kind: AssocItemKind::Type {
                    bounds: Vec::new(),
                    default: None,
                },
            },
        );
    }

    pub fn add_impl_assoc_type(
        &mut self,
        impl_def_id: DefId,
        item_def_id: DefId,
        trait_item_def_id: DefId,
        ident: Symbol,
        ty: TyId,
    ) {
        self.add_impl_assoc_item(
            impl_def_id,
            AssocItemInfo {
                def_id: item_def_id,
                trait_item_def_id: Some(trait_item_def_id),
                ident,
                kind: AssocItemKind::Type {
                    bounds: Vec::new(),
                    default: Some(ty),
                },
            },
        );
    }

    fn projection_ty(
        &self,
        trait_def_id: DefId,
        item_def_id: DefId,
        self_ty: TyId,
    ) -> ProjectionTy {
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(self_ty)]);
        ProjectionTy {
            trait_ref: TraitRef {
                def_id: trait_def_id,
                args,
            },
            item_def_id,
        }
    }

    pub fn projection_goal(
        &self,
        trait_def_id: DefId,
        item_def_id: DefId,
        self_ty: TyId,
        term: TyId,
        param_env: ParamEnv,
    ) -> crate::goal::Goal {
        let projection_ty = self.projection_ty(trait_def_id, item_def_id, self_ty);
        crate::goal::Goal::new(
            param_env,
            Predicate::Projection(ProjectionPredicate {
                projection_ty,
                term,
            }),
        )
    }

    pub fn normalizes_to_goal(
        &self,
        trait_def_id: DefId,
        item_def_id: DefId,
        self_ty: TyId,
        term: TyId,
        param_env: ParamEnv,
    ) -> crate::goal::Goal {
        let projection_ty = self.projection_ty(trait_def_id, item_def_id, self_ty);
        crate::goal::Goal::new(
            param_env,
            Predicate::NormalizesTo(NormalizesToPredicate {
                projection_ty,
                term,
            }),
        )
    }

    pub fn mk_adt(&self, def_id: DefId, args: &[TyId]) -> TyId {
        let args: Vec<_> = args
            .iter()
            .map(|&ty| yelang_ty::generic::GenericArg::Type(ty))
            .collect();
        self.interner.mk_ty(Ty::Adt(
            AdtDef { def_id },
            self.interner.mk_generic_args(&args),
        ))
    }

    pub fn trait_ref(&self, trait_def_id: DefId, args: &[TyId]) -> TraitRef {
        let args: Vec<_> = args
            .iter()
            .map(|&ty| yelang_ty::generic::GenericArg::Type(ty))
            .collect();
        TraitRef {
            def_id: trait_def_id,
            args: self.interner.mk_generic_args(&args),
        }
    }

    pub fn trait_goal(
        &self,
        trait_def_id: DefId,
        self_ty: TyId,
        param_env: ParamEnv,
    ) -> crate::goal::Goal {
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(self_ty)]);
        crate::goal::Goal::new(
            param_env,
            Predicate::Trait(TraitPredicate {
                trait_ref: TraitRef {
                    def_id: trait_def_id,
                    args,
                },
                polarity: ImplPolarity::Positive,
            }),
        )
    }

    pub fn param_env(&self, bounds: &[Predicate]) -> ParamEnv {
        ParamEnv {
            caller_bounds: self.interner.mk_predicates(bounds),
        }
    }

    pub fn mk_i32(&self) -> TyId {
        self.interner
            .mk_ty(Ty::Int(yelang_ty::primitive::IntTy::I32))
    }

    pub fn mk_vec(&self, elem: TyId) -> TyId {
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(elem)]);
        // Use a synthetic ADT def id for Vec.
        self.interner.mk_ty(Ty::Adt(
            yelang_ty::ty::AdtDef {
                def_id: DefId::new(100),
            },
            args,
        ))
    }

    pub fn mk_wrapper(&self, inner: TyId) -> TyId {
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(inner)]);
        self.interner.mk_ty(Ty::Adt(
            yelang_ty::ty::AdtDef {
                def_id: DefId::new(101),
            },
            args,
        ))
    }

    pub fn mk_pair(&self, a: TyId, _b: TyId) -> TyId {
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(a)]);
        let inner = self.interner.mk_ty(Ty::Adt(
            yelang_ty::ty::AdtDef {
                def_id: DefId::new(102),
            },
            args,
        ));
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(inner)]);
        self.interner.mk_ty(Ty::Adt(
            yelang_ty::ty::AdtDef {
                def_id: DefId::new(103),
            },
            args,
        ))
    }
}

impl<'a> SolverCtxt for TestCtxt<'a> {
    fn interner(&self) -> &Interner {
        self.interner
    }

    fn trait_info(&self, def_id: DefId) -> Option<TraitDefInfo> {
        self.traits.get(&def_id).cloned()
    }

    fn impls_for_trait(&self, def_id: DefId) -> &[ImplInfo] {
        self.impls.get(&def_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn builtin_kind(&self, def_id: DefId) -> Option<BuiltinTraitKind> {
        self.builtins.get(&def_id).copied()
    }

    fn trait_assoc_items(&self, def_id: DefId) -> &[AssocItemInfo] {
        self.trait_assoc_items
            .get(&def_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn impl_assoc_items(&self, impl_def_id: DefId) -> &[AssocItemInfo] {
        self.impl_assoc_items
            .get(&impl_def_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn adt_field_tys(&self, adt_def_id: DefId) -> &[TyId] {
        self.adt_fields
            .get(&adt_def_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}
