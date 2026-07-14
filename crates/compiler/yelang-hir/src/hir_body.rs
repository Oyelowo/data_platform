//! Function bodies and parameters.

use yelang_lexer::Span;

use crate::hir_pat::Pat;
use crate::hir_ty::Ty;

/// A function body.
#[derive(Debug, Clone)]
pub struct Body {
    pub params: Vec<Param>,
    pub value: crate::hir::Expr,
    pub span: Span,
}

/// A parameter in a function signature or closure.
#[derive(Debug, Clone)]
pub struct Param {
    pub pat: Pat,
    pub ty: Ty,
    pub span: Span,
}
