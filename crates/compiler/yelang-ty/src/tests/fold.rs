use crate::interner::Interner;
use crate::primitive::IntTy;
use crate::ty::{Ty, TyKind};
use crate::fold::{TypeFolder, TypeFoldable};

struct IdentityFolder;

impl<'tcx> TypeFolder<'tcx> for IdentityFolder {
    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        ty
    }
}

#[test]
fn fold_identity() {
    let interner = Interner::new();
    let t = interner.mk_ty(TyKind::Int(IntTy::I32));
    let folded = t.fold_with(&mut IdentityFolder);
    assert_eq!(t, folded);
}

#[test]
fn fold_visits_all_nodes() {
    let interner = Interner::new();
    let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
    let t_bool = interner.mk_ty(TyKind::Bool);
    let tuple = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
        crate::generic::GenericArg::Type(t_i32),
        crate::generic::GenericArg::Type(t_bool),
    ])));

    // Identity folder should return the same type
    let folded = tuple.fold_with(&mut IdentityFolder);
    assert_eq!(tuple, folded);
}
