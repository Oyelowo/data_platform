/*! Core type representation.
 *
 * `Ty<'tcx>` is a pointer to an interned `TyKind<'tcx>`. It is `Copy` and
 * equality is a single pointer comparison.
 */

use std::fmt;
use std::hash::{Hash, Hasher};

use yelang_interner::Symbol;
use yelang_util::DefId;

use crate::binder::{BoundTy, BoundVar, BoundVariableKind, DebruijnIndex};
use crate::generic::GenericArg;
use crate::list::List;
use crate::primitive::{FloatTy, IntTy, UintTy};

// ---------------------------------------------------------------------------
// Ty
// ---------------------------------------------------------------------------

/// A canonical, interned type.
///
/// `Ty<'tcx>` is a thin wrapper around `&'tcx TyKind<'tcx>`. It is `Copy`
/// and two `Ty`s are equal iff they point to the same interned allocation.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Ty<'tcx> {
    ptr: &'tcx TyKind<'tcx>,
    _marker: std::marker::PhantomData<&'tcx ()>,
}

impl<'tcx> Ty<'tcx> {
    /// Construct a `Ty` from a raw pointer.
    ///
    /// # Safety
    /// The pointer must be to an arena-allocated, interned `TyKind`.
    pub(crate) const fn from_ptr(ptr: &'tcx TyKind<'tcx>) -> Self {
        Self {
            ptr,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn kind(self) -> &'tcx TyKind<'tcx> {
        self.ptr
    }

    pub fn as_ptr(self) -> *const TyKind<'tcx> {
        self.ptr as *const _
    }

    pub fn is_primitive(self) -> bool {
        matches!(
            self.ptr,
            TyKind::Bool
                | TyKind::Char
                | TyKind::Str
                | TyKind::Int(_)
                | TyKind::Uint(_)
                | TyKind::Float(_)
                | TyKind::Never
        )
    }

    pub fn is_never(self) -> bool {
        matches!(self.ptr, TyKind::Never)
    }

    pub fn is_unit(self) -> bool {
        matches!(self.ptr, TyKind::Tuple(args) if args.is_empty())
    }

    pub fn is_fn_ptr(self) -> bool {
        matches!(self.ptr, TyKind::FnPtr(_))
    }
}

impl<'tcx> PartialEq for Ty<'tcx> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.ptr, other.ptr)
    }
}

impl<'tcx> Eq for Ty<'tcx> {}

impl<'tcx> Hash for Ty<'tcx> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.ptr as *const TyKind<'tcx>).hash(state);
    }
}

impl<'tcx> fmt::Debug for Ty<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.ptr.fmt(f)
    }
}

// ---------------------------------------------------------------------------
// TyKind
// ---------------------------------------------------------------------------

