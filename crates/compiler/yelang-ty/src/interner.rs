/*! Arena-based interner for types and lists.
 *
 * The `Interner` provides hash-consing: structurally equal types (and lists)
 * share the same allocation, so equality is a single pointer comparison.
 */

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use bumpalo::Bump;
use rustc_hash::FxHashMap;

use crate::list::List;
use crate::ty::{Ty, TyKind};

/// An interning arena for the type system.
///
/// All `Ty` and `List` values are allocated inside `Bump` and tracked in
/// hash maps so that duplicate structures are deduplicated.
pub struct Interner<'tcx> {
    arena: Bump,
    types: RefCell<FxHashMap<TyKind<'tcx>, Ty<'tcx>>>,
    ty_lists: RefCell<FxHashMap<SliceKey<Ty<'tcx>>, List<Ty<'tcx>>>>,
    generic_args: RefCell<
        FxHashMap<
            SliceKey<crate::generic::GenericArg<'tcx>>,
            List<crate::generic::GenericArg<'tcx>>,
        >,
    >,
    bound_var_lists: RefCell<
        FxHashMap<
            SliceKey<crate::binder::BoundVariableKind>,
            List<crate::binder::BoundVariableKind>,
        >,
    >,
    existential_predicates: RefCell<
        FxHashMap<
            SliceKey<crate::ty::ExistentialPredicate<'tcx>>,
            List<crate::ty::ExistentialPredicate<'tcx>>,
        >,
    >,
    anon_struct_fields: RefCell<FxHashMap<SliceKey<crate::ty::AnonField<'tcx>>, List<crate::ty::AnonField<'tcx>>>>,
    predicates: RefCell<FxHashMap<SliceKey<crate::predicate::Predicate<'tcx>>, List<crate::predicate::Predicate<'tcx>>>>,
    canonical_var_kinds: RefCell<FxHashMap<SliceKey<crate::canonical::CanonicalVarKind>, List<crate::canonical::CanonicalVarKind>>>,
    _marker: PhantomData<&'tcx ()>,
}

/// A hash key that compares slices by their **contents**, not by pointer.
/// This is required for interning: two input slices with the same elements
/// must map to the same interned `List`.
#[derive(Clone, Copy)]
struct SliceKey<T: Copy> {
    ptr: *const T,
    len: usize,
}

impl<T: Copy + PartialEq> PartialEq for SliceKey<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        // SAFETY: Both pointers are valid for `len` elements (they come from slices).
        unsafe {
            std::slice::from_raw_parts(self.ptr, self.len)
                == std::slice::from_raw_parts(other.ptr, other.len)
        }
    }
}

impl<T: Copy + Eq> Eq for SliceKey<T> {}

impl<T: Copy + Hash> Hash for SliceKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // SAFETY: The pointer is valid for `len` elements.
        unsafe {
            std::slice::from_raw_parts(self.ptr, self.len).hash(state);
        }
    }
}

impl<'tcx> Interner<'tcx> {
    pub fn new() -> Self {
        Self {
            arena: Bump::new(),
            types: RefCell::new(FxHashMap::default()),
            ty_lists: RefCell::new(FxHashMap::default()),
            generic_args: RefCell::new(FxHashMap::default()),
            bound_var_lists: RefCell::new(FxHashMap::default()),
            existential_predicates: RefCell::new(FxHashMap::default()),
            anon_struct_fields: RefCell::new(FxHashMap::default()),
            predicates: RefCell::new(FxHashMap::default()),
            canonical_var_kinds: RefCell::new(FxHashMap::default()),
            _marker: PhantomData,
        }
    }

    /// Intern a `TyKind` and return the canonical `Ty`.
    pub fn mk_ty(&self, kind: TyKind<'tcx>) -> Ty<'tcx> {
        if let Some(&existing) = self.types.borrow().get(&kind) {
            return existing;
        }
        let ptr = self.arena.alloc(kind);
        // SAFETY: The arena is owned by `Interner<'tcx>` and lives for `'tcx`.
        let ptr: &'tcx TyKind<'tcx> = unsafe { std::mem::transmute(ptr) };
        let ty = Ty::from_ptr(ptr);
        self.types.borrow_mut().insert(kind, ty);
        ty
    }

    /// Intern a list of types.
    pub fn mk_ty_list(&self, elems: &[Ty<'tcx>]) -> List<Ty<'tcx>> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.ty_lists.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        // SAFETY: The arena lives for `'tcx`.
        let slice: &'tcx [Ty<'tcx>] = unsafe { std::mem::transmute(slice) };
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.ty_lists.borrow_mut().insert(key, list);
        list
    }

    /// Intern a list of generic arguments.
    pub fn mk_generic_args(
        &self,
        elems: &[crate::generic::GenericArg<'tcx>],
    ) -> List<crate::generic::GenericArg<'tcx>> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.generic_args.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        let slice: &'tcx [crate::generic::GenericArg<'tcx>] = unsafe { std::mem::transmute(slice) };
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.generic_args.borrow_mut().insert(key, list);
        list
    }

    /// Intern a list of bound variable kinds.
    pub fn mk_bound_var_list(
        &self,
        elems: &[crate::binder::BoundVariableKind],
    ) -> List<crate::binder::BoundVariableKind> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.bound_var_lists.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        let slice: &'tcx [crate::binder::BoundVariableKind] = unsafe { std::mem::transmute(slice) };
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.bound_var_lists.borrow_mut().insert(key, list);
        list
    }

    /// Allocate a value in the arena.
    pub fn alloc<T>(&self, value: T) -> &'tcx T {
        let ptr = self.arena.alloc(value);
        // SAFETY: The arena lives for `'tcx`.
        unsafe { std::mem::transmute(ptr) }
    }

