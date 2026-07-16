/*
 * Copyright (c) 2025 Oyelowo Oyedayo. All Rights Reserved.
 *
 * This software is the proprietary and confidential information of Oyelowo Oyedayo.
 * You shall not disclose, distribute, modify, or reproduce this software in any form,
 * in whole or in part, without the prior written consent of Oyelowo Oyedayo.
 *
 * For inquiries, contact: oyelowo.oss@gmail.com
 */

use crate::{Literal, T, Type};
use yelang_lexer::{ParseTokenStream, SeparatedList, TokenResult, TokenStream};

/// Parse an optional `extern "ABI"` prefix and return the ABI string.
fn parse_optional_abi(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<Option<String>> {
    let checkpoint = stream.checkpoint();
    if stream.parse::<Option<T![extern]>>()?.is_none() {
        return Ok(None);
    }

    let lit = stream.parse::<Literal>()?;
    if let Literal::Str(s) = lit {
        Ok(Some(stream.interner().resolve(&s.value).to_string()))
    } else {
        Err(yelang_lexer::TokenError::UnexpectedToken {
            expected: "string literal ABI".into(),
            found: "non-string literal".into(),
            span: stream.span_since(checkpoint),
        })
    }
}

/// Function type signature
///
/// # Example
/// ```
/// fn(i32, i32) -> i32
/// fn(string, ...) -> bool  // variadic
/// extern "C" fn(i32) -> i32
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    /// Optional ABI string for `extern "ABI" fn` pointer types.
    pub abi: Option<String>,
    /// Whether the function type is async
    pub is_async: bool,
    /// Parameter types
    pub params: Vec<Type>,
    /// Return type
    pub return_type: Box<Type>,
    /// Whether the function accepts variable arguments
    pub is_variadic: bool,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for FunctionType {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let abi = parse_optional_abi(stream)?;
        let (is_async, _fn, _opa, params, _cpa, _arrow, return_ty) = stream.parse::<(
            Option<T![async]>,
            T![fn],
            T!['('],
            SeparatedList<Type, T![,], true>,
            T![')'],
            T![->],
            Type,
        )>()?;

        let res = Self {
            abi,
            is_async: is_async.is_some(),
            params: params.value_owned(),
            return_type: Box::new(return_ty),
            is_variadic: false,
        };
        Ok(res)
    }
}
