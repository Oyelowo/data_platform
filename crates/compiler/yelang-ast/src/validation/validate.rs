use super::AstValidationError;

/// Run lightweight semantic validations on the AST.
///
/// This is intended to catch shape/binder invariants that are awkward in the parser
/// and should run before name resolution.
pub fn validate_program(program: &crate::Program) -> Vec<AstValidationError> {
    let mut errors = Vec::new();

    // Keep validations independent so we can add/remove checks without entanglement.
    errors.extend(super::update::validate_program(program));

    errors
}
