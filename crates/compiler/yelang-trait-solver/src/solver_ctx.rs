/*! `SolverCtxt` — the trait solver's view of the program.
 *
 * `yelang-trait-solver` is intentionally independent of `yelang-tycheck` and
 * `yelang-hir` to avoid a dependency cycle. The solver only knows what it is
 * told through this trait. `TyCtxt` will implement it in Phase 6.
 */

use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{Predicate, TraitRef};
use yelang_ty::ty::{ImplPolarity, PolyFnSig, Ty};

/// Information about a trait definition that the solver needs.
#[derive(Clone, Debug)]
pub struct TraitDefInfo<'tcx> {
    pub def_id: DefId,
    /// Whether this is an auto trait (coinductive cycles are allowed).
    pub is_auto: bool,
    /// Supertraits, e.g. `trait Foo: Bar + Baz` stores `Bar` and `Baz`.
    pub supertraits: Vec<TraitRef<'tcx>>,
}

/// Information about a trait impl block that the solver needs.
#[derive(Clone, Debug)]
pub struct ImplInfo<'tcx> {
    pub def_id: DefId,
    /// The trait ref implemented by this impl, with `Self` as the first arg.
    pub trait_ref: TraitRef<'tcx>,
    /// Polarity of the impl.
    pub polarity: ImplPolarity,
    /// Number of generic parameters (type + const) introduced by the impl.
    pub generic_param_count: usize,
    /// Where-clause predicates of the impl, with params referenced by index.
    pub predicates: Vec<Predicate<'tcx>>,
}

/// Built-in traits the solver knows about without user-written impls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinTraitKind {
    Sized,
    Copy,
    Clone,
}

/// An associated item, used for projection normalization.
#[derive(Clone, Debug)]
pub struct AssocItemInfo<'tcx> {
    pub def_id: DefId,
    /// For an impl assoc item, the def id of the corresponding trait item.
    /// For a trait assoc item this is `None` (the item is its own trait item).
    pub trait_item_def_id: Option<DefId>,
    pub ident: Symbol,
    pub kind: AssocItemKind<'tcx>,
}

/// Kind of an associated item.
#[derive(Clone, Debug)]
pub enum AssocItemKind<'tcx> {
    /// An associated type.
    Type {
        bounds: Vec<TraitRef<'tcx>>,
        default: Option<Ty<'tcx>>,
    },
    /// An associated function.
    Fn { sig: PolyFnSig<'tcx> },
    /// An associated const.
    Const { ty: Ty<'tcx> },
}

/// The solver's interface to the rest of the compiler.
pub trait SolverCtxt<'tcx> {
    /// The interner for creating types and lists.
    fn interner(&self) -> &Interner<'tcx>;

    /// Look up a trait definition.
    fn trait_info(&self, def_id: DefId) -> Option<TraitDefInfo<'tcx>>;

    /// All user-written impls of the given trait.
    fn impls_for_trait(&self, def_id: DefId) -> &[ImplInfo<'tcx>];

    /// If the trait is a built-in, return its kind.
    fn builtin_kind(&self, def_id: DefId) -> Option<BuiltinTraitKind>;

    /// Associated items of a trait definition.
    fn trait_assoc_items(&self, def_id: DefId) -> &[AssocItemInfo<'tcx>];

    /// Associated items of an impl block.
    fn impl_assoc_items(&self, impl_def_id: DefId) -> &[AssocItemInfo<'tcx>];

    /// Field types of an ADT, for auto-trait derivation.
    fn adt_field_tys(&self, adt_def_id: DefId) -> &[Ty<'tcx>];
}
