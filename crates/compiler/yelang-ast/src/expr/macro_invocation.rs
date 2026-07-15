/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 13/07/2025
 */

use crate::token::{TokenStream as MacroTokenStream, convert::from_lexer_tokens};
use crate::{Expr, Ident, Path, Stmt, T};
use yelang_lexer::{ArrayCreator, ParseTokenStream, Token, TokenResult, TokenStream};

/// Macro invocation expression: `assert!(true)`, `vec![1, 2, 3]`, `macro! { stmt }`
///
/// Parsed when a path is immediately followed by `!` and a delimited group.
/// This is distinct from a function call to allow compile-time expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroInvocation {
    pub path: Path,
    /// Raw token stream of the macro arguments, including the delimiting group.
    pub args: MacroTokenStream,
    pub span: yelang_lexer::Span,
}

impl MacroInvocation {
    pub fn name(&self, interner: &crate::Interner) -> Option<String> {
        if self.path.segments.len() == 1 {
            Some(
                interner
                    .resolve(&self.path.segments[0].ident.symbol)
                    .to_string(),
            )
        } else {
            None
        }
    }
}

/// Parse macro invocation arguments after the `!` token has been consumed.
///
/// The delimited lexer tokens are preserved as a macro `TokenStream` so that
/// the macro expander can operate on the raw tokens hygienically.
pub fn parse_macro_args(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<MacroTokenStream> {
    use crate::tokenizer::TokenKind;

    let current_span = stream.current_span();
    let open_kind =
        stream
            .peek()
            .map(|t| t.kind().clone())
            .ok_or(yelang_lexer::TokenError::UnexpectedEof {
                expected: "`(` or `[` or `{` after `!`".to_string(),
                span: current_span,
            })?;

    let (open_kind, close_kind) = match open_kind {
        TokenKind::OpenParen => (TokenKind::OpenParen, TokenKind::CloseParen),
        TokenKind::OpenBracket => (TokenKind::OpenBracket, TokenKind::CloseBracket),
        TokenKind::OpenBrace => (TokenKind::OpenBrace, TokenKind::CloseBrace),
        ref other => {
            return Err(yelang_lexer::TokenError::UnexpectedToken {
                expected: "`(` or `[` or `{` after `!`".to_string(),
                found: other.to_string(),
                span: stream.current_span(),
            });
        }
    };

    let tokens = consume_balanced(stream, open_kind, close_kind)?;
    Ok(from_lexer_tokens(&tokens))
}

/// Consume a balanced delimited sequence from the lexer stream, including the
/// opening and closing delimiter tokens.
fn consume_balanced(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
    open_kind: crate::tokenizer::TokenKind,
    close_kind: crate::tokenizer::TokenKind,
) -> TokenResult<Vec<Token<crate::tokenizer::TokenKind>>> {
    let mut depth = 0usize;
    let mut tokens = Vec::new();

    loop {
        let eof_span = stream.current_span();
        let token = stream
            .advance()
            .ok_or(yelang_lexer::TokenError::UnexpectedEof {
                expected: "matching macro argument delimiter".to_string(),
                span: eof_span,
            })?;

        if token.kind() == &open_kind {
            depth += 1;
        } else if token.kind() == &close_kind {
            if depth == 0 {
                // Unbalanced: saw a close before an open. Should not happen because
                // the caller verified the first token is an open delimiter.
                return Err(yelang_lexer::TokenError::UnexpectedToken {
                    expected: "macro argument".to_string(),
                    found: close_kind.to_string(),
                    span: stream.current_span(),
                });
            }
            depth -= 1;
        }

        tokens.push(token.clone());

        if depth == 0 {
            break;
        }
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExprKind, Interner, TokenKind};
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn macro_invocation_parses_paren_args() {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize("assert!(true)", &mut interner).unwrap();

        let expr = stream.parse::<Expr>().unwrap();
        let ExprKind::MacroInvocation(inv) = expr.kind else {
            panic!("expected MacroInvocation, got {:?}", expr.kind);
        };
        assert_eq!(inv.name(&interner), Some("assert".to_string()));
        assert!(!inv.args.is_empty());
    }

    #[test]
    fn macro_invocation_parses_bracket_args() {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize("vec![1, 2]", &mut interner).unwrap();

        let expr = stream.parse::<Expr>().unwrap();
        let ExprKind::MacroInvocation(inv) = expr.kind else {
            panic!("expected MacroInvocation, got {:?}", expr.kind);
        };
        assert_eq!(inv.name(&interner), Some("vec".to_string()));
        assert!(!inv.args.is_empty());
    }

    #[test]
    fn macro_invocation_parses_qualified_path() {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize("std::assert!(true)", &mut interner).unwrap();

        let expr = stream.parse::<Expr>().unwrap();
        let ExprKind::MacroInvocation(inv) = expr.kind else {
            panic!("expected MacroInvocation, got {:?}", expr.kind);
        };
        assert_eq!(inv.path.segments.len(), 2);
    }

    #[test]
    fn macro_invocation_preserves_compound_operators() {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize("assert!(x <= y)", &mut interner).unwrap();

        let expr = stream.parse::<Expr>().unwrap();
        let ExprKind::MacroInvocation(inv) = expr.kind else {
            panic!("expected MacroInvocation, got {:?}", expr.kind);
        };
        assert_eq!(inv.args.render(&interner), "(x<=y)");
    }

    #[test]
    fn macro_invocation_missing_args_errors() {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize("assert!", &mut interner).unwrap();
        assert!(stream.parse::<Expr>().is_err());
    }

    #[test]
    fn macro_invocation_invalid_delimiter_errors() {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize("assert!<>", &mut interner).unwrap();
        assert!(stream.parse::<Expr>().is_err());
    }
}
