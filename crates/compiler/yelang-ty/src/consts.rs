/*! Re-exports of constant types from `ty`.
 *
 * The canonical definitions of `Const`, `ConstKind`, `ConstValue`, etc.
 * live in `crate::ty` because they are referenced by `TyKind`.
 */

pub use crate::ty::{Const, ConstKind, ConstValue, PlaceholderConst, UnevaluatedConst};