/// All type constructors in Yelang.
///
/// # Invariant
/// Every `Ty<'tcx>` and `List<T>` contained in a `TyKind` must itself be
/// interned. This is enforced because `Ty` is only constructible through
/// `Interner::mk_ty`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum TyKind<'tcx> {
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
    Adt(AdtDef, GenericArgsRef<'tcx>),
    /// A function pointer: `fn(i32) -> i32`.
    FnPtr(PolyFnSig<'tcx>),
    /// A function item (monomorphic reference to a specific function).
    FnDef(FnDef<'tcx>),
    /// A tuple type: `(i32, bool)` or `()`.
    Tuple(GenericArgsRef<'tcx>),
    /// An array type: `[T; N]`.
    Array(Ty<'tcx>, Const<'tcx>),
    /// A slice type: `[T]`.
    Slice(Ty<'tcx>),
    /// A raw pointer: `*mut T` or `*const T`.
    RawPtr(TypeAndMut<'tcx>),
    /// A reference: `&T` or `&mut T` (no lifetime in Yelang).
    Ref(Ty<'tcx>, Mutability),
    /// The never type `!`.
    Never,
    /// An anonymous struct type: `{ x: i32, y: i32 }`.
    AnonStruct(AnonStructDef<'tcx>),
    /// A union/sum type: `A | B`.
    Union(Ty<'tcx>, Ty<'tcx>),
    /// A type literal: `"pending"`.
    TypeLit(Symbol),
    /// A utility type: `Omit<T, K>`, `Pick<T, K>`, etc.
    Utility(UtilityKind, GenericArgsRef<'tcx>),
    /// An opaque type (`impl Trait`) or associated type projection.
    Alias(AliasTy<'tcx>),
    /// A trait object: `dyn Trait`.
    Dynamic(Binder<'tcx, ExistentialPredicate<'tcx>>),
    /// A placeholder type, used during canonicalization.
    Placeholder(PlaceholderType),
    /// Error recovery type.
    Error,
}

// ---------------------------------------------------------------------------
// Sub-structures
// ---------------------------------------------------------------------------

/// Generic arguments reference: interned list of `GenericArg`.
pub type GenericArgsRef<'tcx> = List<GenericArg<'tcx>>;

/// A binder for higher-ranked types: `for<T> T`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binder<'tcx, T> {
    pub bound_vars: List<BoundVariableKind>,
    pub value: T,
    pub _marker: std::marker::PhantomData<&'tcx ()>,
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
pub struct FnDef<'tcx> {
    pub def_id: DefId,
    pub args: GenericArgsRef<'tcx>,
}

/// A polymorphic function signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PolyFnSig<'tcx> {
    pub sig: FnSig<'tcx>,
}

/// A function signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnSig<'tcx> {
    pub inputs: GenericArgsRef<'tcx>,
    pub output: Ty<'tcx>,
}

/// Mutability.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Mutability {
    Mut,
    Not,
}

/// Type + mutability (for raw pointers).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeAndMut<'tcx> {
    pub ty: Ty<'tcx>,
    pub mutbl: Mutability,
}

/// Anonymous struct definition.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnonStructDef<'tcx> {
    pub fields: List<AnonField<'tcx>>,
}

/// A field in an anonymous struct.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnonField<'tcx> {
    pub name: Symbol,
    pub ty: Ty<'tcx>,
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

/// An alias / projection type (`impl Trait` or associated type).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AliasTy<'tcx> {
    pub def_id: DefId,
    pub args: GenericArgsRef<'tcx>,
}

/// An existential predicate for `dyn Trait`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExistentialPredicate<'tcx> {
    Trait(ExistentialTraitRef<'tcx>),
    Projection(ExistentialProjection<'tcx>),
    AutoTrait(DefId),
}

/// An existential trait reference for `dyn Trait`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExistentialTraitRef<'tcx> {
    pub def_id: DefId,
    pub args: GenericArgsRef<'tcx>,
}

/// An existential projection for `dyn Trait`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExistentialProjection<'tcx> {
    pub def_id: DefId,
    pub args: GenericArgsRef<'tcx>,
    pub term: Ty<'tcx>,
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

/// A type-level constant.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Const<'tcx> {
    pub kind: ConstKind<'tcx>,
    pub ty: Ty<'tcx>,
}

/// Kinds of type-level constants.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstKind<'tcx> {
    /// A literal constant: `42`.
    Value(ConstValue),
    /// A bound const variable.
    Bound(DebruijnIndex, BoundVar),
    /// A placeholder const.
    Placeholder(PlaceholderConst),
    /// An unevaluated const expression.
    Unevaluated(UnevaluatedConst<'tcx>),
    /// An inference const variable.
    Infer(ConstVid),
    /// Error recovery.
    Error,
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
pub struct UnevaluatedConst<'tcx> {
    pub def: DefId,
    pub args: GenericArgsRef<'tcx>,
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

impl<'tcx> fmt::Debug for TyKind<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TyKind::Bool => write!(f, "bool"),
            TyKind::Char => write!(f, "char"),
            TyKind::Str => write!(f, "str"),
            TyKind::Int(it) => write!(f, "{}", it.name_str()),
            TyKind::Uint(ut) => write!(f, "{}", ut.name_str()),
            TyKind::Float(ft) => write!(f, "{}", ft.name_str()),
            TyKind::Param(p) => write!(f, "{}", p.name.as_usize()),
            TyKind::Bound(debruijn, bt) => {
                write!(f, "Bound({:?}, {:?})", debruijn, bt)
            }
            TyKind::Infer(iv) => write!(f, "{:?}", iv),
            TyKind::Adt(adt, _) => write!(f, "Adt({:?})", adt.def_id),
            TyKind::FnPtr(_) => write!(f, "fn_ptr"),
            TyKind::FnDef(fd) => write!(f, "fn_def({:?})", fd.def_id),
            TyKind::Tuple(args) => {
                let mut t = f.debug_tuple("");
                for arg in args.iter() {
                    t.field(arg);
                }
                t.finish()
            }
            TyKind::Array(ty, _) => write!(f, "[{:?}; _]", ty),
            TyKind::Slice(ty) => write!(f, "[{:?}]", ty),
            TyKind::RawPtr(tam) => {
                let mut_str = match tam.mutbl {
                    Mutability::Mut => "mut ",
                    Mutability::Not => "const ",
                };
                write!(f, "*{} {:?}", mut_str, tam.ty)
            }
            TyKind::Ref(ty, mutbl) => {
                let mut_str = match mutbl {
                    Mutability::Mut => "mut ",
                    Mutability::Not => "",
                };
                write!(f, "&{}{:?}", mut_str, ty)
            }
            TyKind::Never => write!(f, "!"),
            TyKind::AnonStruct(_) => write!(f, "anon_struct"),
            TyKind::Union(a, b) => write!(f, "{:?} | {:?}", a, b),
            TyKind::TypeLit(sym) => write!(f, "type_lit({})", sym.as_usize()),
            TyKind::Utility(k, _) => write!(f, "Utility({:?})", k),
            TyKind::Alias(_) => write!(f, "alias"),
            TyKind::Dynamic(_) => write!(f, "dyn _"),
            TyKind::Placeholder(p) => write!(f, "Placeholder({:?})", p),
            TyKind::Error => write!(f, "{{error}}"),
        }
    }
}

