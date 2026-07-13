/*
 * Copyright (c) 2025 Oyelowo Oyedayo. All Rights Reserved.
 *
 * This software is the proprietary and confidential information of Oyelowo Oyedayo.
 * You shall not disclose, distribute, modify, or reproduce this software in any form,
 * in whole or in part, without the prior written consent of Oyelowo Oyedayo.
 *
 * For inquiries, contact: oyelowo.oss@gmail.com
 */

use crate::{T, Type};
use yelang_lexer::{ParseTokenStream, SeparatedList, TokenResult, TokenStream};

/// Function type signature
///
/// # Example
/// ```
/// fn(i32, i32) -> i32
/// fn(string, ...) -> bool  // variadic
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
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
            is_async: is_async.is_some(),
            params: params.value_owned(),
            return_type: Box::new(return_ty),
            is_variadic: false,
        };
        Ok(res)
    }
}
