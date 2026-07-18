/*! Arena-based interner for types, constants, and lists.
 *
 * The `Interner` provides hash-consing: structurally equal types (and lists)
 * share the same ID, so equality is a single integer comparison.
 */

use std::cell::RefCell;
use std::hash::{Hash, Hasher};

use rustc_hash::FxHashMap;
use yelang_arena::{ConstId, IndexVec, TyId};

use crate::generic::GenericArg;
use crate::list::List;
use crate::ty::{Const, ConstData, Ty};

/// An interning arena for the type system.
///
/// All `TyId`/`ConstId` IDs are backed by dense `IndexVec` tables, and all
/// `List` values are allocated inside a `bumpalo::Bump` arena.
pub struct Interner {
    /// Dense storage for interned type constructors.
    types: RefCell<IndexVec<TyId, Ty>>,
    /// Hash-consing map from `Ty` to its `TyId`.
    type_map: RefCell<FxHashMap<Ty, TyId>>,
    /// Dense storage for interned constants.
    consts: RefCell<IndexVec<ConstId, ConstData>>,
    /// Hash-consing map from `ConstData` to its `ConstId`.
    const_map: RefCell<FxHashMap<ConstData, ConstId>>,

    // List interning. Lists are immutable, hash-consed slices allocated in the
    // bump arena so pointer equality implies structural equality.
    arena: bumpalo::Bump,
    ty_lists: RefCell<FxHashMap<SliceKey<TyId>, List<TyId>>>,
    generic_args: RefCell<FxHashMap<SliceKey<GenericArg>, List<GenericArg>>>,
    bound_var_lists: RefCell<FxHashMap<SliceKey<crate::binder::BoundVariableKind>, List<crate::binder::BoundVariableKind>>>,
    existential_predicates: RefCell<FxHashMap<SliceKey<crate::ty::ExistentialPredicate>, List<crate::ty::ExistentialPredicate>>>,
    anon_struct_fields: RefCell<FxHashMap<SliceKey<crate::ty::AnonField>, List<crate::ty::AnonField>>>,
    predicates: RefCell<FxHashMap<SliceKey<crate::predicate::Predicate>, List<crate::predicate::Predicate>>>,
    canonical_var_kinds: RefCell<FxHashMap<SliceKey<crate::canonical::CanonicalVarKind>, List<crate::canonical::CanonicalVarKind>>>,
    canonical_var_values: RefCell<FxHashMap<SliceKey<crate::canonical::CanonicalVarValue>, List<crate::canonical::CanonicalVarValue>>>,
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
                == std::slice::from_raw_parts(other.ptr, self.len)
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

impl Interner {
    pub fn new() -> Self {
        Self {
            types: RefCell::new(IndexVec::new()),
            type_map: RefCell::new(FxHashMap::default()),
            consts: RefCell::new(IndexVec::new()),
            const_map: RefCell::new(FxHashMap::default()),
            arena: bumpalo::Bump::new(),
            ty_lists: RefCell::new(FxHashMap::default()),
            generic_args: RefCell::new(FxHashMap::default()),
            bound_var_lists: RefCell::new(FxHashMap::default()),
            existential_predicates: RefCell::new(FxHashMap::default()),
            anon_struct_fields: RefCell::new(FxHashMap::default()),
            predicates: RefCell::new(FxHashMap::default()),
            canonical_var_kinds: RefCell::new(FxHashMap::default()),
            canonical_var_values: RefCell::new(FxHashMap::default()),
        }
    }

    /// Look up a `Ty` by ID.
    pub fn ty(&self, id: TyId) -> Ty {
        self.types.borrow()[id]
    }

    /// Look up a `ConstData` kind by ID.
    pub fn const_kind(&self, id: ConstId) -> Const {
        self.consts.borrow()[id].kind
    }

    /// Look up a `ConstData` type by ID.
    pub fn const_ty(&self, id: ConstId) -> TyId {
        self.consts.borrow()[id].ty
    }

    /// Intern a `Ty` and return the canonical `TyId`.
    pub fn mk_ty(&self, ty: Ty) -> TyId {
        if let Some(&existing) = self.type_map.borrow().get(&ty) {
            return existing;
        }
        let id = self.types.borrow_mut().push(ty);
        self.type_map.borrow_mut().insert(ty, id);
        id
    }

    /// Intern a `ConstData` and return the canonical `ConstId`.
    pub fn mk_const(&self, data: ConstData) -> ConstId {
        if let Some(&existing) = self.const_map.borrow().get(&data) {
            return existing;
        }
        let id = self.consts.borrow_mut().push(data);
        self.const_map.borrow_mut().insert(data, id);
        id
    }

