/*! Generic arguments, substitutions, and generic parameter definitions. */

use yelang_interner::Symbol;
use yelang_arena::DefId;

use crate::primitive::{IntTy, UintTy};
use crate::ty::{Const, Ty};

/// A single generic argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GenericArg<'tcx> {
    Type(Ty<'tcx>),
    Const(Const<'tcx>),
}

/// Definition of generic parameters for an item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generics {
    pub params: Vec<GenericParamDef>,
    pub has_where_clause_predicates: bool,
    pub own_counts: GenericParamCount,
}

/// A single generic parameter definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenericParamDef {
    pub name: Symbol,
    pub def_id: DefId,
    pub index: u32,
    pub pure_wrt_drop: bool,
    pub kind: GenericParamDefKind,
}

/// The kind of a generic parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenericParamDefKind {
    Type { has_default: bool, synthetic: bool },
    Const { ty: ConstParamTy, has_default: bool },
}

impl Default for GenericParamDefKind {
    fn default() -> Self {
        GenericParamDefKind::Type {
            has_default: false,
            synthetic: false,
        }
    }
}

/// Types allowed for const generics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstParamTy {
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
}

/// Counts of generic parameters by kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GenericParamCount {
    pub type_params: usize,
    pub const_params: usize,
}

/// A substitution maps parameter indices to concrete arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Substitution<'tcx> {
    pub args: Vec<GenericArg<'tcx>>,
}

impl<'tcx> GenericArg<'tcx> {
    pub fn expect_type(self) -> Ty<'tcx> {
        match self {
            GenericArg::Type(ty) => ty,
            GenericArg::Const(_) => panic!("expected type, found const"),
        }
    }

    pub fn expect_const(self) -> Const<'tcx> {
        match self {
            GenericArg::Type(_) => panic!("expected const, found type"),
            GenericArg::Const(ct) => ct,
        }
    }

    pub fn is_type(self) -> bool {
        matches!(self, GenericArg::Type(_))
    }

    pub fn is_const(self) -> bool {
        matches!(self, GenericArg::Const(_))
    }
}

impl Generics {
    pub fn empty() -> Self {
        Self {
            params: Vec::new(),
            has_where_clause_predicates: false,
            own_counts: GenericParamCount::default(),
        }
    }

    pub fn count(&self) -> GenericParamCount {
        self.own_counts
    }
}

impl GenericParamCount {
    pub fn total(&self) -> usize {
        self.type_params + self.const_params
    }
}

impl<'tcx> Substitution<'tcx> {
    pub fn empty() -> Self {
        Self { args: Vec::new() }
    }

    pub fn from_args(args: Vec<GenericArg<'tcx>>) -> Self {
        Self { args }
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn get(&self, index: usize) -> Option<GenericArg<'tcx>> {
        self.args.get(index).copied()
    }

    pub fn type_at(&self, index: usize) -> Ty<'tcx> {
        self.args[index].expect_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::Interner;
    use crate::primitive::IntTy;
    use crate::ty::TyKind;

    #[test]
    fn generic_arg_type() {
        let interner = Interner::new();
        let t = interner.mk_ty(TyKind::Int(IntTy::I32));
        let arg = GenericArg::Type(t);
        assert_eq!(arg.expect_type(), t);
        assert!(arg.is_type());
        assert!(!arg.is_const());
    }

    #[test]
    fn substitution_basic() {
        let interner = Interner::new();
        let t1 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t2 = interner.mk_ty(TyKind::Bool);
        let sub = Substitution::from_args(vec![GenericArg::Type(t1), GenericArg::Type(t2)]);
        assert_eq!(sub.len(), 2);
        assert_eq!(sub.type_at(0), t1);
        assert_eq!(sub.type_at(1), t2);
    }

    #[test]
    fn generic_param_count() {
        let count = GenericParamCount {
            type_params: 2,
            const_params: 1,
        };
        assert_eq!(count.total(), 3);
    }
}
