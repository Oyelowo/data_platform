/*! Structural visiting over types.
 *
 * `TypeVisitor` allows inspecting types without modifying them.
 */

use crate::ty::{Const, Ty, TyKind};

/// Control flow for visitation.
pub type ControlFlow = std::ops::ControlFlow<()>;

/// A visitor that inspects types.
pub trait TypeVisitor<'tcx>: Sized {
    fn visit_ty(&mut self, ty: Ty<'tcx>) -> ControlFlow {
        ty.super_visit_with(self)
    }

    fn visit_const(&mut self, _ct: Const<'tcx>) -> ControlFlow {
        ControlFlow::Continue(())
    }
}

/// Super-visit: the default structural traversal.
pub trait TypeSuperVisitable<'tcx> {
    fn super_visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow;
}

impl<'tcx> TypeSuperVisitable<'tcx> for Ty<'tcx> {
    fn super_visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow {
        match self.kind() {
            TyKind::Bool
            | TyKind::Char
            | TyKind::Str
            | TyKind::Int(_)
            | TyKind::Uint(_)
            | TyKind::Float(_)
            | TyKind::Param(_)
            | TyKind::Bound(_, _)
            | TyKind::Infer(_)
            | TyKind::Never
            | TyKind::TypeLit(_)
            | TyKind::Placeholder(_)
            | TyKind::Error => ControlFlow::Continue(()),
            TyKind::Adt(_, args) => {
                for arg in args.iter() {
                    match arg {
                        crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                        crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                    }
                }
                ControlFlow::Continue(())
            }
            TyKind::FnPtr(sig) => {
                for arg in sig.sig.inputs.iter() {
                    match arg {
                        crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                        crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                    }
                }
                visitor.visit_ty(sig.sig.output)
            }
            TyKind::FnDef(fd) => {
                for arg in fd.args.iter() {
                    match arg {
                        crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                        crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                    }
                }
                ControlFlow::Continue(())
            }
            TyKind::Tuple(args) => {
                for arg in args.iter() {
                    match arg {
                        crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                        crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                    }
                }
                ControlFlow::Continue(())
            }
            TyKind::Array(ty, ct) => {
                visitor.visit_ty(*ty)?;
                visitor.visit_const(*ct)
            }
            TyKind::Slice(ty) => visitor.visit_ty(*ty),
            TyKind::RawPtr(tam) => visitor.visit_ty(tam.ty),
            TyKind::Ref(ty, _) => visitor.visit_ty(*ty),
            TyKind::AnonStruct(anon) => {
                for f in anon.fields.iter() {
                    visitor.visit_ty(f.ty)?;
                }
                ControlFlow::Continue(())
            }
            TyKind::Union(a, b) => {
                visitor.visit_ty(*a)?;
                visitor.visit_ty(*b)
            }
            TyKind::Utility(_, args) => {
                for arg in args.iter() {
                    match arg {
                        crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                        crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                    }
                }
                ControlFlow::Continue(())
            }
            TyKind::Alias(alias) => {
                for arg in alias.args.iter() {
                    match arg {
                        crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                        crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                    }
                }
                ControlFlow::Continue(())
            }
            TyKind::Dynamic(binder) => match binder.value {
                crate::ty::ExistentialPredicate::Trait(tr) => {
                    for arg in tr.args.iter() {
                        match arg {
                            crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                            crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                        }
                    }
                    ControlFlow::Continue(())
                }
                crate::ty::ExistentialPredicate::Projection(pr) => {
                    for arg in pr.args.iter() {
                        match arg {
                            crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
                            crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
                        }
                    }
                    visitor.visit_ty(pr.term)
                }
                crate::ty::ExistentialPredicate::AutoTrait(_) => ControlFlow::Continue(()),
            },
        }
    }
}

impl<'tcx> TypeSuperVisitable<'tcx> for Const<'tcx> {
    fn super_visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow {
        visitor.visit_ty(self.ty)
    }
}
