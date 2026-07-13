use crate::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct SynMeta<T> {
    ast: T,
    span: Span,
}

impl<T> SynMeta<T> {
    pub fn new(ast: T, span: Span) -> Self {
        SynMeta { ast, span }
    }

    pub fn ast(&self) -> &T {
        &self.ast
    }

    pub fn ast_owned(self) -> T {
        self.ast
    }

    pub fn into_parts(self) -> (T, Span) {
        (self.ast, self.span)
    }

    pub fn span(&self) -> Span {
        self.span
    }
}
