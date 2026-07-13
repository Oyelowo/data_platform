use super::AstValidationError;
use crate::CreatePath;

/// Validate LINK path structure at the AST level.
///
/// This is a placeholder for future work.
///
/// Intended future checks (examples):
/// - Direction-token consistency (if/when the AST preserves both sides)
/// - Binder visibility rules for edge/node aliases
/// - Restrictions for "mutation blocks" (if LINK is allowed inside them)
pub fn validate_link_path(_path: &CreatePath) -> Vec<AstValidationError> {
    Vec::new()
}
