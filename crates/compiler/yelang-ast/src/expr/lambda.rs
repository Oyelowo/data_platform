/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/03/2025
 */

use crate::pattern::RestrictedPattern;
use crate::{Expr, FnRefType, FnSig, Param, T, Type, TypeAtom, TypeKind};
use yelang_lexer::{ParseTokenStream, SeparatedList, Span, TokenError, TokenResult, TokenStream};

// (x) => x * x
// |x| x * x
// (x, y) => x + y
// |x, y| x + y
// () => 42
// || 42
// user.blogs@b[*].tags@t[*].map((t) => t.name.upper())
// user.blogs@b[*].tags@t[*].map(|t| t.name.upper())
#[derive(Debug, Clone, PartialEq)]
pub struct LambdaExpr {
    /// Span covering the `|params|` header (using token spans, not cumulative stream spans).
    ///
    /// This is used as the stable key for the synthetic closure DefId during resolution/lowering,
    /// because `Expr::span` is derived from a cumulative stream span and can collide with nested
    /// sub-expressions that end at the same token.
    pub header_span: Span,
    pub fn_sig: FnSig,
    pub body: Box<Expr>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for LambdaExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let is_async = if stream
            .peek()
            .is_some_and(|token| token.kind() == &crate::tokenizer::TokenKind::Async)
        {
            let checkpoint = stream.checkpoint();
            stream.parse::<T![async]>()?;
            if stream
                .peek()
                .is_some_and(|token| token.kind() == &crate::tokenizer::TokenKind::Pipe)
            {
                true
            } else {
                stream.restore(checkpoint);
                false
            }
        } else {
            false
        };

        // Check for empty closure parameters: || { }
        // let checkpoint = stream.checkpoint();
        // if let Ok((T![|], T![|])) = stream.parse::<(T![|], T![|])>() {
        //     // Empty parameters - parse return type and body
        //     let return_type_opt = stream.parse::<Option<(T![->], Type)>>()?;
        //     let body_expr = stream.parse::<Expr>()?;

        //     let fn_sig = FnSig {
        //         params: vec![],
        //         return_type: return_type_opt
        //             .map_or(FnRefType::Default(stream.span()), |(_arrow, ty)| {
        //                 FnRefType::Type(ty)
        //             }),
        //         is_async: false,
        //         is_variadic: false,
        //     };

        //     return Ok(Self {
        //         fn_sig,
        //         body: Box::new(body_expr),
        //     });
        // }

        // Not empty params, restore and try normal parsing
        // stream.restore(checkpoint);

        // Use RestrictedPattern to ensure we don't consume the closing `|` as an OR operator.
        //
        // IMPORTANT: capture the `|params|` header span using token spans (via `peek().span()`),
        // not `stream.span_since`, because `TokenStream::current_span` is cumulative.
        type ParamTy = (RestrictedPattern, Option<(T![:], TypeAtom)>);

        let open_pipe_span = stream
            .peek()
            .map(|t| t.span())
            .ok_or(TokenError::UnexpectedEof {
                expected: "|".to_string(),
                span: stream.current_span(),
            })?;
        stream.parse::<T![|]>()?;

        let param_list = stream.parse::<Option<SeparatedList<ParamTy, T![,], true>>>()?;

        let close_pipe_span = stream
            .peek()
            .map(|t| t.span())
            .ok_or(TokenError::UnexpectedEof {
                expected: "|".to_string(),
                span: stream.current_span(),
            })?;
        stream.parse::<T![|]>()?;

        let header_span = open_pipe_span.merge(close_pipe_span);

        let return_type_opt = stream.parse::<Option<(T![->], Type)>>()?;
        let body_expr = stream.parse::<Expr>()?;

        let fn_sig = FnSig {
            params: param_list
                .map(|l| l.value_owned())
                .unwrap_or_default()
                .into_iter()
                .map(|(pat, type_opt)| {
                    let default_ty = Type {
                        kind: TypeKind::Infer,
                        span: stream.current_span(),
                    };
                    let ty = type_opt.map_or(default_ty, |(_c, ty)| ty.0);
                    Param {
                        span: *pat.0.span(),
                        ty,
                        pattern: pat.0,
                    }
                })
                .collect::<Vec<_>>(),
            return_type: return_type_opt
                .map_or(FnRefType::Default(stream.span()), |(_arrow, ty)| {
                    FnRefType::Type(ty)
                }),
            is_async,
            is_variadic: false,
        };

        Ok(Self {
            header_span,
            fn_sig,
            body: Box::new(body_expr),
        })
    }
}

impl LambdaExpr {}
