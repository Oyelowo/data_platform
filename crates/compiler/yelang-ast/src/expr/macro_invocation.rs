/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 13/07/2025
 */

use crate::{Expr, Ident, Path, Stmt, T};
use yelang_lexer::{ArrayCreator, ParseTokenStream, TokenResult, TokenStream};

/// Macro invocation expression: `assert!(true)`, `vec![1, 2, 3]`, `macro! { stmt }`
///
/// Parsed when a path is immediately followed by `!` and a delimited group.
/// This is distinct from a function call to allow compile-time expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroInvocation {
    pub path: Path,
    pub args: MacroArgs,
    pub span: yelang_lexer::Span,
}

/// Arguments to a macro invocation, preserving the delimiter kind.
#[derive(Debug, Clone, PartialEq)]
pub enum MacroArgs {
    /// `foo(a, b)` — parenthesized arguments
    Paren(Vec<Expr>),
    /// `bar[a, b]` — bracketed arguments
    Bracket(Vec<Expr>),
    /// `baz { stmt; stmt }` — braced arguments (statements)
    Brace(Vec<Stmt>),
}

impl MacroArgs {
    pub fn span(&self) -> yelang_lexer::Span {
        match self {
            MacroArgs::Paren(exprs) => exprs.first().map(|e| e.span).unwrap_or_default(),
            MacroArgs::Bracket(exprs) => exprs.first().map(|e| e.span).unwrap_or_default(),
            MacroArgs::Brace(stmts) => stmts.first().map(|s| s.span).unwrap_or_default(),
        }
    }
}

impl MacroInvocation {
    pub fn name(&self, interner: &crate::Interner) -> Option<String> {
        if self.path.segments.len() == 1 {
            Some(interner.resolve(&self.path.segments[0].ident.symbol).to_string())
        } else {
            None
        }
    }
}

/// Parse macro invocation arguments after the `!` token has been consumed.
pub fn parse_macro_args(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<MacroArgs> {
    use crate::tokenizer::TokenKind;

    match stream.peek().map(|t| t.kind()) {
        Some(TokenKind::OpenParen) => {
            type ParenArgs = ArrayCreator<T!['('], Expr, T![,], T![')']>;
            let args = stream.parse::<ParenArgs>()?;
            Ok(MacroArgs::Paren(args.items_owned()))
        }
        Some(TokenKind::OpenBracket) => {
            type BracketArgs = ArrayCreator<T!['['], Expr, T![,], T![']']>;
            let args = stream.parse::<BracketArgs>()?;
            Ok(MacroArgs::Bracket(args.items_owned()))
        }
        Some(TokenKind::OpenBrace) => {
            type BraceArgs = ArrayCreator<T!['{'], Stmt, T![;], T!['}']>;
            let args = stream.parse::<BraceArgs>()?;
            Ok(MacroArgs::Brace(args.items_owned()))
        }
        Some(other) => Err(yelang_lexer::TokenError::UnexpectedToken {
            expected: "`(` or `[` or `{` after `!`".to_string(),
            found: other.to_string(),
            span: stream.current_span(),
        }),
        None => Err(yelang_lexer::TokenError::UnexpectedEof {
            expected: "`(` or `[` or `{` after `!`".to_string(),
            span: stream.current_span(),
        }),
    }
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

        // Parse the expression; it should be a MacroInvocation
        let expr = stream.parse::<Expr>().unwrap();
        let ExprKind::MacroInvocation(inv) = expr.kind else {
            panic!("expected MacroInvocation, got {:?}", expr.kind);
        };
        assert_eq!(inv.name(&interner), Some("assert".to_string()));
        let MacroArgs::Paren(args) = inv.args else {
            panic!("expected Paren args, got {:?}", inv.args);
        };
        assert_eq!(args.len(), 1);
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
        let MacroArgs::Bracket(args) = inv.args else {
            panic!("expected Bracket args, got {:?}", inv.args);
        };
        assert_eq!(args.len(), 2);
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
}