    /// Intern a list of existential predicates.
    pub fn mk_existential_predicates(
        &self,
        elems: &[crate::ty::ExistentialPredicate<'tcx>],
    ) -> List<crate::ty::ExistentialPredicate<'tcx>> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.existential_predicates.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        let slice: &'tcx [crate::ty::ExistentialPredicate<'tcx>] =
            unsafe { std::mem::transmute(slice) };
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.existential_predicates.borrow_mut().insert(key, list);
        list
    }

    /// Intern a list of anonymous struct fields.
    pub fn mk_anon_struct_fields(
        &self,
        elems: &[crate::ty::AnonField<'tcx>],
    ) -> List<crate::ty::AnonField<'tcx>> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.anon_struct_fields.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        let slice: &'tcx [crate::ty::AnonField<'tcx>] = unsafe { std::mem::transmute(slice) };
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.anon_struct_fields.borrow_mut().insert(key, list);
        list
    }

    /// Intern a list of predicates.
    pub fn mk_predicates(
        &self,
        elems: &[crate::predicate::Predicate<'tcx>],
    ) -> List<crate::predicate::Predicate<'tcx>> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.predicates.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        let slice: &'tcx [crate::predicate::Predicate<'tcx>] =
            unsafe { std::mem::transmute(slice) };
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.predicates.borrow_mut().insert(key, list);
        list
    }

    /// Intern a list of canonical variable kinds.
    pub fn mk_canonical_var_kinds(
        &self,
        elems: &[crate::canonical::CanonicalVarKind],
    ) -> List<crate::canonical::CanonicalVarKind> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.canonical_var_kinds.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        let slice: &'tcx [crate::canonical::CanonicalVarKind] =
            unsafe { std::mem::transmute(slice) };
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.canonical_var_kinds.borrow_mut().insert(key, list);
        list
    }

    /// Generic list interning for types that do not have a dedicated table.
    ///
    /// Prefer the dedicated `mk_*` methods for known list kinds; this is a
    /// fallback for one-off lists (e.g., `List<Ty>` used in tests).
    pub fn mk_list<T: Copy + 'tcx>(&self, elems: &[T]) -> List<T> {
        if elems.is_empty() {
            return List::empty();
        }
        // Fallback: allocate without deduplication. Callers that need
        // deduplication should use a dedicated interner method.
        let slice = self.arena.alloc_slice_copy(elems);
        let slice: &'tcx [T] = unsafe { std::mem::transmute(slice) };
        List::from_slice(slice)
    }

    /// Allocate a slice copy in the arena.
    pub fn alloc_slice<T: Copy>(&self, slice: &[T]) -> &'tcx [T] {
        let ptr = self.arena.alloc_slice_copy(slice);
        // SAFETY: The arena lives for `'tcx`.
        unsafe { std::mem::transmute(ptr) }
    }
}

impl<'tcx> Default for Interner<'tcx> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::{IntTy, UintTy};
    use crate::ty::TyKind;

    #[test]
    fn intern_ty_deduplicates() {
        let interner = Interner::new();
        let ty1 = interner.mk_ty(TyKind::Bool);
        let ty2 = interner.mk_ty(TyKind::Bool);
        assert_eq!(ty1, ty2);
        // Pointer equality
        assert_eq!(ty1.as_ptr(), ty2.as_ptr());
    }

    #[test]
    fn intern_different_types_distinct() {
        let interner = Interner::new();
        let t_bool = interner.mk_ty(TyKind::Bool);
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_u32 = interner.mk_ty(TyKind::Uint(UintTy::U32));
        assert_ne!(t_bool, t_i32);
        assert_ne!(t_i32, t_u32);
    }

    #[test]
    fn intern_ty_list_deduplicates() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(TyKind::Int(IntTy::I32));
        let t_bool = interner.mk_ty(TyKind::Bool);
        let list1 = interner.mk_ty_list(&[t_i32, t_bool]);
        let list2 = interner.mk_ty_list(&[t_i32, t_bool]);
        assert_eq!(list1, list2);
    }

    #[test]
    fn intern_empty_list() {
        let interner = Interner::new();
        let empty1 = interner.mk_ty_list(&[]);
        let empty2 = interner.mk_ty_list(&[]);
        assert_eq!(empty1, empty2);
        assert!(empty1.is_empty());
    }
}
