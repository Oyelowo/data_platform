/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/11/2025
 */

use super::types::{Expr, ExprKind, Restrictions};
use crate::{
    ArrayAccess, ArrayIndex, AssignEqExpr, AssignOpExpr, Associativity, AtomicExpr, BinaryExpr,
    BindAtExpr, CallArgs, CallExpr, DestructureAssignExpr, Document, DocumentAccess,
    ExprPathSegment, Ident, InfixOp, IntegerLit, IsTypeExpr, LetExpr, MemberAccess, MethodCallExpr,
    Pattern, PatternKind, Precedence, PrecedenceExt, RangeExpr, RangeOp, T, TernaryExpr,
    TrySafeAccess, Type, TypeAscription, TypeCast,
};
use yelang_lexer::Span;
use yelang_lexer::{
    Either, OneOf9, ParseTokenStream, TokenError, TokenResult, TokenStream, Verify,
};

impl ParseTokenStream<crate::tokenizer::TokenKind> for Expr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        Self::parse_pratt(stream, Precedence::None, Restrictions::NONE)
    }
}

impl Expr {
    /// Returns true if this expression is block-like.
    ///
    /// Block-like expressions include:
    /// - Block expressions: `{ ... }`
    /// - If expressions: `if cond { ... }`
    /// - Match expressions: `match val { ... }`
    /// - Loop expressions: `loop { ... }`, `while cond { ... }`, `for x in iter { ... }`
    ///
    /// This is used to determine if a comma is required after a match arm.
    /// In Rust, commas are optional after block-like expressions but required after
    /// non-block expressions (except for the last arm).
    pub fn is_block_like(&self) -> bool {
        matches!(
            self.kind,
            ExprKind::Block(_)
                | ExprKind::If(_)
                | ExprKind::Match(_)
                | ExprKind::Loop(_)
                | ExprKind::While(_)
                | ExprKind::ForLoop(_)
        )
    }

    pub fn parse_pratt(
        stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        min_precedence: Precedence,
        restrictions: Restrictions,
    ) -> TokenResult<Expr> {
        if min_precedence <= Precedence::Assignment
            && let Some(expr) = Self::try_parse_destructure_assign(stream, restrictions)?
        {
            return Ok(expr);
        }

        let mut left = Self::parse_prefix(stream, restrictions)?;

        left = Self::parse_postfix(stream, left)?;

        loop {
            let op_checkpoint = stream.checkpoint();

            // In `<...>` generic-arg contexts, `>` is a closing delimiter.
            if restrictions.gt_is_delimiter && stream.parse::<Verify<T![>]>>().is_ok() {
                break;
            }

            // Try to parse any infix operator (assignment, range, or binary)
            let Ok(op) = stream.parse::<Verify<InfixOp>>() else {
                break;
            };
            let op_precedence = op.precedence();

            if op_precedence < min_precedence {
                break;
            }

            let op = stream.parse::<InfixOp>()?;
            let next_prec = match op.associativity() {
                Associativity::Left => op_precedence.increment(),
                Associativity::Right => op_precedence,
                Associativity::NonAssociative => op_precedence,
            };

            // Try to parse the right-hand side
            // If parsing fails, restore the checkpoint and stop parsing infix operators
            // This handles cases like "1024 >" in generic contexts where > is a closing delimiter
            let right = match Self::parse_pratt(stream, next_prec, restrictions) {
                Ok(r) => r,
                Err(_) => {
                    // Failed to parse RHS - restore and stop
                    stream.restore(op_checkpoint);
                    break;
                }
            };

            let span = stream.span_since(op_checkpoint);

            Self::parse_infix(&mut left, op, right, span);

            left = Self::parse_postfix(stream, left)?;
        }

        if stream.parse::<Verify<T![?]>>().is_ok() {
            left = Self::parse_ternary(stream, left)?;
        }

        Ok(left)
    }

    fn try_parse_destructure_assign(
        stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        restrictions: Restrictions,
    ) -> TokenResult<Option<Expr>> {
        let checkpoint = stream.checkpoint();
        let Ok(pattern) = stream.parse::<Pattern>() else {
            stream.restore(checkpoint);
            return Ok(None);
        };

        if !is_destructure_assignment_pattern(&pattern) {
            stream.restore(checkpoint);
            return Ok(None);
        }

        if stream.parse::<Option<T![=]>>()?.is_none() {
            stream.restore(checkpoint);
            return Ok(None);
        }

        let value = Self::parse_pratt(stream, Precedence::Assignment, restrictions)?;
        let span = stream.span_since(checkpoint);
        Ok(Some(Expr {
            kind: ExprKind::DestructureAssign(DestructureAssignExpr {
                pattern,
                value: Box::new(value),
            }),
            span,
        }))
    }

