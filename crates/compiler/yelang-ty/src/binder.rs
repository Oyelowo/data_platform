/*! Binders, bound variables, and de Bruijn indices. */

use yelang_interner::Symbol;

/// A de Bruijn index counts binders from the inside out.
/// `INNERMOST` (0) is the binder closest to the variable occurrence.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DebruijnIndex(pub u32);

impl DebruijnIndex {
    pub const INNERMOST: DebruijnIndex = DebruijnIndex(0);

    /// Return the next outer binder index.
    pub fn shifted_in(self) -> DebruijnIndex {
        DebruijnIndex(self.0 + 1)
    }

    /// Return the next inner binder index.
    pub fn shifted_out(self) -> DebruijnIndex {
        DebruijnIndex(self.0.saturating_sub(1))
    }
}

/// A bound variable index (independent of de Bruijn level).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BoundVar(pub u32);

/// Kinds of bound variables.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BoundVariableKind {
    Ty(BoundTy),
    Const,
}

/// A bound type variable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BoundTy {
    pub var: BoundVar,
    pub kind: BoundTyKind,
}

/// The kind of a bound type variable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BoundTyKind {
    Param(Symbol),
    Anon,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debruijn_shift() {
        let d0 = DebruijnIndex::INNERMOST;
        let d1 = d0.shifted_in();
        let d2 = d1.shifted_in();
        assert_eq!(d0.0, 0);
        assert_eq!(d1.0, 1);
        assert_eq!(d2.0, 2);
        assert_eq!(d2.shifted_out().0, 1);
        assert_eq!(d0.shifted_out().0, 0); // saturating
    }

    #[test]
    fn bound_var_equality() {
        let b1 = BoundVar(0);
        let b2 = BoundVar(0);
        let b3 = BoundVar(1);
        assert_eq!(b1, b2);
        assert_ne!(b1, b3);
    }

    #[test]
    fn bound_variable_kind_variants() {
        let ty_kind = BoundVariableKind::Ty(BoundTy {
            var: BoundVar(0),
            kind: BoundTyKind::Anon,
        });
        let const_kind = BoundVariableKind::Const;
        assert_ne!(
            std::mem::discriminant(&ty_kind),
            std::mem::discriminant(&const_kind)
        );
    }
}
