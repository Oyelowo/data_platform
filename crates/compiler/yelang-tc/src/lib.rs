/*!
 * yelang-tc: The Yelang type checker.
 *
 * Architecture:
 * - `ty`      — Type representation, interning, and unification
 * - `error`   — Typed diagnostic errors
 * - `inference` — Hindley-Milner style type inference with extensions
 * - `check`   — Bidirectional type checking (synthesis + checking)
 *
 * Type system features:
 * - Row polymorphism for anonymous structs
 * - Union & intersection types (type literals)
 * - Utility types: Omit, Pick, ReturnType, Params
 * - Higher-ranked type bounds (HRTB): `for<T> fn(T) -> T`
 * - No lifetimes, no borrow checker
 */

pub mod check;
pub mod error;
pub mod inference;
pub mod ty;

/// A typed compilation unit: HIR + inferred types for every node.
pub struct TypedCrate {
    // TODO: links to HIR, type tables, etc.
}
