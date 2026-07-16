//! Proc-macro function signatures.

/// The kind of a procedural macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcMacroKind {
    FunctionLike,
    Attribute,
    Derive,
}
