/*! `TyCtxt` — global type context for a crate.
 *
 * `TyCtxt` owns the `Interner`, all item-type tables, and the HIR reference.
 * It is the single source of truth for item signatures, ADT layouts, trait
 * definitions, and impl blocks.
 */

use yelang_arena::{DefId, FxHashMap, Id, index_vec as iv};
use yelang_ast::Ident;
use yelang_hir::Crate as HirCrate;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{Predicate, TraitRef};
use yelang_ty::ty::{Const, PolyFnSig, Ty};

/// Tag for impl-def IDs in the global impl table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImplDefTag;

/// ID of an impl block in `TyCtxt::impl_defs`.
pub type ImplDefId = Id<ImplDefTag>;

/// The global type context.
pub struct TyCtxt<'tcx> {
    interner: Interner<'tcx>,
    crate_hir: &'tcx HirCrate,

    // -----------------------------------------------------------------------
    // Item tables
    // -----------------------------------------------------------------------
    /// The canonical type of each item (fn, struct, enum, trait, const, static).
    pub item_types: iv::SecondaryMap<DefId, Ty<'tcx>>,
    /// ADT definitions.
    pub adt_defs: iv::SecondaryMap<DefId, AdtDefData<'tcx>>,
    /// Function signatures.
    pub fn_sigs: iv::SecondaryMap<DefId, PolyFnSig<'tcx>>,
    /// Trait definitions.
    pub trait_defs: iv::SecondaryMap<DefId, TraitDefData<'tcx>>,
    /// Impl blocks.
    pub impl_defs: iv::IndexVec<ImplDefId, ImplDefData<'tcx>>,
    /// Index from trait `DefId` to impl blocks that implement it.
    pub trait_impl_index: FxHashMap<DefId, Vec<ImplDefId>>,
}

impl<'tcx> TyCtxt<'tcx> {
    pub fn new(crate_hir: &'tcx HirCrate) -> Self {
        Self {
            interner: Interner::new(),
            crate_hir,
            item_types: iv::SecondaryMap::new(),
            adt_defs: iv::SecondaryMap::new(),
            fn_sigs: iv::SecondaryMap::new(),
            trait_defs: iv::SecondaryMap::new(),
            impl_defs: iv::IndexVec::new(),
            trait_impl_index: FxHashMap::default(),
        }
    }

    /// The interner for creating canonical types and lists.
    pub fn interner(&self) -> &Interner<'tcx> {
        &self.interner
    }

    /// The HIR crate being type-checked.
    pub fn crate_hir(&self) -> &'tcx HirCrate {
        self.crate_hir
    }

    /// Look up the type of an item.
    pub fn item_ty(&self, def_id: DefId) -> Option<Ty<'tcx>> {
        self.item_types.get(def_id).copied()
    }

    /// Look up an ADT definition.
    pub fn adt_def(&self, def_id: DefId) -> Option<&AdtDefData<'tcx>> {
        self.adt_defs.get(def_id)
    }

    /// Look up a function signature.
    pub fn fn_sig(&self, def_id: DefId) -> Option<PolyFnSig<'tcx>> {
        self.fn_sigs.get(def_id).copied()
    }

    /// Look up a trait definition.
    pub fn trait_def(&self, def_id: DefId) -> Option<&TraitDefData<'tcx>> {
        self.trait_defs.get(def_id)
    }

    /// Look up an impl definition.
    pub fn impl_def(&self, id: ImplDefId) -> &ImplDefData<'tcx> {
        &self.impl_defs[id]
    }

    /// Return all impls for a given trait, if any.
    pub fn trait_impls(&self, trait_def_id: DefId) -> &[ImplDefId] {
        self.trait_impl_index.get(&trait_def_id).map(|v| v.as_slice()).unwrap_or(&[])
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
pub struct AdtDefData<'tcx> {
    pub def_id: DefId,
    pub kind: AdtKind,
    pub ident: Ident,
    pub variants: Vec<VariantData<'tcx>>,
    pub generics: GenericsData<'tcx>,
}

/// Lowered enum/struct variant.
#[derive(Debug, Clone)]
pub struct VariantData<'tcx> {
    pub def_id: DefId,
    pub ident: Ident,
    pub fields: Vec<FieldData<'tcx>>,
    pub discriminant: Option<Const<'tcx>>,
}

/// Lowered struct/variant field.
#[derive(Debug, Clone)]
pub struct FieldData<'tcx> {
    pub def_id: DefId,
    pub ident: Ident,
    pub ty: Ty<'tcx>,
}

/// Lowered trait definition.
#[derive(Debug, Clone)]
pub struct TraitDefData<'tcx> {
    pub def_id: DefId,
    pub ident: Ident,
    pub generics: GenericsData<'tcx>,
    pub supertraits: Vec<TraitRef<'tcx>>,
    pub items: Vec<TraitItemDefData<'tcx>>,
}

/// Lowered trait item.
#[derive(Debug, Clone)]
pub enum TraitItemDefData<'tcx> {
    Fn {
        def_id: DefId,
        sig: PolyFnSig<'tcx>,
    },
    Const {
        def_id: DefId,
        ty: Ty<'tcx>,
    },
    Type {
        def_id: DefId,
        bounds: Vec<TraitRef<'tcx>>,
        default: Option<Ty<'tcx>>,
    },
}

/// Lowered impl block.
#[derive(Debug, Clone)]
pub struct ImplDefData<'tcx> {
    pub id: ImplDefId,
    pub def_id: DefId,
    pub trait_ref: Option<TraitRef<'tcx>>,
    pub self_ty: Ty<'tcx>,
    pub generics: GenericsData<'tcx>,
    pub items: Vec<ImplItemDefData<'tcx>>,
}

/// Lowered impl item.
#[derive(Debug, Clone)]
pub enum ImplItemDefData<'tcx> {
    Fn {
        def_id: DefId,
        sig: PolyFnSig<'tcx>,
    },
    Const {
        def_id: DefId,
        ty: Ty<'tcx>,
    },
    Type {
        def_id: DefId,
        ty: Ty<'tcx>,
    },
}

/// Lowered generics and where clauses.
#[derive(Debug, Clone, Default)]
pub struct GenericsData<'tcx> {
    pub params: Vec<GenericParamData>,
    pub predicates: Vec<Predicate<'tcx>>,
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
