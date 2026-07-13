use crate::Span;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenCheckpoint {
    pub(crate) position: usize,
    pub(crate) current_span: Span,
}

impl TokenCheckpoint {
    pub fn current_pos(&self) -> Span {
        self.current_span
    }
}
