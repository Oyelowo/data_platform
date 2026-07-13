use yelang_lexer::Span;

#[derive(Debug, Clone)]
pub struct Node<T> {
    pub kind: T,
    pub span: Span,
}

impl<T> Node<T> {
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Node<U> {
        Node {
            kind: f(self.kind),
            span: self.span,
        }
    }

    pub fn kind(&self) -> &T {
        &self.kind
    }

    pub fn span(&self) -> Span {
        self.span
    }
}
