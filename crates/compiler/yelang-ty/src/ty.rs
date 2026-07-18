/*! Core type representation.
 *
 * `Ty` is the recursive enum of all type constructors. It is interned: the
 * `Interner` stores `Ty` values in a dense `IndexVec<TyId, Ty>` table and
 * hash-conses them so that structurally equal types share the same `TyId`.
 * Equality of types is therefore `TyId` equality.
 */

use std::fmt;
use std::hash::Hash;

use yelang_interner::Symbol;

use crate::binder::{BoundTy, BoundVar, BoundVariableKind, DebruijnIndex};
use crate::generic::GenericArg;
use crate::list::List;
use crate::predicate::TraitRef;
use crate::primitive::{FloatTy, IntTy, UintTy};

// Re-export the interned IDs so callers can import everything from `yelang_ty::ty`.
pub use yelang_arena::{ConstId, DefId, TyId};

// ---------------------------------------------------------------------------
// Ty
// ---------------------------------------------------------------------------

/// All type constructors in Yelang.
///
/// # Invariant
/// Every `TyId` and `List<T>` contained in a `Ty` must itself be interned.
/// This is enforced because `TyId`/`ConstId` are only constructible through
/// `Interner::mk_ty` / `Interner::mk_const`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ty {
    /// `bool`
    Bool,
    /// `char`
    Char,
    /// `str` (string slice).
    Str,
    /// `i8`, `i16`, `i32`, `i64`, `i128`, `isize`
    Int(IntTy),
    /// `u8`, `u16`, `u32`, `u64`, `u128`, `usize`
    Uint(UintTy),
    /// `f32`, `f64`
    Float(FloatTy),
    /// A type parameter `T`.
    Param(ParamTy),
    /// A bound type variable, e.g. the `T` in `for<T> fn(T) -> T`.
    Bound(DebruijnIndex, BoundTy),
    /// An inference variable.
    Infer(InferTy),
    /// A struct, enum, or union type.
    Adt(AdtDef, GenericArgsRef),
    /// A function pointer: `fn(i32) -> i32`.
    FnPtr(PolyFnSig),
    /// A function item (monomorphic reference to a specific function).
    FnDef(FnDef),
    /// A tuple type: `(i32, bool)` or `()`.
    Tuple(GenericArgsRef),
    /// An array type: `[T; N]`.
    Array(TyId, ConstId),
    /// A slice type: `[T]`.
    Slice(TyId),
    /// A raw pointer: `*mut T` or `*const T`.
    RawPtr(TypeAndMut),
    /// A reference: `&T` or `&mut T` (no lifetime in Yelang).
    Ref(TyId, Mutability),
    /// The never type `!`.
    Never,
    /// An anonymous struct type: `{ x: i32, y: i32 }`.
    AnonStruct(AnonStructDef),
    /// A union/sum type: `A | B`.
    Union(TyId, TyId),
    /// A type literal: `"pending"`.
    TypeLit(Symbol),
    /// A utility type: `Omit<T, K>`, `Pick<T, K>`, etc.
    Utility(UtilityKind, GenericArgsRef),
    /// An opaque type (`impl Trait`) or type alias expansion.
    Alias(AliasTy),
    /// An associated type projection: `<T as Trait>::Assoc`.
    Projection(ProjectionTy),
    /// A trait object: `dyn Trait`.
    Dynamic(Binder<List<ExistentialPredicate>>),
    /// A placeholder type, used during canonicalization.
    Placeholder(PlaceholderType),
    /// Error recovery type.
    Error,
}

impl Ty {
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Ty::Bool
                | Ty::Char
                | Ty::Str
                | Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(_)
                | Ty::Never
        )
    }

    pub fn is_never(&self) -> bool {
        matches!(self, Ty::Never)
    }

    pub fn is_unit(&self) -> bool {
        matches!(self, Ty::Tuple(args) if args.is_empty())
    }

    pub fn is_fn_ptr(&self) -> bool {
        matches!(self, Ty::FnPtr(_))
    }
}

