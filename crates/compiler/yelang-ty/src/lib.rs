/*!
 * yelang-ty: Core Type System IR
 *
 * This crate defines the canonical representation of types used throughout
 * the Yelang compiler after HIR lowering. Every `TyId` is an interned ID into
 * the `Interner`'s dense type table, so equality is integer equality.
 *
 * ## Architecture
 *
 * - `ty::Ty` — All type constructors (primitives, ADTs, tuples, etc.).
 * - `ty::TyId` — A lifetime-free ID for an interned `Ty`.
 * - `ty::Const` — All type-level constant constructors.
 * - `ty::ConstId` — A lifetime-free ID for an interned `Const`.
 * - `interner::Interner` — Hash-consing arena for types, constants, and lists.
 * - `generic::GenericArg` — Type/const generic arguments.
 * - `predicate::Predicate` — Trait bounds, projection equalities.
 * - `canonical::Canonical<T>` — Inference-var-free goals for caching.
 * - `fold` / `visit` — Structural traversal traits.
 *
 * ## Design Axioms
 *
 * 1. **ID equality**: Two types are equal iff their `TyId` IDs are equal.
 * 2. **No lifetimes**: Yelang is lifetime-free; no region variables exist.
 * 3. **No subtyping**: Unification is equality; width subtyping is coercion.
 * 4. **Copy everywhere**: `TyId`/`ConstId` are `Copy` (they are just 4-byte IDs).
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
pub use fold::*;
pub use generic::*;
pub use interner::*;
pub use list::*;
pub use predicate::*;
pub use primitive::*;
pub use subst::*;
pub use ty::*;
pub use visit::*;

#[cfg(test)]
mod tests;