    /// Convenience: intern a constant from its kind and type.
    pub fn mk_const_from_parts(&self, kind: Const, ty: TyId) -> ConstId {
        self.mk_const(ConstData { kind, ty })
    }

    /// Intern a list of types.
    pub fn mk_ty_list(&self, elems: &[TyId]) -> List<TyId> {
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
        elems: &[GenericArg],
    ) -> List<GenericArg> {
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
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.bound_var_lists.borrow_mut().insert(key, list);
        list
    }

    /// Allocate a value in the arena.
    pub fn alloc<T>(&self, value: T) -> &T {
        self.arena.alloc(value)
    }

    /// Intern a list of existential predicates.
    pub fn mk_existential_predicates(
        &self,
        elems: &[crate::ty::ExistentialPredicate],
    ) -> List<crate::ty::ExistentialPredicate> {
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
        elems: &[crate::ty::AnonField],
    ) -> List<crate::ty::AnonField> {
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
        elems: &[crate::predicate::Predicate],
    ) -> List<crate::predicate::Predicate> {
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
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.canonical_var_kinds.borrow_mut().insert(key, list);
        list
    }

    /// Intern a list of canonical variable values.
    pub fn mk_canonical_var_values(
        &self,
        elems: &[crate::canonical::CanonicalVarValue],
    ) -> List<crate::canonical::CanonicalVarValue> {
        if elems.is_empty() {
            return List::empty();
        }
        let key = SliceKey {
            ptr: elems.as_ptr(),
            len: elems.len(),
        };
        if let Some(&existing) = self.canonical_var_values.borrow().get(&key) {
            return existing;
        }
        let slice = self.arena.alloc_slice_copy(elems);
        let list = List::from_slice(slice);
        let key = SliceKey {
            ptr: slice.as_ptr(),
            len: slice.len(),
        };
        self.canonical_var_values.borrow_mut().insert(key, list);
        list
    }

    /// Generic list interning for types that do not have a dedicated table.
    ///
    /// Prefer the dedicated `mk_*` methods for known list kinds; this is a
    /// fallback for one-off lists (e.g., `List<TyId>` used in tests).
    pub fn mk_list<T: Copy>(&self, elems: &[T]) -> List<T> {
        if elems.is_empty() {
            return List::empty();
        }
        // Fallback: allocate without deduplication. Callers that need
        // deduplication should use a dedicated interner method.
        let slice = self.arena.alloc_slice_copy(elems);
        List::from_slice(slice)
    }

    /// Allocate a slice copy in the arena.
    pub fn alloc_slice<T: Copy>(&self, slice: &[T]) -> &[T] {
        self.arena.alloc_slice_copy(slice)
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::{IntTy, UintTy};
    use crate::ty::Ty;

    #[test]
    fn intern_ty_deduplicates() {
        let interner = Interner::new();
        let ty1 = interner.mk_ty(Ty::Bool);
        let ty2 = interner.mk_ty(Ty::Bool);
        assert_eq!(ty1, ty2);
    }

    #[test]
    fn intern_different_types_distinct() {
        let interner = Interner::new();
        let t_bool = interner.mk_ty(Ty::Bool);
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_u32 = interner.mk_ty(Ty::Uint(UintTy::U32));
        assert_ne!(t_bool, t_i32);
        assert_ne!(t_i32, t_u32);
    }

    #[test]
    fn intern_ty_list_deduplicates() {
        let interner = Interner::new();
        let t_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let t_bool = interner.mk_ty(Ty::Bool);
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

    #[test]
    fn intern_const_deduplicates() {
        let interner = Interner::new();
        let ty_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let c1 = interner.mk_const_from_parts(Const::Value(crate::ty::ConstValue::Int(42)), ty_i32);
        let c2 = interner.mk_const_from_parts(Const::Value(crate::ty::ConstValue::Int(42)), ty_i32);
        assert_eq!(c1, c2);
    }

    #[test]
    fn intern_different_consts_distinct() {
        let interner = Interner::new();
        let ty_i32 = interner.mk_ty(Ty::Int(IntTy::I32));
        let c1 = interner.mk_const_from_parts(Const::Value(crate::ty::ConstValue::Int(42)), ty_i32);
        let c2 = interner.mk_const_from_parts(Const::Value(crate::ty::ConstValue::Int(43)), ty_i32);
        let c3 = interner.mk_const_from_parts(Const::Value(crate::ty::ConstValue::Int(42)), interner.mk_ty(Ty::Int(IntTy::I64)));
        assert_ne!(c1, c2);
        assert_ne!(c1, c3);
        assert_ne!(c2, c3);
    }
}
