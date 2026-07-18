/*! Structural visiting over types.
 *
 * `TypeVisitor` allows inspecting types without modifying them.
 */

use crate::existential::ExistentialPredicate;
use crate::list::List;
use crate::predicate::Predicate;
use crate::ty::{Const, ConstKind, Ty, TyKind};

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

/// Types that can be structurally visited.
pub trait TypeVisitable<'tcx>: Sized {
    fn visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow;
}

/// Super-visit: the default structural traversal.
pub trait TypeSuperVisitable<'tcx> {
    fn super_visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow;
}

impl<'tcx> TypeVisitable<'tcx> for Ty<'tcx> {
    fn visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow {
        visitor.visit_ty(self)
    }
}

impl<'tcx> TypeVisitable<'tcx> for Const<'tcx> {
    fn visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow {
        visitor.visit_const(self)
    }
}

fn visit_generic_args<'tcx, V: TypeVisitor<'tcx>>(
    args: &crate::ty::GenericArgsRef<'tcx>,
    visitor: &mut V,
) -> ControlFlow {
    for arg in args.iter() {
        match arg {
            crate::generic::GenericArg::Type(ty) => visitor.visit_ty(*ty)?,
            crate::generic::GenericArg::Const(ct) => visitor.visit_const(*ct)?,
        }
    }
    ControlFlow::Continue(())
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
            TyKind::Adt(_, args) => visit_generic_args(args, visitor),
            TyKind::FnPtr(sig) => {
                visit_generic_args(&sig.sig.inputs, visitor)?;
                visitor.visit_ty(sig.sig.output)
            }
            TyKind::FnDef(fd) => visit_generic_args(&fd.args, visitor),
            TyKind::Tuple(args) => visit_generic_args(args, visitor),
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
            TyKind::Utility(_, args) => visit_generic_args(args, visitor),
            TyKind::Alias(alias) => visit_generic_args(&alias.args, visitor),
            TyKind::Projection(proj) => {
                visit_generic_args(&proj.trait_ref.args, visitor)
            }
            TyKind::Dynamic(binder) => {
                for pred in binder.value.iter() {
                    match pred {
                        ExistentialPredicate::Trait(tr) => {
                            visit_generic_args(&tr.args, visitor)?;
                        }
                        ExistentialPredicate::Projection(pr) => {
                            visit_generic_args(&pr.args, visitor)?;
                            visitor.visit_ty(pr.term)?;
                        }
                        ExistentialPredicate::AutoTrait(_) => {}
                    }
                }
                ControlFlow::Continue(())
            }
        }
    }
}

impl<'tcx> TypeSuperVisitable<'tcx> for Const<'tcx> {
    fn super_visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow {
        visitor.visit_ty(self.ty)?;
        match self.kind {
            ConstKind::Unevaluated(u) => visit_generic_args(&u.args, visitor),
            ConstKind::Value(_)
            | ConstKind::Param(_)
            | ConstKind::Bound(_, _)
            | ConstKind::Placeholder(_)
            | ConstKind::Infer(_)
            | ConstKind::Error => ControlFlow::Continue(()),
        }
    }
}

impl<'tcx> TypeVisitable<'tcx> for Predicate<'tcx> {
    fn visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow {
        match self {
            Predicate::Trait(p) => {
                visit_generic_args(&p.trait_ref.args, visitor)?;
            }
            Predicate::Projection(p) => {
                visit_generic_args(&p.projection_ty.trait_ref.args, visitor)?;
                visitor.visit_ty(p.term)?;
            }
            Predicate::NormalizesTo(p) => {
                visit_generic_args(&p.projection_ty.trait_ref.args, visitor)?;
                visitor.visit_ty(p.term)?;
            }
            Predicate::WellFormed(p) => {
                visitor.visit_ty(p.ty)?;
            }
            Predicate::TypeOutlives(p) => {
                visitor.visit_ty(p.ty)?;
            }
            Predicate::ConstEvaluatable(ct) => {
                visitor.visit_const(ct)?;
            }
        }
        ControlFlow::Continue(())
    }
}

impl<'tcx> TypeVisitable<'tcx> for List<Predicate<'tcx>> {
    fn visit_with<V: TypeVisitor<'tcx>>(self, visitor: &mut V) -> ControlFlow {
        for predicate in self.iter() {
            predicate.visit_with(visitor)?;
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::Interner;
    use crate::primitive::IntTy;
    use crate::ty::{AdtDef, ParamTy, TyKind};
    use crate::visit::{TypeVisitable, TypeVisitor};
    use yelang_interner::Symbol;

    struct CountTysVisitor {
        count: usize,
    }

    impl<'tcx> TypeVisitor<'tcx> for CountTysVisitor {
        fn visit_ty(&mut self, ty: Ty<'tcx>) -> ControlFlow {
            self.count += 1;
            ty.super_visit_with(self)
        }
    }

    #[test]
    fn visit_counts_nested_types() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_bool = interner.mk_ty(TyKind::Bool);
        let args = interner.mk_generic_args(&[
            crate::generic::GenericArg::Type(t_i32),
            crate::generic::GenericArg::Type(t_bool),
        ]);
        let t_adt = interner.mk_ty(TyKind::Adt(AdtDef { def_id: yelang_arena::DefId::new(1) }, args));

        let mut visitor = CountTysVisitor { count: 0 };
        assert!(t_adt.visit_with(&mut visitor).is_continue());
        // Adt + 2 args = 3
        assert_eq!(visitor.count, 3);
    }

    #[test]
    fn visit_finds_param() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(TyKind::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let args = interner.mk_generic_args(&[crate::generic::GenericArg::Type(t_param)]);
        let t_tuple = interner.mk_ty(TyKind::Tuple(args));

        struct FindParam;
        impl<'tcx> TypeVisitor<'tcx> for FindParam {
            fn visit_ty(&mut self, ty: Ty<'tcx>) -> ControlFlow {
                if matches!(ty.kind(), TyKind::Param(_)) {
                    ControlFlow::Break(())
                } else {
                    ty.super_visit_with(self)
                }
            }
        }

        let mut visitor = FindParam;
        assert!(t_tuple.visit_with(&mut visitor).is_break());
    }
}
