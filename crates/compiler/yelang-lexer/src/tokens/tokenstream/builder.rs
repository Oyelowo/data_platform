use super::{Token, TokenStream, TokenTrait};
use crate::{Interner, Span};

#[derive(Debug)]
pub struct TokenStreamBuilder<Tkind: TokenTrait> {
    tokens: Vec<Token<Tkind>>,
    interner: Interner,
}

impl<Tkind: TokenTrait> TokenStreamBuilder<Tkind> {
    pub fn new(interner: Interner) -> Self {
        TokenStreamBuilder {
            tokens: Vec::new(),
            interner,
        }
    }

    pub fn append(&mut self, kind: Tkind, span: Span) {
        self.tokens.push(Token::new(kind, span));
    }

    pub fn build(self) -> TokenStream<Tkind> {
        let file_id = self
            .tokens
            .first()
            .map(|t| t.span().file_id())
            .unwrap_or_default();

        TokenStream::new_built(
            self.tokens.into(),
            self.interner,
            Span::default_with_file_id(file_id),
        )
    }
}
