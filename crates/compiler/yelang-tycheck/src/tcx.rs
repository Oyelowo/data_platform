/*! `TyCtxt` — global type context for a crate.
 *
 * `TyCtxt` owns the `Interner`, all item-type tables, and the HIR reference.
 * It is the single source of truth for item signatures, ADT layouts, trait
 * definitions, and impl blocks.
 */

use yelang_arena::{DefId, FxHashMap, Id, index_vec as iv};
use yelang_ast::Ident;
use yelang_hir::Crate as HirCrate;
use yelang_trait_solver::solver_ctx::{AssocItemInfo, ImplInfo};
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{ParamEnv, Predicate, TraitRef};
use yelang_ty::ty::{ConstId, PolyFnSig, TyId};

// Re-export the built-in trait kind used by `register_builtin_trait`.
pub use yelang_trait_solver::solver_ctx::BuiltinTraitKind;

/// Tag for impl-def IDs in the global impl table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImplDefTag;

/// ID of an impl block in `TyCtxt::impl_defs`.
pub type ImplDefId = Id<ImplDefTag>;

/// The global type context.
pub struct TyCtxt {
    interner: Interner,
    crate_hir: HirCrate,

    // -----------------------------------------------------------------------
    // Item tables
    // -----------------------------------------------------------------------
    /// The canonical type of each item (fn, struct, enum, trait, const, static).
    pub item_types: iv::SecondaryMap<DefId, TyId>,
    /// ADT definitions.
    pub adt_defs: iv::SecondaryMap<DefId, AdtDefData>,
    /// Function signatures.
    pub fn_sigs: iv::SecondaryMap<DefId, PolyFnSig>,
    /// Generic parameters and where clauses for items (fn, struct, enum, trait, impl item).
    pub generics: iv::SecondaryMap<DefId, GenericsData>,
    /// Trait definitions.
    pub trait_defs: iv::SecondaryMap<DefId, TraitDefData>,
    /// Impl blocks.
    pub impl_defs: iv::IndexVec<ImplDefId, ImplDefData>,
    /// Index from trait `DefId` to impl blocks that implement it.
    pub trait_impl_index: FxHashMap<DefId, Vec<ImplDefId>>,
    /// Map from trait `DefId` to built-in trait kind (Sized, Copy, Clone).
    pub builtin_traits: FxHashMap<DefId, BuiltinTraitKind>,
    /// Precomputed solver views of trait impls.
    pub trait_impl_info_cache: FxHashMap<DefId, Box<[ImplInfo]>>,
    /// Precomputed solver views of trait associated items.
    pub trait_assoc_items_cache: FxHashMap<DefId, Box<[AssocItemInfo]>>,
    /// Precomputed solver views of impl associated items.
    pub impl_assoc_items_cache: FxHashMap<DefId, Box<[AssocItemInfo]>>,
    /// Precomputed solver views of ADT field types.
    pub adt_field_tys_cache: FxHashMap<DefId, Box<[TyId]>>,
    /// `Deref` trait lang item, if registered.
    pub deref_trait: Option<DefId>,
    /// `Deref::Target` associated-type lang item, if registered.
    pub deref_target: Option<DefId>,
}

impl TyCtxt {
    pub fn new(crate_hir: HirCrate) -> Self {
        Self {
            interner: Interner::new(),
            crate_hir,
            item_types: iv::SecondaryMap::new(),
            adt_defs: iv::SecondaryMap::new(),
            fn_sigs: iv::SecondaryMap::new(),
            generics: iv::SecondaryMap::new(),
            trait_defs: iv::SecondaryMap::new(),
            impl_defs: iv::IndexVec::new(),
            trait_impl_index: FxHashMap::default(),
            builtin_traits: FxHashMap::default(),
            trait_impl_info_cache: FxHashMap::default(),
            trait_assoc_items_cache: FxHashMap::default(),
            impl_assoc_items_cache: FxHashMap::default(),
            adt_field_tys_cache: FxHashMap::default(),
            deref_trait: None,
            deref_target: None,
        }
    }

    /// The interner for creating canonical types and lists.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// The HIR crate being type-checked.
    pub fn crate_hir(&self) -> &HirCrate {
        &self.crate_hir
    }

    /// Mutable access to the HIR crate, used by tests and driver phases that
    /// need to allocate additional HIR nodes during type checking.
    pub fn crate_hir_mut(&mut self) -> &mut HirCrate {
        &mut self.crate_hir
    }

    /// Look up the type of an item.
    pub fn item_ty(&self, def_id: DefId) -> Option<TyId> {
        self.item_types.get(def_id).copied()
    }

    /// Look up an ADT definition.
    pub fn adt_def(&self, def_id: DefId) -> Option<&AdtDefData> {
        self.adt_defs.get(def_id)
    }

    /// Look up a function signature.
    pub fn fn_sig(&self, def_id: DefId) -> Option<PolyFnSig> {
        self.fn_sigs.get(def_id).copied()
    }

    /// Look up a trait definition.
    pub fn trait_def(&self, def_id: DefId) -> Option<&TraitDefData> {
        self.trait_defs.get(def_id)
    }

    /// Look up an impl definition.
    pub fn impl_def(&self, id: ImplDefId) -> &ImplDefData {
        &self.impl_defs[id]
    }