// ---------------------------------------------------------------------------
// Const
// ---------------------------------------------------------------------------

/// Kinds of type-level constants.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Const {
    /// A literal constant: `42`.
    Value(ConstValue),
    /// A const parameter: `N` in `fn foo<const N: usize>() `.
    Param(ParamConst),
    /// A bound const variable.
    Bound(DebruijnIndex, BoundVar),
    /// A placeholder const.
    Placeholder(PlaceholderConst),
    /// An unevaluated const expression.
    Unevaluated(UnevaluatedConst),
    /// An inference const variable.
    Infer(ConstVid),
    /// Error recovery.
    Error,
}

/// The data stored in the interner for each `Const`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstData {
    pub kind: Const,
    pub ty: TyId,
}

// ---------------------------------------------------------------------------
// Sub-structures
// ---------------------------------------------------------------------------

/// Generic arguments reference: interned list of `GenericArg`.
pub type GenericArgsRef = List<GenericArg>;

/// A binder for higher-ranked types: `for<T> T`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binder<T> {
    pub bound_vars: List<BoundVariableKind>,
    pub value: T,
}

/// A type parameter like `T` in `fn foo<T>() {}`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ParamTy {
    pub index: u32,
    pub name: Symbol,
}

/// Inference variables.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum InferTy {
    /// A general type variable: `?T`.
    TyVar(TyVid),
    /// An integral type variable (from integer literals like `42`).
    IntVar(IntVid),
    /// A floating-point type variable (from float literals like `3.14`).
    FloatVar(FloatVid),
}

/// General type variable ID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TyVid(pub u32);

/// Integral type variable ID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct IntVid(pub u32);

/// Floating-point type variable ID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FloatVid(pub u32);

/// Const variable ID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ConstVid(pub u32);

/// An ADT definition reference.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AdtDef {
    pub def_id: DefId,
}

/// A function definition reference.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnDef {
    pub def_id: DefId,
    pub args: GenericArgsRef,
}

/// A polymorphic function signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PolyFnSig {
    pub sig: FnSig,
}

/// A function signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnSig {
    pub inputs: GenericArgsRef,
    pub output: TyId,
    /// True when the return type was written as `_` and should be inferred from
    /// the function body.
    pub return_ty_infer: bool,
}

/// Mutability.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Mutability {
    Mut,
    Not,
}

/// Type + mutability (for raw pointers).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeAndMut {
    pub ty: TyId,
    pub mutbl: Mutability,
}

/// Anonymous struct definition.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnonStructDef {
    pub fields: List<AnonField>,
}

/// A field in an anonymous struct.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnonField {
    pub name: Symbol,
    pub ty: TyId,
}

/// Utility type kinds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum UtilityKind {
    Omit,
    Pick,
    ReturnType,
    Parameters,
    Partial,
    Required,
}

/// An opaque type (`impl Trait`) or a type alias expansion.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AliasTy {
    pub def_id: DefId,
    pub args: GenericArgsRef,
}

/// An associated type projection: `<T as Trait>::Assoc`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectionTy {
    pub trait_ref: TraitRef,
    pub item_def_id: DefId,
}

/// An existential predicate for `dyn Trait`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExistentialPredicate {
    Trait(ExistentialTraitRef),
    Projection(ExistentialProjection),
    AutoTrait(DefId),
}

/// An existential trait reference for `dyn Trait`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExistentialTraitRef {
    pub def_id: DefId,
    pub args: GenericArgsRef,
}

/// An existential projection for `dyn Trait`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExistentialProjection {
    pub def_id: DefId,
    pub args: GenericArgsRef,
    pub term: TyId,
}

/// A placeholder type, used during canonicalization.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlaceholderType {
    pub universe: UniverseIndex,
    pub name: Symbol,
}

/// Universe index for placeholder types.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct UniverseIndex(pub u32);

/// A const parameter like `N` in `fn foo<const N: usize>() {}`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ParamConst {
    pub index: u32,
    pub name: Symbol,
}