    fn parse_infix(left: &mut Expr, op: InfixOp, right: Expr, span: Span) {
        let old_left = std::mem::replace(
            left,
            Expr {
                kind: ExprKind::Underscore,
                span,
            },
        );
        *left = match op {
            InfixOp::Binary(binary_op) => Expr {
                kind: ExprKind::Binary(BinaryExpr {
                    left: Box::new(old_left),
                    op: binary_op,
                    right: Box::new(right),
                }),
                span,
            },
            InfixOp::AssignEq => Expr {
                kind: ExprKind::AssignEq(AssignEqExpr {
                    target: Box::new(old_left),
                    value: Box::new(right),
                }),
                span,
            },
            InfixOp::AssignOp(assign_op) => Expr {
                kind: ExprKind::AssignOp(AssignOpExpr {
                    target: Box::new(old_left),
                    op: assign_op,
                    value: Box::new(right),
                }),
                span,
            },
            InfixOp::Range(op) => Expr {
                kind: ExprKind::Range(RangeExpr {
                    start: Some(Box::new(old_left)),
                    op,
                    end: Some(Box::new(right)),
                }),
                span,
            },
        };
    }

    fn parse_ternary(
        stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        left: Expr,
    ) -> TokenResult<Expr> {
        let ((_, if_true, _, if_false), span) =
            stream.parse_with_span::<(T![?], Expr, T![:], Expr)>()?;

        Ok(Expr {
            kind: ExprKind::Ternary(TernaryExpr {
                condition: Box::new(left),
                if_true: Box::new(if_true),
                if_false: Box::new(if_false),
            }),
            span,
        })
    }

    fn parse_prefix(
        stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        restrictions: Restrictions,
    ) -> TokenResult<Expr> {
        // Check for let expressions first (only valid in if/while conditions)
        let checkpoint = stream.checkpoint();
        if stream.parse::<Verify<T![let]>>().is_ok() {
            stream.restore(checkpoint);
            let checkpoint = stream.checkpoint();
            let let_expr = stream.parse::<LetExpr>()?;
            let span = stream.span_since(checkpoint);
            return Ok(Expr {
                kind: ExprKind::Let(let_expr),
                span,
            });
        }
        stream.restore(checkpoint);

        // Check for prefix ranges
        let prefix_range_result = Self::try_parse_prefix_range(stream, restrictions);
        if let Some(expr) = prefix_range_result? {
            return Ok(expr);
        }

        // Fall back to regular atomic expressions
        let expr = AtomicExpr::parse_with_restrictions(stream, restrictions)?.as_expr();
        Ok(expr)
    }

