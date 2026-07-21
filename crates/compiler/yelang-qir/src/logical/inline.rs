//! Simple THIR method-body inliner for `Queryable` sugar methods.

use yelang_arena::DefId;
use yelang_thir::ThirExprId;

use crate::errors::LoweringError;

/// Inline a trait/impl method body, substituting `self` and formal params with
/// the supplied argument expressions.
pub fn inline_method_body(
    _ctx: &super::ExtractCtxt<'_>,
    _method_def_id: DefId,
    _args: &[ThirExprId],
) -> Result<ThirExprId, LoweringError> {
    // TODO(phase3): implement simple inlining for default sugar bodies.
    Err(LoweringError::UnsupportedExpr)
}