/// A concrete constant value.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ConstValue {
    Bool(bool),
    Int(i128),
    Uint(u128),
    Float(f64),
    Str(Symbol),
}

impl Eq for ConstValue {}

impl std::hash::Hash for ConstValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ConstValue::Bool(b) => b.hash(state),
            ConstValue::Int(i) => i.hash(state),
            ConstValue::Uint(u) => u.hash(state),
            ConstValue::Float(f) => f.to_bits().hash(state),
            ConstValue::Str(s) => s.hash(state),
        }
    }
}

/// A placeholder const.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlaceholderConst {
    pub universe: UniverseIndex,
    pub name: Symbol,
}

/// An unevaluated constant.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnevaluatedConst {
    pub def: DefId,
    pub args: GenericArgsRef,
}

// ---------------------------------------------------------------------------
// ImplPolarity
// ---------------------------------------------------------------------------

/// Polarity of an impl or trait bound.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ImplPolarity {
    Positive,
    Negative,
}

// ---------------------------------------------------------------------------
// Debug impls
// ---------------------------------------------------------------------------

impl fmt::Debug for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Bool => write!(f, "bool"),
            Ty::Char => write!(f, "char"),
            Ty::Str => write!(f, "str"),
            Ty::Int(it) => write!(f, "{}", it.name_str()),
            Ty::Uint(ut) => write!(f, "{}", ut.name_str()),
            Ty::Float(ft) => write!(f, "{}", ft.name_str()),
            Ty::Param(p) => write!(f, "{}", p.name.as_usize()),
            Ty::Bound(debruijn, bt) => {
                write!(f, "Bound({:?}, {:?})", debruijn, bt)
            }
            Ty::Infer(iv) => write!(f, "{:?}", iv),
            Ty::Adt(adt, _) => write!(f, "Adt({:?})", adt.def_id),
            Ty::FnPtr(_) => write!(f, "fn_ptr"),
            Ty::FnDef(fd) => write!(f, "fn_def({:?})", fd.def_id),
            Ty::Tuple(args) => {
                let mut t = f.debug_tuple("");
                for arg in args.iter() {
                    t.field(arg);
                }
                t.finish()
            }
            Ty::Array(ty, _) => write!(f, "[{:?}; _]", ty),
            Ty::Slice(ty) => write!(f, "[{:?}]", ty),
            Ty::RawPtr(tam) => {
                let mut_str = match tam.mutbl {
                    Mutability::Mut => "mut ",
                    Mutability::Not => "const ",
                };
                write!(f, "*{} {:?}", mut_str, tam.ty)
            }
            Ty::Ref(ty, mutbl) => {
                let mut_str = match mutbl {
                    Mutability::Mut => "mut ",
                    Mutability::Not => "",
                };
                write!(f, "&{}{:?}", mut_str, ty)
            }
            Ty::Never => write!(f, "!"),
            Ty::AnonStruct(_) => write!(f, "anon_struct"),
            Ty::Union(a, b) => write!(f, "{:?} | {:?}", a, b),
            Ty::TypeLit(sym) => write!(f, "type_lit({})", sym.as_usize()),
            Ty::Utility(k, _) => write!(f, "Utility({:?})", k),
            Ty::Alias(_) => write!(f, "alias"),
            Ty::Projection(_) => write!(f, "projection"),
            Ty::Dynamic(_) => write!(f, "dyn _"),
            Ty::Placeholder(p) => write!(f, "Placeholder({:?})", p),
            Ty::Error => write!(f, "{{error}}"),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Binder<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "for<{:?}> {:?}", self.bound_vars, self.value)
    }
}

impl fmt::Debug for AnonStructDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("AnonStruct");
        for field in self.fields.iter() {
            d.field(&format!("{:?}", field.name.as_usize()), &field.ty);
        }
        d.finish()
    }
}

impl fmt::Debug for AnonField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {:?}", self.name.as_usize(), self.ty)
    }
}

impl fmt::Debug for AliasTy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Alias({:?})", self.def_id)
    }
}

