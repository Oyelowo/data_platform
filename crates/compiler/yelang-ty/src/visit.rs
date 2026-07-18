/*! Structural visiting over types.
 *
 * `TypeVisitor` allows inspecting types without modifying them.
 */

use crate::existential::ExistentialPredicate;
use crate::interner::Interner;
use crate::list::List;
use crate::predicate::Predicate;
use crate::ty::{Const, ConstId, Ty, TyId};

/// Control flow for visitation.
pub type ControlFlow = std::ops::ControlFlow<()>;

/// A visitor that inspects types.
pub trait TypeVisitor: Sized {
    fn interner(&self) -> &Interner;

    fn visit_ty(&mut self, ty: TyId) -> ControlFlow {
        ty.super_visit_with(self)
    }

    fn visit_const(&mut self, _ct: ConstId) -> ControlFlow {
        ControlFlow::Continue(())
    }
}

/// Types that can be structurally visited.
pub trait TypeVisitable: Sized {
    fn visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow;
}

/// Super-visit: the default structural traversal.
pub trait TypeSuperVisitable {
    fn super_visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow;
}

impl TypeVisitable for TyId {
    fn visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow {
        visitor.visit_ty(self)
    }
}

impl TypeVisitable for ConstId {
    fn visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow {
        visitor.visit_const(self)
    }
}

fn visit_generic_args<V: TypeVisitor>(
    args: &crate::ty::GenericArgsRef,
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

impl TypeSuperVisitable for TyId {
    fn super_visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow {
        let kind = visitor.interner().ty(self);
        match kind {
            Ty::Bool
            | Ty::Char
            | Ty::Str
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Param(_)
            | Ty::Bound(_, _)
            | Ty::Infer(_)
            | Ty::Never
            | Ty::TypeLit(_)
            | Ty::Placeholder(_)
            | Ty::Error => ControlFlow::Continue(()),
            Ty::Adt(_, args) => visit_generic_args(&args, visitor),
            Ty::FnPtr(sig) => {
                visit_generic_args(&sig.sig.inputs, visitor)?;
                visitor.visit_ty(sig.sig.output)
            }
            Ty::FnDef(fd) => visit_generic_args(&fd.args, visitor),
            Ty::Tuple(args) => visit_generic_args(&args, visitor),
            Ty::Array(ty, ct) => {
                visitor.visit_ty(ty)?;
                visitor.visit_const(ct)
            }
            Ty::Slice(ty) => visitor.visit_ty(ty),
            Ty::RawPtr(tam) => visitor.visit_ty(tam.ty),
            Ty::Ref(ty, _) => visitor.visit_ty(ty),
            Ty::AnonStruct(anon) => {
                for f in anon.fields.iter() {
                    visitor.visit_ty(f.ty)?;
                }
                ControlFlow::Continue(())
            }
            Ty::Union(a, b) => {
                visitor.visit_ty(a)?;
                visitor.visit_ty(b)
            }
            Ty::Utility(_, args) => visit_generic_args(&args, visitor),
            Ty::Alias(alias) => visit_generic_args(&alias.args, visitor),
            Ty::Projection(proj) => visit_generic_args(&proj.trait_ref.args, visitor),
            Ty::Dynamic(binder) => {
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

impl TypeSuperVisitable for ConstId {
    fn super_visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow {
        let interner = visitor.interner();
        let kind = interner.const_kind(self);
        visitor.visit_ty(interner.const_ty(self))?;
        match kind {
            Const::Unevaluated(u) => visit_generic_args(&u.args, visitor),
            Const::Value(_)
            | Const::Param(_)
            | Const::Bound(_, _)
            | Const::Placeholder(_)
            | Const::Infer(_)
            | Const::Error => ControlFlow::Continue(()),
        }
    }
}

impl TypeVisitable for Predicate {
    fn visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow {
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

impl TypeVisitable for List<Predicate> {
    fn visit_with<V: TypeVisitor>(self, visitor: &mut V) -> ControlFlow {
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
    use crate::ty::{AdtDef, ParamTy, Ty};
    use yelang_interner::Symbol;

    struct CountTysVisitor<'a> {
        interner: &'a Interner,
        count: usize,
    }

    impl<'a> TypeVisitor for CountTysVisitor<'a> {
        fn interner(&self) -> &Interner {
            self.interner
        }

        fn visit_ty(&mut self, ty: TyId) -> ControlFlow {
            self.count += 1;
            ty.super_visit_with(self)
        }
    }

    #[test]
    fn visit_counts_nested_types() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_bool = interner.mk_ty(Ty::Bool);
        let args = interner.mk_generic_args(&[
            crate::generic::GenericArg::Type(t_i32),
            crate::generic::GenericArg::Type(t_bool),
        ]);
        let t_adt = interner.mk_ty(Ty::Adt(
            AdtDef {
                def_id: yelang_arena::DefId::new(1),
            },
            args,
        ));

        let mut visitor = CountTysVisitor {
            interner: &interner,
            count: 0,
        };
        assert!(t_adt.visit_with(&mut visitor).is_continue());
        // Adt + 2 args = 3
        assert_eq!(visitor.count, 3);
    }

    #[test]
    fn visit_finds_param() {
        let interner = Interner::new();
        let t_param = interner.mk_ty(Ty::Param(ParamTy {
            index: 0,
            name: Symbol::from(1),
        }));
        let args = interner.mk_generic_args(&[crate::generic::GenericArg::Type(t_param)]);
        let t_tuple = interner.mk_ty(Ty::Tuple(args));

        struct FindParam<'a> {
            interner: &'a Interner,
        }
        impl<'a> TypeVisitor for FindParam<'a> {
            fn interner(&self) -> &Interner {
                self.interner
            }

            fn visit_ty(&mut self, ty: TyId) -> ControlFlow {
                if matches!(self.interner.ty(ty), Ty::Param(_)) {
                    ControlFlow::Break(())
                } else {
                    ty.super_visit_with(self)
                }
            }
        }

        let mut visitor = FindParam {
            interner: &interner,
        };
        assert!(t_tuple.visit_with(&mut visitor).is_break());
    }

    #[test]
    fn visit_counts_projection_types() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let trait_ref = crate::predicate::TraitRef {
            def_id: yelang_arena::DefId::new(1),
            args: interner.mk_generic_args(&[crate::generic::GenericArg::Type(t_i32)]),
        };
        let projection = interner.mk_ty(Ty::Projection(crate::ty::ProjectionTy {
            trait_ref,
            item_def_id: yelang_arena::DefId::new(2),
        }));

        let mut visitor = CountTysVisitor {
            interner: &interner,
            count: 0,
        };
        assert!(projection.visit_with(&mut visitor).is_continue());
        // Projection + i32 arg = 2
        assert_eq!(visitor.count, 2);
    }

    #[test]
    fn visit_counts_dynamic_predicates() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let existential = crate::ty::ExistentialPredicate::Trait(crate::ty::ExistentialTraitRef {
            def_id: yelang_arena::DefId::new(1),
            args: interner.mk_generic_args(&[crate::generic::GenericArg::Type(t_i32)]),
        });
        let dynamic = interner.mk_ty(Ty::Dynamic(crate::ty::Binder {
            bound_vars: interner.mk_bound_var_list(&[]),
            value: interner.mk_existential_predicates(&[existential]),
        }));

        let mut visitor = CountTysVisitor {
            interner: &interner,
            count: 0,
        };
        assert!(dynamic.visit_with(&mut visitor).is_continue());
        // Dynamic + i32 arg = 2
        assert_eq!(visitor.count, 2);
    }
}