    fn parse_postfix(
        stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        mut base: Expr,
    ) -> TokenResult<Expr> {
        type PostfixOp = OneOf9<
            T![?],
            (T![@], Ident),
            Either<(T![as], Type), Either<(T![is], Type), (T![:], Type)>>,
            (T![.], T![await]),
            (T![.], Document),
            (T![.], ExprPathSegment, CallArgs),
            // Member access: `obj.field` OR Rust-style tuple fields: `t.0`, `t.1`, ...
            // For numeric tuple fields, we keep the numeric member name (`t.0`) so later phases
            // can recognize it as a tuple-field access (rather than a struct field).
            (T![.], Either<IntegerLit, Ident>),
            ArrayIndex,
            CallArgs,
        >;

        loop {
            let checkpoint = stream.checkpoint();
            let Ok(op) = stream.parse::<PostfixOp>() else {
                break;
            };

            let kind = match op {
                OneOf9::_1(op) => ExprKind::Try(TrySafeAccess {
                    base: Box::new(base),
                    op,
                }),
                OneOf9::_2((_, as_)) => ExprKind::BindAt(BindAtExpr {
                    base: Box::new(base),
                    at: as_,
                }),
                OneOf9::_3(either) => match either {
                    Either::Left((_, ty)) => ExprKind::TypeCast(TypeCast {
                        base: Box::new(base),
                        ty,
                    }),
                    Either::Right(Either::Left((_, ty))) => ExprKind::IsType(IsTypeExpr {
                        expr: Box::new(base),
                        ty,
                    }),
                    Either::Right(Either::Right((_, ty))) => {
                        ExprKind::TypeAscription(TypeAscription {
                            expr: Box::new(base),
                            ty,
                        })
                    }
                },
                OneOf9::_4((_, _)) => ExprKind::Await(Box::new(base)),
                OneOf9::_5((_, object)) => ExprKind::DocumentAccess(DocumentAccess {
                    base: Box::new(base),
                    object,
                }),
                OneOf9::_6((_, method, args)) => ExprKind::MethodCall(MethodCallExpr {
                    receiver: Box::new(base),
                    segment: method.0,
                    arguments: args.items_owned(),
                }),
                OneOf9::_7((_, member)) => match member {
                    Either::Left(int_lit) => {
                        if int_lit.suffix.is_some() {
                            return Err(TokenError::CustomError {
                                msg: "Tuple field access index cannot have a type suffix".into(),
                                span: stream.span_since(checkpoint),
                            });
                        }

                        let raw = stream.interner().resolve(&int_lit.value);
                        if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
                            return Err(TokenError::CustomError {
                                msg: "Tuple field access index must be an unsigned integer".into(),
                                span: stream.span_since(checkpoint),
                            });
                        }

                        let member_symbol = stream.interner().get_or_intern(raw);
                        ExprKind::MemberAccess(MemberAccess {
                            base: Box::new(base),
                            member: Ident::new(member_symbol, stream.span_since(checkpoint)),
                        })
                    }
                    Either::Right(member) => ExprKind::MemberAccess(MemberAccess {
                        base: Box::new(base),
                        member,
                    }),
                },
                OneOf9::_8(index) => ExprKind::ArrayAccess(ArrayAccess {
                    base: Box::new(base),
                    index,
                }),
                OneOf9::_9(args) => ExprKind::Call(CallExpr {
                    callee: Box::new(base),
                    args: args.items_owned(),
                }),
            };
            base = Expr {
                kind,
                span: stream.span_since(checkpoint),
            };
        }
        Ok(base)
    }

    /// Parse prefix ranges: ..end and ..=end
    fn try_parse_prefix_range(
        stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        restrictions: Restrictions,
    ) -> TokenResult<Option<Expr>> {
        let checkpoint = stream.checkpoint();
        let Ok(op) = stream.parse::<RangeOp>() else {
            return Ok(None);
        };

        // Must have a right-hand side for prefix ranges
        let end = Self::parse_pratt(stream, Precedence::Range, restrictions)?;

        let span = stream.span_since(checkpoint);

        Ok(Some(Expr {
            kind: ExprKind::Range(RangeExpr {
                start: None,
                op,
                end: Some(Box::new(end)),
            }),
            span,
        }))
    }
}

fn is_destructure_assignment_pattern(pattern: &Pattern) -> bool {
    match &pattern.pattern {
        PatternKind::Tuple { patterns } => patterns.len() != 1,
        PatternKind::Struct { .. }
        | PatternKind::Record { .. }
        | PatternKind::TupleStruct { .. }
        | PatternKind::Slice { .. } => true,
        PatternKind::Grouped(inner) => is_destructure_assignment_pattern(inner),
        PatternKind::Binding {
            subpattern: Some(subpattern),
            ..
        } => is_destructure_assignment_pattern(subpattern),
        _ => false,
    }
}

#[cfg(test)]
mod destructure_assignment_tests {
    use crate::tokenizer::TokenKind;
    use crate::{Expr, ExprKind, Interner, PatternKind};

    #[test]
    fn structural_record_assignment_parses_as_destructure_assignment() {
        let input = "{ index, value: user } = row";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let expr = stream.parse::<Expr>().unwrap();

        let ExprKind::DestructureAssign(assign) = expr.kind else {
            panic!("expected destructuring assignment, got {expr:?}");
        };

        let PatternKind::Record { fields, rest } = assign.pattern.pattern else {
            panic!("expected record pattern, got {:?}", assign.pattern);
        };
        assert_eq!(fields.len(), 2);
        assert!(!rest);
    }

    #[test]
    fn simple_assignment_stays_regular_assignment() {
        let input = "user = row";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let expr = stream.parse::<Expr>().unwrap();

        assert!(matches!(expr.kind, ExprKind::AssignEq(_)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::TokenKind;

    #[test]
    fn contextual_identifier_method_call_parses_as_method_call() {
        let mut interner = crate::Interner::new();
        let mut stream =
            TokenKind::tokenize("[7, 8].enumerate()", &mut interner).expect("tokenize");

        let expr = Expr::parse(&mut stream).expect("parse expr");
        let ExprKind::MethodCall(method_call) = expr.kind else {
            panic!("expected method call, got {:?}", expr.kind);
        };

        assert_eq!(
            interner.resolve(&method_call.segment.ident.symbol),
            "enumerate"
        );
        assert!(method_call.arguments.is_empty());
    }
}
