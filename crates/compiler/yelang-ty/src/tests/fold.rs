use crate::fold::{TypeFoldable, TypeFolder, TypeSuperFoldable};
use crate::generic::GenericArg;
use crate::interner::Interner;
use crate::primitive::IntTy;
use crate::ty::{ParamTy, Ty, TyKind};

struct IdentityFolder<'tcx> {
    interner: &'tcx Interner<'tcx>,
}

impl<'tcx> TypeFolder<'tcx> for IdentityFolder<'tcx> {
    fn interner(&self) -> &'tcx Interner<'tcx> {
        self.interner
    }

    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        ty
    }
}

#[test]
fn fold_identity() {
    let interner = Interner::new();
    let t = interner.mk_ty(TyKind::Int(IntTy::I32));
    let mut folder = IdentityFolder { interner: &interner };
    let folded = t.fold_with(&mut folder);
    assert_eq!(t, folded);
}

#[test]
fn fold_visits_all_nodes() {
    let interner = Interner::new();
    let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
    let t_bool = interner.mk_ty(TyKind::Bool);
    let tuple = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
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
    let t_param = interner.mk_ty(TyKind::Param(ParamTy {
        index: 0,
        name: yelang_interner::Symbol::from(1),
    }));
    let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
    let tuple = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
        GenericArg::Type(t_param),
    ])));

    struct ReplaceParamFolder<'tcx> {
        interner: &'tcx Interner<'tcx>,
        replacement: Ty<'tcx>,
    }

    impl<'tcx> TypeFolder<'tcx> for ReplaceParamFolder<'tcx> {
        fn interner(&self) -> &'tcx Interner<'tcx> {
            self.interner
        }

        fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
            if let TyKind::Param(_) = ty.kind() {
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

    match folded.kind() {
        TyKind::Tuple(args) => {
            assert_eq!(args.len(), 1);
            assert_eq!(args[0].expect_type(), t_i32);
        }
        _ => panic!("expected tuple"),
    }
}
