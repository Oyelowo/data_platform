/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */
use super::{
    Array, BlockExpr, BreakExpr, ContinueExpr, Expr, ForLoopExpr, GroupedExpr, IfExpr, LambdaExpr,
    Literal, LoopExpr, Object, TupleExpr, UnaryExpr, WhileExpr,
};
use crate::expr::{
    list_comp::ComprehensionExpr, struct_expr::FieldAssign, template::InterpolatedStringExpr,
};
use crate::{AsyncExpr, ExprKind, MatchExpr, Query, StructExpr, T, tokens::TokenKind};
use yelang_lexer::{ParseTokenStream, SeparatedList, TokenResult, TokenStream, Verify, match_map};

#[derive(Debug, Clone)]
pub struct AtomicExpr(Expr);

impl AtomicExpr {
    pub fn new(expr: Expr) -> Self {
        Self(expr)
    }

    pub fn as_expr(self) -> Expr {
        self.0
    }

    pub fn as_expr_ref(&self) -> &Expr {
        &self.0
    }

    pub fn as_expr_mut(&mut self) -> &mut Expr {
        &mut self.0
    }
}

impl AtomicExpr {
    pub fn parse_with_restrictions(
        stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        restrictions: super::Restrictions,
    ) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // Queries are keyword-led constructs (select/create/...). If a query keyword is present,
        // we must parse it deterministically and propagate any diagnostics instead of
        // backtracking into unrelated atomic-expression alternatives.
        if stream.parse::<Verify<T![select]>>().is_ok()
            || stream.parse::<Verify<T![create]>>().is_ok()
            || stream.parse::<Verify<T![update]>>().is_ok()
            || stream.parse::<Verify<T![upsert]>>().is_ok()
            || stream.parse::<Verify<T![link]>>().is_ok()
            || stream.parse::<Verify<T![unlink]>>().is_ok()
            || stream.parse::<Verify<T![delete]>>().is_ok()
        {
            let query = stream.parse::<Query>()?;
            let span = stream.span_since(checkpoint);
            return Ok(Self(Expr {
                kind: ExprKind::Query(Box::new(query)),
                span,
            }));
        }

        if stream
            .peek()
            .is_some_and(|token| token.kind() == &TokenKind::OpenBrace)
        {
            let object_checkpoint = stream.checkpoint();
            if let Ok(object) = stream.parse::<Object>() {
                if !object.fields.is_empty() {
                    let span = stream.span_since(checkpoint);
                    return Ok(Self(Expr {
                        kind: ExprKind::Object(object),
                        span,
                    }));
                }
            }
            stream.restore(object_checkpoint);
        }

        if stream
            .peek()
            .is_some_and(|token| token.kind() == &TokenKind::Async)
        {
            let lambda_checkpoint = stream.checkpoint();
            if let Ok(lambda) = stream.parse::<LambdaExpr>() {
                let span = stream.span_since(checkpoint);
                return Ok(Self(Expr {
                    kind: ExprKind::Lambda(lambda),
                    span,
                }));
            }
            stream.restore(lambda_checkpoint);
        }

        // First try all non-path-based expressions
        let atomic = match_map!(
            stream,
            MatchExpr => |ma| ExprKind::Match(Box::new(ma)),
            LoopExpr => |l| ExprKind::Loop(Box::new(l)),
            WhileExpr => ExprKind::While,
            ForLoopExpr => ExprKind::ForLoop,
            IfExpr => ExprKind::If,
            Array => ExprKind::Array,
            LambdaExpr => ExprKind::Lambda,
            UnaryExpr => ExprKind::Unary,
            Literal => ExprKind::Literal,
            InterpolatedStringExpr => |is| ExprKind::InterpolatedString(is.0),
            GroupedExpr => ExprKind::Grouped,
            TupleExpr => |t| ExprKind::Tuple(t.0),
            ComprehensionExpr => ExprKind::Comprehension,
            BreakExpr => ExprKind::Break,
            (T![return], Option<Expr>) => |(_b, expr)| ExprKind::Return(expr.map(Box::new)),
            ContinueExpr => ExprKind::Continue,
            AsyncExpr => ExprKind::Async,
            (T![gen], Expr) => |(_g, expr)| ExprKind::Gen(Box::new(expr)),
            BlockExpr => ExprKind::Block,
            T!["_"] => |_| ExprKind::Underscore,
        );

        // If we matched something non-path-based, return it
        if let Ok(kind) = atomic {
            let span = stream.span_since(checkpoint);
            return Ok(Self(Expr { kind, span }));
        }

        // Now handle path-based expressions with struct literal disambiguation
        // `null` is a reserved keyword but not a supported value at the language surface.
        if let Some(tok) = stream.peek() {
            if tok.kind() == &TokenKind::Null {
                return Err(yelang_lexer::TokenError::CustomError {
                    msg: "`null` is not supported. Use `()` (unit) or a domain-specific value."
                        .into(),
                    span: tok.span(),
                });
            }
        }

        // Parse Path, then check if `{` follows and restrictions allow struct literals
        let path = stream.parse::<super::path::ExprPath>()?.0;

        // Check if `{` follows and if we're allowed to parse struct literals
        let peek_brace = stream
            .peek()
            .map(|t| t.kind() == &TokenKind::OpenBrace)
            .unwrap_or(false);
        if peek_brace && !restrictions.forbid_structs {
            // Try to parse as StructExpr: Path { fields }
            let struct_checkpoint = stream.checkpoint();
            type StructFields = (
                T!['{'],
                Option<SeparatedList<FieldAssign, T![,], false>>,
                Option<(T![,], T![..], Expr)>,
                Option<T![,]>,
                T!['}'],
            );

            if let Ok((_ob, fields, rest, _trailing, _cb)) = stream.parse::<StructFields>() {
                let span = stream.span_since(checkpoint);
                return Ok(Self(Expr {
                    kind: ExprKind::Struct(StructExpr {
                        path,
                        fields: fields.map(|f| f.items()).unwrap_or_default(),
                        rest: rest.map(|(_comma, _dd, expr)| Box::new(expr)),
                    }),
                    span,
                }));
            }
            // If struct parsing failed, restore and treat as plain Path
            stream.restore(struct_checkpoint);
        }

        // Just a path expression
        let span = stream.span_since(checkpoint);
        Ok(Self(Expr {
            kind: ExprKind::Path(path),
            span,
        }))
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AtomicExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        Self::parse_with_restrictions(stream, super::Restrictions::NONE)
    }
}

impl From<AtomicExpr> for Expr {
    fn from(value: AtomicExpr) -> Self {
        value.0
    }
}
