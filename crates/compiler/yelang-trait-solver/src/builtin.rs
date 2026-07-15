/*! Built-in trait impls.
 *
 * Traits like `Sized`, `Copy`, `Clone` have built-in rules that the
 * solver knows about without requiring user-written impls.
 */

use yelang_ty::ty::TyKind;

/// Check if a type is `Sized` according to built-in rules.
pub fn is_sized(ty_kind: &TyKind<'_>) -> bool {
    match ty_kind {
        TyKind::Bool
        | TyKind::Char
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::Str
        | TyKind::Param(_)
        | TyKind::Infer(_)
        | TyKind::FnPtr(_)
        | TyKind::FnDef(_)
        | TyKind::RawPtr(_)
        | TyKind::Ref(_, _)
        | TyKind::Never => true,
        TyKind::Adt(_, _) => true, // Most ADTs are Sized
        TyKind::Tuple(args) => args.iter().all(|arg| match arg {
            yelang_ty::generic::GenericArg::Type(t) => is_sized(t.kind()),
            _ => true,
        }),
        TyKind::Array(ty, _) => is_sized(ty.kind()),
        TyKind::Slice(_) => false,   // Dynamically sized
        TyKind::Dynamic(_) => false, // Dynamically sized
        TyKind::Error => true,
        TyKind::Alias(_) => true, // TODO: check alias expansion
        TyKind::AnonStruct(anon) => anon.fields.iter().all(|f| is_sized(f.ty.kind())),
        TyKind::Union(a, b) => is_sized(a.kind()) && is_sized(b.kind()),
        TyKind::TypeLit(_) => true,
        TyKind::Utility(_, _) => true,
        TyKind::Bound(_, _) => true,
        TyKind::Placeholder(_) => true,
    }
}

/// Check if a type is `Copy` according to built-in rules.
pub fn is_copy(ty_kind: &TyKind<'_>) -> bool {
    match ty_kind {
        TyKind::Bool
        | TyKind::Char
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::Param(_)
        | TyKind::Infer(_)
        | TyKind::FnPtr(_)
        | TyKind::FnDef(_)
        | TyKind::Never => true,
        TyKind::Ref(_, yelang_ty::ty::Mutability::Not) => true, // Immutable refs are Copy
        TyKind::Tuple(args) => args.iter().all(|arg| match arg {
            yelang_ty::generic::GenericArg::Type(t) => is_copy(t.kind()),
            _ => true,
        }),
        TyKind::Array(ty, _) => is_copy(ty.kind()),
        TyKind::Error => true,
        _ => false,
    }
}

/// Check if a type is `Clone` according to built-in rules.
pub fn is_clone(ty_kind: &TyKind<'_>) -> bool {
    // For now, Clone has the same built-in rules as Copy.
    // In a full implementation, Clone would be broader (e.g., String is Clone but not Copy).
    is_copy(ty_kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ty::interner::Interner;
    use yelang_ty::primitive::IntTy;
    use yelang_ty::ty::TyKind;

    #[test]
    fn primitives_are_sized() {
        let interner = Interner::new();
        assert!(is_sized(interner.mk_ty(TyKind::Bool).kind()));
        assert!(is_sized(interner.mk_ty(TyKind::Int(IntTy::I32)).kind()));
        assert!(is_sized(
            interner
                .mk_ty(TyKind::Float(yelang_ty::primitive::FloatTy::F64))
                .kind()
        ));
    }

    #[test]
    fn slice_not_sized() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        assert!(!is_sized(interner.mk_ty(TyKind::Slice(t_i32)).kind()));
    }

    #[test]
    fn tuple_sized_if_elements_sized() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_bool = interner.mk_ty(TyKind::Bool);
        let tuple = interner.mk_ty(TyKind::Tuple(interner.mk_generic_args(&[
            yelang_ty::generic::GenericArg::Type(t_i32),
            yelang_ty::generic::GenericArg::Type(t_bool),
        ])));
        assert!(is_sized(tuple.kind()));
    }

    #[test]
    fn primitives_are_copy() {
        let interner = Interner::new();
        assert!(is_copy(interner.mk_ty(TyKind::Bool).kind()));
        assert!(is_copy(interner.mk_ty(TyKind::Int(IntTy::I32)).kind()));
    }
}
