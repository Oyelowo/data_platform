/*! `SolverCtxt` — the trait solver's view of the program.
 *
 * `yelang-trait-solver` is intentionally independent of `yelang-tycheck` and
 * `yelang-hir` to avoid a dependency cycle. The solver only knows what it is
 * told through this trait. `TyCtxt` will implement it in Phase 6.
 */

use yelang_arena::DefId;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{Predicate, TraitRef};

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
}
