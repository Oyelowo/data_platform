//! Function bodies and parameters.

use yelang_lexer::Span;

use crate::ids::{ExprId, PatId, HirTyId};

/// A function body.
#[derive(Debug, Clone)]
pub struct Body {
    pub params: Vec<Param>,
    pub value: ExprId,
    pub span: Span,
}

/// A parameter in a function signature or closure.
#[derive(Debug, Clone)]
pub struct Param {
    pub pat: PatId,
    pub ty: HirTyId,
    pub span: Span,
}
