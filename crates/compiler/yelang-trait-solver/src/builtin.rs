/*! Built-in trait impls.
 *
 * Traits like `Sized`, `Copy`, `Clone` have built-in rules that the
 * solver knows about without requiring user-written impls.
 *
 * These rules are intentionally conservative: an ADT is never considered
 * `Copy` or `Clone` by the built-in solver; it must have a user impl.
 * `Sized` is treated as built-in for ADTs because an unsized ADT would be
 * reported by well-formedness checks elsewhere.
 */

use yelang_ty::interner::Interner;
use yelang_ty::ty::{Mutability, Ty, TyId};

/// Check if a type is `Sized` according to built-in rules.
pub fn is_sized(ty: TyId, interner: &Interner) -> bool {
    match interner.ty(ty) {
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Param(_)
        | Ty::Infer(_)
        | Ty::FnPtr(_)
        | Ty::FnDef(_)
        | Ty::RawPtr(_)
        | Ty::Ref(_, _)
        | Ty::Never
        | Ty::Adt(_, _) => true,
        Ty::Tuple(args) => args.iter().all(|arg| match arg {
            yelang_ty::generic::GenericArg::Type(t) => is_sized(*t, interner),
            _ => true,
        }),
        Ty::Array(ty, _) => is_sized(ty, interner),
        Ty::Slice(_) => false,   // Dynamically sized
        Ty::Dynamic(_) => false, // Dynamically sized
        Ty::Error => true,
        Ty::Alias(_) => true,      // TODO: check alias expansion
        Ty::Projection(_) => true, // TODO: check projection normalization
        Ty::AnonStruct(anon) => anon.fields.iter().all(|f| is_sized(f.ty, interner)),
        Ty::Union(a, b) => is_sized(a, interner) && is_sized(b, interner),
        Ty::TypeLit(_) => true,
        Ty::Utility(_, _) => true,
        Ty::Bound(_, _) => true,
        Ty::Placeholder(_) => true,
    }
}

/// Check if a type is `Copy` according to built-in rules.
///
/// This is conservative: ADTs are excluded even though many of them could be
/// `Copy`. The solver must rely on user impls for ADTs.
pub fn is_copy(ty: TyId, interner: &Interner) -> bool {
    match interner.ty(ty) {
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Param(_)
        | Ty::Infer(_)
        | Ty::FnPtr(_)
        | Ty::FnDef(_)
        | Ty::Never => true,
        Ty::Ref(_, Mutability::Not) => true, // Immutable refs are Copy
        Ty::Tuple(args) => args.iter().all(|arg| match arg {
            yelang_ty::generic::GenericArg::Type(t) => is_copy(*t, interner),
            _ => true,
        }),
        Ty::Array(ty, _) => is_copy(ty, interner),
        Ty::Error => true,
        _ => false,
    }
}

/// Check if a type is `Clone` according to built-in rules.
///
/// For Phase 4 `Clone` has the same conservative rules as `Copy`. In the
/// future this will be broader (e.g., `String` is `Clone` but not `Copy`).
pub fn is_clone(ty: TyId, interner: &Interner) -> bool {
    is_copy(ty, interner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_arena::DefId;
    use yelang_ty::interner::Interner;
    use yelang_ty::primitive::IntTy;
    use yelang_ty::ty::{AdtDef, Ty};

    #[test]
    fn primitives_are_sized() {
        let interner = Interner::new();
        assert!(is_sized(interner.mk_ty(Ty::Bool), &interner));
        assert!(is_sized(interner.mk_ty(Ty::Int(IntTy::I32)), &interner));
        assert!(is_sized(
            interner
                .mk_ty(Ty::Float(yelang_ty::primitive::FloatTy::F64)),
            &interner
        ));
    }

    #[test]
    fn slice_not_sized() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        assert!(!is_sized(interner.mk_ty(Ty::Slice(t_i32)), &interner));
    }

    #[test]
    fn tuple_sized_if_elements_sized() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_bool = interner.mk_ty(Ty::Bool);
        let tuple = interner.mk_ty(Ty::Tuple(interner.mk_generic_args(&[
            yelang_ty::generic::GenericArg::Type(t_i32),
            yelang_ty::generic::GenericArg::Type(t_bool),
        ])));
        assert!(is_sized(tuple, &interner));
    }

    #[test]
    fn primitives_are_copy() {
        let interner = Interner::new();
        assert!(is_copy(interner.mk_ty(Ty::Bool), &interner));
        assert!(is_copy(interner.mk_ty(Ty::Int(IntTy::I32)), &interner));
    }

    #[test]
    fn adt_is_not_builtin_copy() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let adt = interner.mk_ty(Ty::Adt(
            AdtDef {
                def_id: DefId::new(1),
            },
            interner.mk_generic_args(&[yelang_ty::generic::GenericArg::Type(t_i32)]),
        ));
        assert!(!is_copy(adt, &interner));
    }
}