    /// Return all impls for a given trait, if any.
    pub fn trait_impls(&self, trait_def_id: DefId) -> &[ImplDefId] {
        self.trait_impl_index.get(&trait_def_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Look up the generic data of an item.
    pub fn generics_of(&self, def_id: DefId) -> Option<&GenericsData> {
        self.generics.get(def_id)
    }

    /// Build the `ParamEnv` for a function body or item.
    ///
    /// For top-level items this uses the item's own `GenericsData`. For impl
    /// items it falls back to the enclosing impl block's generics and trait
    /// ref.
    pub fn param_env(&self, def_id: DefId) -> ParamEnv {
        let mut predicates = Vec::new();

        if let Some(generics) = self.generics.get(def_id) {
            predicates.extend(generics.predicates.iter().copied());
        } else {
            // Impl items inherit the impl block's generics.
            for imp in self.impl_defs.iter() {
                if imp.items.iter().any(|item| item.def_id() == def_id) {
                    predicates.extend(imp.generics.predicates.iter().copied());
                    if let Some(tr) = imp.trait_ref {
                        predicates.push(Predicate::Trait(yelang_ty::predicate::TraitPredicate {
                            trait_ref: tr,
                            polarity: yelang_ty::ty::ImplPolarity::Positive,
                        }));
                    }
                    break;
                }
            }
        }

        ParamEnv {
            caller_bounds: self.interner.mk_predicates(&predicates),
        }
    }

    /// Register a prelude trait as a built-in trait for the solver.
    pub fn register_builtin_trait(&mut self, def_id: DefId, kind: BuiltinTraitKind) {
        self.builtin_traits.insert(def_id, kind);
    }

    /// Register the `Deref` trait and its `Target` associated type as lang items.
    ///
    /// Method/field autoderef needs these IDs to build projection normalization
    /// goals (`<T as Deref>::Target normalizes-to U`).
    pub fn register_deref_lang_item(&mut self, trait_def_id: DefId, target_item_def_id: DefId) {
        self.deref_trait = Some(trait_def_id);
        self.deref_target = Some(target_item_def_id);
    }
}

// ---------------------------------------------------------------------------
// Table data types
// ---------------------------------------------------------------------------

/// ADT kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdtKind {
    Struct,
    Enum,
    Union,
}

/// Lowered ADT definition.
#[derive(Debug, Clone)]
pub struct AdtDefData {
    pub def_id: DefId,
    pub kind: AdtKind,
    pub ident: Ident,
    pub variants: Vec<VariantData>,
    pub generics: GenericsData,
}

/// Lowered enum/struct variant.
#[derive(Debug, Clone)]
pub struct VariantData {
    pub def_id: DefId,
    pub ident: Ident,
    pub fields: Vec<FieldData>,
    pub discriminant: Option<ConstId>,
}

/// Lowered struct/variant field.
#[derive(Debug, Clone)]
pub struct FieldData {
    pub def_id: DefId,
    pub ident: Ident,
    pub ty: TyId,
}

/// Lowered trait definition.
#[derive(Debug, Clone)]
pub struct TraitDefData {
    pub def_id: DefId,
    pub ident: Ident,
    pub generics: GenericsData,
    pub supertraits: Vec<TraitRef>,
    pub items: Vec<TraitItemDefData>,
}

/// Lowered trait item.
#[derive(Debug, Clone)]
pub enum TraitItemDefData {
    Fn {
        def_id: DefId,
        ident: Ident,
        sig: PolyFnSig,
    },
    Const {
        def_id: DefId,
        ident: Ident,
        ty: TyId,
    },
    Type {
        def_id: DefId,
        ident: Ident,
        bounds: Vec<TraitRef>,
        default: Option<TyId>,
    },
}

impl TraitItemDefData {
    pub fn def_id(&self) -> DefId {
        match *self {
            TraitItemDefData::Fn { def_id, .. }
            | TraitItemDefData::Const { def_id, .. }
            | TraitItemDefData::Type { def_id, .. } => def_id,
        }
    }

    pub fn ident(&self) -> Ident {
        match *self {
            TraitItemDefData::Fn { ident, .. }
            | TraitItemDefData::Const { ident, .. }
            | TraitItemDefData::Type { ident, .. } => ident,
        }
    }
}

/// Lowered impl block.
#[derive(Debug, Clone)]
pub struct ImplDefData {
    pub id: ImplDefId,
    pub def_id: DefId,
    pub trait_ref: Option<TraitRef>,
    pub self_ty: TyId,
    pub generics: GenericsData,
    pub items: Vec<ImplItemDefData>,
}

/// Lowered impl item.
#[derive(Debug, Clone)]
pub enum ImplItemDefData {
    Fn {
        def_id: DefId,
        ident: Ident,
        sig: PolyFnSig,
    },
    Const {
        def_id: DefId,
        ident: Ident,
        ty: TyId,
    },
    Type {
        def_id: DefId,
        ident: Ident,
        ty: TyId,
    },
}

impl ImplItemDefData {
    pub fn def_id(&self) -> DefId {
        match *self {
            ImplItemDefData::Fn { def_id, .. }
            | ImplItemDefData::Const { def_id, .. }
            | ImplItemDefData::Type { def_id, .. } => def_id,
        }
    }

    pub fn ident(&self) -> Ident {
        match *self {
            ImplItemDefData::Fn { ident, .. }
            | ImplItemDefData::Const { ident, .. }
            | ImplItemDefData::Type { ident, .. } => ident,
        }
    }
}

/// Lowered generics and where clauses.
#[derive(Debug, Clone, Default)]
pub struct GenericsData {
    pub params: Vec<GenericParamData>,
    pub predicates: Vec<Predicate>,
}

/// A lowered generic parameter.
#[derive(Debug, Clone)]
pub struct GenericParamData {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: GenericParamKind,
}

/// Kind of a generic parameter.
#[derive(Debug, Clone, Copy)]
pub enum GenericParamKind {
    Type,
    Const,
}
