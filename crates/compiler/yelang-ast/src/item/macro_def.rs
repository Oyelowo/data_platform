/*
 * Declarative macro definition: `macro name { ... }`
 */

use crate::expr::convert::from_lexer_tokens;
use crate::{Ident, TokenKind};
use yelang_lexer::{
    ParseTokenStream, Span, Token, TokenError, TokenResult, TokenStream, consume_token,
};
use yelang_macro_core::token_tree::TokenStream as MacroTokenStream;

/// A declarative macro definition.
///
/// The `body` token stream contains the matcher / transcriber rules inside the
/// braces. It is stored in the macro-core token-tree format so the expander can
/// operate on it hygienically.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroDef {
    pub name: Ident,
    pub body: MacroTokenStream,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for MacroDef {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let start_span = stream.current_span();
        // Consume the leading `macro` keyword.
        stream.advance();
        let name = *consume_token!(stream, TokenKind::Ident(ident) => ident);

        if stream.peek().map(|t| t.kind()) != Some(&TokenKind::OpenBrace) {
            return Err(TokenError::UnexpectedToken {
                expected: "`{`".to_string(),
                found: stream
                    .peek()
                    .map(|t| t.kind().to_string())
                    .unwrap_or_else(|| "<eof>".to_string()),
                span: stream.current_span(),
            });
        }

        let inner = consume_balanced(stream, TokenKind::OpenBrace, TokenKind::CloseBrace)?;
        let body = from_lexer_tokens(&inner, stream.interner());
        let end_span = stream.current_span();
        let span = start_span.merge(end_span);

        Ok(MacroDef { name, body, span })
    }
}

fn consume_balanced(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
    open_kind: crate::tokenizer::TokenKind,
    close_kind: crate::tokenizer::TokenKind,
) -> TokenResult<Vec<Token<crate::tokenizer::TokenKind>>> {
    // Consume the opening token.
    let open_span = stream.current_span();
    stream.advance().ok_or_else(|| TokenError::UnexpectedEof {
        expected: "opening delimiter".to_string(),
        span: open_span,
    })?;

    let mut depth = 1usize;
    let mut tokens = Vec::new();

    loop {
        let eof_span = stream.current_span();
        let token = stream.advance().ok_or_else(|| TokenError::UnexpectedEof {
            expected: "matching delimiter".to_string(),
            span: eof_span,
        })?;

        if token.kind() == &open_kind {
            depth += 1;
        } else if token.kind() == &close_kind {
            depth -= 1;
        }

        if depth == 0 {
            break;
        }

        tokens.push(token.clone());
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interner, TokenKind};
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn parse_simple_macro_def() {
        let mut interner = Interner::new();
        let src = r#"macro unless { ($cond:expr) => { if !$cond { {} } }; }"#;
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        let item = stream.parse::<crate::Item>().unwrap();
        let crate::ItemKind::MacroDef(def) = item.kind else {
            panic!("expected MacroDef, got {:?}", item.kind);
        };
        assert_eq!(interner.resolve(&def.name.symbol), "unless");
        assert!(!def.body.is_empty());
    }

    #[test]
    fn macro_def_requires_name() {
        let mut interner = Interner::new();
        let src = r#"macro { }"#;
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        assert!(stream.parse::<crate::Item>().is_err());
    }

    #[test]
    fn macro_def_requires_brace_body() {
        let mut interner = Interner::new();
        let src = r#"macro unless;"#;
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        assert!(stream.parse::<crate::Item>().is_err());
    }
}
