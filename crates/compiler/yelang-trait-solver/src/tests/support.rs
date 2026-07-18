/*! Test support for the trait solver.
 *
 * Provides a `TestCtxt` implementing `SolverCtxt` so solver tests do not
 * depend on `yelang-tycheck` or `yelang-hir`.
 */

use yelang_arena::{DefId, FxHashMap};
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{ParamEnv, Predicate, TraitPredicate, TraitRef};
use yelang_ty::ty::{ImplPolarity, Ty, TyKind};

use crate::solver_ctx::{BuiltinTraitKind, ImplInfo, SolverCtxt, TraitDefInfo};

pub struct TestCtxt<'tcx> {
    interner: &'tcx Interner<'tcx>,
    traits: FxHashMap<DefId, TraitDefInfo<'tcx>>,
    impls: FxHashMap<DefId, Vec<ImplInfo<'tcx>>>,
    builtins: FxHashMap<DefId, BuiltinTraitKind>,
}

impl<'tcx> TestCtxt<'tcx> {
    pub fn new(interner: &'tcx Interner<'tcx>) -> Self {
        Self {
            interner,
            traits: FxHashMap::default(),
            impls: FxHashMap::default(),
            builtins: FxHashMap::default(),
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

    pub fn add_builtin(&mut self, def_id: DefId, kind: BuiltinTraitKind) {
        self.builtins.insert(def_id, kind);
    }

    pub fn add_impl(
        &mut self,
        def_id: DefId,
        trait_def_id: DefId,
        self_ty: Ty<'tcx>,
        generic_param_count: usize,
        predicates: Vec<Predicate<'tcx>>,
    ) {
        let trait_ref = self.trait_ref(trait_def_id, &[self_ty]);
        self.impls.entry(trait_def_id).or_default().push(ImplInfo {
            def_id,
            trait_ref,
            generic_param_count,
            predicates,
        });
    }

    pub fn trait_ref(&self, trait_def_id: DefId, args: &[Ty<'tcx>]) -> TraitRef<'tcx> {
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
        self_ty: Ty<'tcx>,
        param_env: ParamEnv<'tcx>,
    ) -> crate::goal::Goal<'tcx> {
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

    pub fn param_env(&self, bounds: &[Predicate<'tcx>]) -> ParamEnv<'tcx> {
        ParamEnv {
            caller_bounds: self.interner.mk_predicates(bounds),
        }
    }

    pub fn mk_i32(&self) -> Ty<'tcx> {
        self.interner
            .mk_ty(TyKind::Int(yelang_ty::primitive::IntTy::I32))
    }

    pub fn mk_vec(&self, elem: Ty<'tcx>) -> Ty<'tcx> {
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(elem)]);
        // Use a synthetic ADT def id for Vec.
        self.interner.mk_ty(TyKind::Adt(
            yelang_ty::ty::AdtDef {
                def_id: DefId::new(100),
            },
            args,
        ))
    }

    pub fn mk_wrapper(&self, inner: Ty<'tcx>) -> Ty<'tcx> {
        let args = self
            .interner
            .mk_generic_args(&[yelang_ty::generic::GenericArg::Type(inner)]);
        self.interner.mk_ty(TyKind::Adt(
            yelang_ty::ty::AdtDef {
                def_id: DefId::new(101),
            },
            args,
        ))
    }
}

impl<'tcx> SolverCtxt<'tcx> for TestCtxt<'tcx> {
    fn interner(&self) -> &Interner<'tcx> {
        self.interner
    }

    fn trait_info(&self, def_id: DefId) -> Option<TraitDefInfo<'tcx>> {
        self.traits.get(&def_id).cloned()
    }

    fn impls_for_trait(&self, def_id: DefId) -> &[ImplInfo<'tcx>] {
        self.impls.get(&def_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn builtin_kind(&self, def_id: DefId) -> Option<BuiltinTraitKind> {
        self.builtins.get(&def_id).copied()
    }
}
