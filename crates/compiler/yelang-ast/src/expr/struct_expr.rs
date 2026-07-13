/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */

use crate::{Expr, ExprKind, Ident, Path, T, TokenKind};
use yelang_lexer::{Either, ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream};

/// Named struct normal struct: `User { id: 1, name: "John" }`
/// Named struct for enum: `Enum::User { id: 1, name: "John" }`
///
/// # Example
/// ```
/// let user = User {
///     id: 1,
///     name: "John",
///     email: "john@example.com"
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct StructExpr {
    pub path: Path,
    pub fields: Vec<FieldAssign>,
    pub rest: Option<Box<Expr>>, // NEW: Handle ..base and .. patterns
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for StructExpr {
    fn parse(stream: &mut TokenStream<TokenKind>) -> TokenResult<Self> {
        // FIX: Require '..' before the rest expression to avoid ambiguity
        // with for-loop bodies: `for x in items { if ... }` should not be
        // parsed as `items { if ... }` (struct literal with implicit rest).
        //
        // Valid forms:
        // - `MyStruct { field1: val1, field2: val2 }` - no rest
        // - `MyStruct { field1: val1, ..base }` - with rest
        // Invalid form (was causing bug):
        // - `MyStruct { some_expr }` - ambiguous, could be for-loop body
        type Named = (
            Path,
            T!['{'],
            Option<SeparatedList<FieldAssign, T![,], false>>,
            Option<(T![,], T![..], Expr)>,
            Option<T![,]>,
            T!['}'],
        );
        let (path, _op_br, fields, rest, _trailing, _) = stream.parse::<Named>()?;

        Ok(Self {
            path,
            fields: fields.map(|f| f.items()).unwrap_or_default(),
            rest: rest.map(|(_comma, _dd, expr)| Box::new(expr)),
        })
    }
}

/// Field assignment in struct literal
///
/// # Example
/// ```
/// id: 1
/// name: "John"
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FieldAssign {
    /// Field name
    pub name: Ident,
    /// Field value
    pub value: Expr,
    pub is_shorthand: bool,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for FieldAssign {
    fn parse(stream: &mut TokenStream<TokenKind>) -> TokenResult<Self> {
        let ((name, (value)), span) = stream.parse_with_span::<(Ident, Option<(T![:], Expr)>)>()?;
        let is_shorthand = value.is_none();

        Ok(FieldAssign {
            name,
            value: value.map(|(_, expr)| expr).unwrap_or(Expr {
                kind: ExprKind::Path(Path::new_single_ident(name)),
                span: name.span,
            }),
            is_shorthand,
            span,
        })
    }
}

impl StructExpr {}

impl FieldAssign {}
