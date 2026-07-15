/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use crate::*;
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream, Verify};

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

/// Statements in the language
///
/// Represents executable statements including declarations, control flow,
/// and standalone expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// Expression without semicolon - value is used
    ///
    /// Returns a value from the statement
    ///
    /// # Example
    /// ```
    /// {
    ///     let x = 42;
    ///     x + 1  // expression statement
    /// }
    /// ```
    Expr(Box<Expr>),

    /// Expression with semicolon - value is discarded
    ///
    /// # Example
    /// ```
    /// {
    ///     let x = 42;
    ///     x + 1;  // semi statement
    /// }
    /// ```
    TermExpr(Box<Expr>),

    /// Local binding: `let x = 5;`
    ///
    /// # Example
    /// ```
    /// let x = 42;
    /// let mut y = 10;
    /// let (a, b) = (1, 2);
    /// ```
    Let(Box<LetStmt>),

    /// Item declaration within block
    ///
    /// Allows nested items like functions, types, etc.
    ///
    /// # Example
    /// ```
    /// {
    ///     fn helper() { ... }
    ///     struct Point { x: i32, y: i32 }
    /// }
    /// ```
    Item(Box<Item>),

    /// Just a trailing semi-colon.
    Empty,

    /// Macro invocation in statement position: `foo! { ... }`.
    ///
    /// Unlike `Expr(MacroInvocation(...))`, this can expand to statements,
    /// items, or multiple statements.
    MacroInvocation(crate::expr::MacroInvocation),
}

impl Stmt {
    pub fn span(&self) -> yelang_lexer::Span {
        self.span
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Stmt {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // Empty statement `;`
        if stream.parse::<Option<T![;]>>()?.is_some() {
            let span = stream.span_since(checkpoint);
            return Ok(Stmt {
                kind: StmtKind::Empty,
                span,
            });
        }

        // If it starts with `let`, it must be a let-statement.
        // Do not use `Option<LetStmt>` here: `Option<T>` intentionally swallows parse errors,
        // which would hide real syntax issues like a missing trailing `;`.
        if stream.parse::<Verify<T![let]>>().is_ok() {
            let let_stmt = stream.parse::<LetStmt>()?;
            let span = stream.span_since(checkpoint);
            return Ok(Stmt {
                kind: StmtKind::Let(Box::new(let_stmt)),
                span,
            });
        }

        // If it looks like an item, parse as an item and surface errors.
        // Keep this check conservative: some keywords (e.g. `async`) can start expressions.
        let looks_like_item = match stream.peek().map(|t| t.kind()) {
            Some(TokenKind::Async) => matches!(
                stream.peek_ahead(1).map(|t| t.kind()),
                // `async fn ...` item
                Some(TokenKind::Fn)
            ),
            Some(
                TokenKind::Fn
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Trait
                | TokenKind::Impl
                | TokenKind::Mod
                | TokenKind::Use
                | TokenKind::TypeToken
                | TokenKind::Const
                | TokenKind::Static
                | TokenKind::Pub,
            ) => true,
            _ => false,
        };

        if looks_like_item {
            let item = stream.parse::<Item>()?;
            let span = stream.span_since(checkpoint);
            return Ok(Stmt {
                kind: StmtKind::Item(Box::new(item)),
                span,
            });
        }

        // Statement-position macro invocation with `{}`: `foo! { ... }`.
        // Parentheses and square brackets are always parsed as expressions
        // (per RFC 378), so only curly braces get the statement-macro node.
        let macro_checkpoint = stream.checkpoint();
        if let Ok(path) = stream.parse::<Path>() {
            if stream.peek().map(|t| t.kind()) == Some(&TokenKind::Bang) {
                if stream
                    .peek_ahead(1)
                    .is_some_and(|t| matches!(t.kind(), TokenKind::OpenBrace))
                {
                    stream.advance(); // consume `!`
                    let args = crate::expr::parse_macro_args(stream)?;
                    let span = stream.span_since(checkpoint);
                    return Ok(Stmt {
                        kind: StmtKind::MacroInvocation(MacroInvocation { path, args, span }),
                        span,
                    });
                }
            }
        }
        stream.restore(macro_checkpoint);

        // Parse as expression (queries are expressions, so they'll be parsed here)
        let expr = stream.parse::<Expr>()?;
        let span = stream.span_since(checkpoint);
        let kind = if stream.parse::<Option<T![;]>>()?.is_some() {
            StmtKind::TermExpr(Box::new(expr))
        } else {
            StmtKind::Expr(Box::new(expr))
        };

        Ok(Stmt { kind, span })
    }
}