impl fmt::Debug for ProjectionTy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Projection({:?}, {:?})", self.trait_ref.def_id, self.item_def_id)
    }
}

impl fmt::Debug for ExistentialPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExistentialPredicate::Trait(t) => write!(f, "Trait({:?})", t.def_id),
            ExistentialPredicate::Projection(p) => write!(f, "Projection({:?})", p.def_id),
            ExistentialPredicate::AutoTrait(d) => write!(f, "AutoTrait({:?})", d),
        }
    }
}

impl fmt::Debug for TypeAndMut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {:?}", self.mutbl, self.ty)
    }
}

impl fmt::Debug for ConstData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {:?}", self.kind, self.ty)
    }
}

impl fmt::Debug for Const {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Const::Value(v) => write!(f, "{:?}", v),
            Const::Param(p) => write!(f, "Param({:?})", p),
            Const::Bound(d, bc) => write!(f, "Bound({:?}, {:?})", d, bc),
            Const::Placeholder(p) => write!(f, "Placeholder({:?})", p),
            Const::Unevaluated(u) => write!(f, "Unevaluated({:?})", u.def),
            Const::Infer(v) => write!(f, "Infer({:?})", v),
            Const::Error => write!(f, "{{error}}"),
        }
    }
}

impl fmt::Debug for UnevaluatedConst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unevaluated({:?})", self.def)
    }
}

impl fmt::Debug for FnDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FnDef({:?})", self.def_id)
    }
}

impl fmt::Debug for PolyFnSig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.sig)
    }
}

impl fmt::Debug for FnSig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fn(")?;
        for (i, arg) in self.inputs.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{:?}", arg)?;
        }
        write!(f, ") -> {:?}", self.output)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::Interner;

    #[test]
    fn ty_id_equality() {
        let interner = Interner::new();
        let t1 = interner.mk_ty(Ty::Bool);
        let t2 = interner.mk_ty(Ty::Bool);
        assert_eq!(t1, t2);
    }

    #[test]
    fn ty_is_primitive() {
        let interner = Interner::new();
        let t_bool = interner.mk_ty(Ty::Bool);
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_tuple = interner.mk_ty(Ty::Tuple(List::empty()));

        assert!(interner.ty(t_bool).is_primitive());
        assert!(interner.ty(t_i32).is_primitive());
        assert!(!interner.ty(t_tuple).is_primitive());
    }

    #[test]
    fn unit_type_detection() {
        let interner = Interner::new();
        let unit = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[])));
        let pair = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
            crate::generic::GenericArg::Type(interner.mk_ty(Ty::Int(IntTy::I32))),
            crate::generic::GenericArg::Type(interner.mk_ty(Ty::Bool)),
        ])));

        assert!(interner.ty(unit).is_unit());
        assert!(!interner.ty(pair).is_unit());
    }

    #[test]
    fn never_type() {
        let interner = Interner::new();
        let never = interner.mk_ty(Ty::Never);
        assert!(interner.ty(never).is_never());
        assert!(interner.ty(never).is_primitive());
    }

    #[test]
    fn infer_ty_variants() {
        let v1 = InferTy::TyVar(TyVid(0));
        let v2 = InferTy::IntVar(IntVid(1));
        let v3 = InferTy::FloatVar(FloatVid(2));
        assert_ne!(v1, v2);
        assert_ne!(v2, v3);
    }

    #[test]
    fn complex_adt_ty() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_bool = interner.mk_ty(Ty::Bool);
        let args = interner.mk_generic_args(&[
            crate::generic::GenericArg::Type(t_i32),
            crate::generic::GenericArg::Type(t_bool),
        ]);
        let adt = Ty::Adt(
            AdtDef {
                def_id: DefId::new(1),
            },
            args,
        );
        let ty = interner.mk_ty(adt);

        match interner.ty(ty) {
            Ty::Adt(def, a) => {
                assert_eq!(def.def_id.raw(), 1);
                assert_eq!(a.len(), 2);
            }
            _ => panic!("expected Adt"),
        }
    }
}
