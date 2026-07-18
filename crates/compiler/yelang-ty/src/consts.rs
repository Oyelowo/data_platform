/*! Re-exports of constant types from `ty`.
 *
 * The canonical definitions of `Const`, `ConstValue`, etc.
 * live in `crate::ty` because they are referenced by `Ty`.
 */

pub use crate::ty::{Const, ConstValue, ParamConst, PlaceholderConst, UnevaluatedConst};
