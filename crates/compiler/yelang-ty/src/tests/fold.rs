use crate::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use crate::generic::GenericArg;
use crate::interner::Interner;
use crate::predicate::TraitRef;
use crate::primitive::IntTy;
use crate::projection::ProjectionTy;
use crate::ty::{ParamTy, Ty, TyId};
use yelang_arena::DefId;

struct IdentityFolder<'a> {
    interner: &'a Interner,
}

impl<'a> TypeFolder for IdentityFolder<'a> {
    fn interner(&self) -> &Interner {
        self.interner
    }

    fn fold_ty(&mut self, ty: TyId) -> TyId {
        ty
    }
}

#[test]
fn fold_identity() {
    let interner = Interner::new();
    let t = interner.mk_ty(Ty::Int(IntTy::I32));
    let mut folder = IdentityFolder { interner: &interner };
    let folded = t.fold_with(&mut folder);
    assert_eq!(t, folded);
}

#[test]
fn fold_visits_all_nodes() {
    let interner = Interner::new();
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let t_bool = interner.mk_ty(Ty::Bool);
    let tuple = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
        GenericArg::Type(t_i32),
        GenericArg::Type(t_bool),
    ])));

    let mut folder = IdentityFolder { interner: &interner };
    let folded = tuple.fold_with(&mut folder);
    assert_eq!(tuple, folded);
}

#[test]
fn fold_substitutes_param() {
    let interner = Interner::new();
    let t_param = interner.mk_ty(Ty::Param(ParamTy {
        index: 0,
        name: yelang_interner::Symbol::from(1),
    }));
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let tuple = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
        GenericArg::Type(t_param),
    ])));

    struct ReplaceParamFolder<'a> {
        interner: &'a Interner,
        replacement: TyId,
    }

    impl<'a> TypeFolder for ReplaceParamFolder<'a> {
        fn interner(&self) -> &Interner {
            self.interner
        }

        fn fold_ty(&mut self, ty: TyId) -> TyId {
            if let Ty::Param(_) = self.interner.ty(ty) {
                self.replacement
            } else {
                ty.super_fold_with(self)
            }
        }
    }

    let mut folder = ReplaceParamFolder {
        interner: &interner,
        replacement: t_i32,
    };
    let folded = tuple.fold_with(&mut folder);

    match interner.ty(folded) {
        Ty::Tuple(args) => {
            assert_eq!(args.len(), 1);
            assert_eq!(args[0].expect_type(), t_i32);
        }
        _ => panic!("expected tuple"),
    }
}

#[test]
fn fold_substitutes_projection_args() {
    let interner = Interner::new();
    let t_param = interner.mk_ty(Ty::Param(ParamTy {
        index: 0,
        name: yelang_interner::Symbol::from(1),
    }));
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let trait_ref = TraitRef {
        def_id: DefId::new(1),
        args: interner.mk_generic_args(&[GenericArg::Type(t_param)]),
    };
    let projection = interner.mk_ty(Ty::Projection(ProjectionTy {
        trait_ref,
        item_def_id: DefId::new(2),
    }));

    struct ReplaceParamFolder<'a> {
        interner: &'a Interner,
        replacement: TyId,
    }

    impl<'a> TypeFolder for ReplaceParamFolder<'a> {
        fn interner(&self) -> &Interner {
            self.interner
        }

        fn fold_ty(&mut self, ty: TyId) -> TyId {
            if let Ty::Param(_) = self.interner.ty(ty) {
                self.replacement
            } else {
                ty.super_fold_with(self)
            }
        }
    }

    let mut folder = ReplaceParamFolder {
        interner: &interner,
        replacement: t_i32,
    };
    let folded = projection.fold_with(&mut folder);

    match interner.ty(folded) {
        Ty::Projection(proj) => {
            assert_eq!(proj.trait_ref.args[0].expect_type(), t_i32);
        }
        _ => panic!("expected projection"),
    }
}

#[test]
fn fold_substitutes_anon_struct_fields() {
    let interner = Interner::new();
    let t_param = interner.mk_ty(Ty::Param(ParamTy {
        index: 0,
        name: yelang_interner::Symbol::from(1),
    }));
    let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
    let anon = interner.mk_ty(Ty::AnonStruct(crate::ty::AnonStructDef {
        fields: interner.mk_anon_struct_fields(&[
            crate::ty::AnonField {
                name: yelang_interner::Symbol::from(1),
                ty: t_param,
            },
        ]),
    }));

    struct ReplaceParamFolder<'a> {
        interner: &'a Interner,
        replacement: TyId,
    }

    impl<'a> TypeFolder for ReplaceParamFolder<'a> {
        fn interner(&self) -> &Interner {
            self.interner
        }

        fn fold_ty(&mut self, ty: TyId) -> TyId {
            if let Ty::Param(_) = self.interner.ty(ty) {
                self.replacement
            } else {
                ty.super_fold_with(self)
            }
        }
    }

    let mut folder = ReplaceParamFolder {
        interner: &interner,
        replacement: t_i32,
    };
    let folded = anon.fold_with(&mut folder);

    match interner.ty(folded) {
        Ty::AnonStruct(def) => {
            assert_eq!(def.fields[0].ty, t_i32);
        }
        _ => panic!("expected anon struct"),
    }
}
