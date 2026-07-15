//! Errors produced during AST -> HIR lowering.

use thiserror::Error;
use yelang_interner::Symbol;
use yelang_lexer::Span;

/// An error encountered while lowering the AST to HIR.
#[derive(Error, Debug, Clone)]
pub enum LoweringError {
    #[error("cannot resolve `{name}` during lowering")]
    UnresolvedName { name: Symbol, span: Span },

    #[error("unsupported AST node: {kind}")]
    UnsupportedAst { kind: String, span: Span },
}
