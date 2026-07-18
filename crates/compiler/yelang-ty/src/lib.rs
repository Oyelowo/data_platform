/*!
 * yelang-ty: Core Type System IR
 *
 * This crate defines the canonical representation of types used throughout
 * the Yelang compiler after HIR lowering. Every `Ty` is interned and
 * arena-allocated, so equality is pointer equality.
 *
 * ## Architecture
 *
 * - `ty::Ty` — A pointer to an interned `TyKind`.
 * - `ty::TyKind` — All type constructors (primitives, ADTs, tuples, etc.).
 * - `interner::Interner` — Hash-consing arena for types and lists.
 * - `generic::GenericArg` — Type/const generic arguments.
 * - `predicate::Predicate` — Trait bounds, projection equalities.
 * - `canonical::Canonical<T>` — Inference-var-free goals for caching.
 * - `fold` / `visit` — Structural traversal traits.
 *
 * ## Design Axioms
 *
 * 1. **Pointer equality**: Two types are equal iff their `Ty` pointers are equal.
 * 2. **No lifetimes**: Yelang is lifetime-free; no region variables exist.
 * 3. **No subtyping**: Unification is equality; width subtyping is coercion.
 * 4. **Copy everywhere**: `Ty<'tcx>` is `Copy` (it's just a reference).
 */

pub mod binder;
pub mod canonical;
pub mod consts;
pub mod existential;
pub mod fold;
pub mod generic;
pub mod interner;
pub mod list;
pub mod predicate;
pub mod primitive;
pub mod projection;
pub mod subst;
pub mod ty;
pub mod visit;

pub use binder::*;
pub use canonical::*;
pub use consts::*;
pub use existential::*;
pub use fold::*;
pub use generic::*;
pub use interner::*;
pub use list::*;
pub use predicate::*;
pub use primitive::*;
pub use projection::*;
pub use subst::*;
pub use ty::*;
pub use visit::*;

#[cfg(test)]
mod tests;