impl<'tcx, T: fmt::Debug> fmt::Debug for Binder<'tcx, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "for<{:?}> {:?}", self.bound_vars, self.value)
    }
}

impl<'tcx> fmt::Debug for AnonStructDef<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("AnonStruct");
        for field in self.fields.iter() {
            d.field(&format!("{:?}", field.name.as_usize()), &field.ty);
        }
        d.finish()
    }
}

impl<'tcx> fmt::Debug for AnonField<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {:?}", self.name.as_usize(), self.ty)
    }
}

impl<'tcx> fmt::Debug for AliasTy<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Alias({:?})", self.def_id)
    }
}

impl<'tcx> fmt::Debug for ExistentialPredicate<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExistentialPredicate::Trait(t) => write!(f, "Trait({:?})", t.def_id),
            ExistentialPredicate::Projection(p) => write!(f, "Projection({:?})", p.def_id),
            ExistentialPredicate::AutoTrait(d) => write!(f, "AutoTrait({:?})", d),
        }
    }
}

impl<'tcx> fmt::Debug for TypeAndMut<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {:?}", self.mutbl, self.ty)
    }
}

impl<'tcx> fmt::Debug for Const<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {:?}", self.kind, self.ty)
    }
}

impl<'tcx> fmt::Debug for ConstKind<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstKind::Value(v) => write!(f, "{:?}", v),
            ConstKind::Bound(d, bc) => write!(f, "Bound({:?}, {:?})", d, bc),
            ConstKind::Placeholder(p) => write!(f, "Placeholder({:?})", p),
            ConstKind::Unevaluated(u) => write!(f, "Unevaluated({:?})", u.def),
            ConstKind::Infer(v) => write!(f, "Infer({:?})", v),
            ConstKind::Error => write!(f, "{{error}}"),
        }
    }
}

impl<'tcx> fmt::Debug for UnevaluatedConst<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unevaluated({:?})", self.def)
    }
}

impl<'tcx> fmt::Debug for FnDef<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FnDef({:?})", self.def_id)
    }
}

impl<'tcx> fmt::Debug for PolyFnSig<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.sig)
    }
}

impl<'tcx> fmt::Debug for FnSig<'tcx> {
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
    fn ty_pointer_equality() {
        let interner = Interner::new();
        let t1 = interner.mk_ty(TyKind::Bool);
        let t2 = interner.mk_ty(TyKind::Bool);
        assert_eq!(t1, t2);
        assert_eq!(t1.as_ptr(), t2.as_ptr());
    }

    #[test]
    fn ty_kind_is_primitive() {
        let interner = Interner::new();
        let t_bool = interner.mk_ty(TyKind::Bool);
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_tuple = interner.mk_ty(TyKind::Tuple(List::empty()));

        assert!(t_bool.is_primitive());
        assert!(t_i32.is_primitive());
        assert!(!t_tuple.is_primitive());
    }

    #[test]
    fn unit_type_detection() {
        let interner = Interner::new();
        let unit = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[])));
        let pair = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
            crate::generic::GenericArg::Type(interner.mk_ty(TyKind::Int(IntTy::I32))),
            crate::generic::GenericArg::Type(interner.mk_ty(TyKind::Bool)),
        ])));

        assert!(unit.is_unit());
        assert!(!pair.is_unit());
    }

    #[test]
    fn never_type() {
        let interner = Interner::new();
        let never = interner.mk_ty(TyKind::Never);
        assert!(never.is_never());
        assert!(never.is_primitive());
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
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_bool = interner.mk_ty(TyKind::Bool);
        let args = interner.mk_generic_args(&[
            crate::generic::GenericArg::Type(t_i32),
            crate::generic::GenericArg::Type(t_bool),
        ]);
        let adt = TyKind::Adt(AdtDef { def_id: DefId::new(1) }, args);
        let ty = interner.mk_ty(adt);

        match ty.kind() {
            TyKind::Adt(def, a) => {
                assert_eq!(def.def_id.raw(), 1);
                assert_eq!(a.len(), 2);
            }
            _ => panic!("expected Adt"),
        }
    }
}
