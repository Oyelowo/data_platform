/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerError, CharLexerResult, ParseChars, ParseTokenStream, TokenError,
    TokenResult, TokenStream, TokenTrait,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Any;

impl ParseChars for Any {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        match cursor.peek() {
            Some(_) => {
                cursor.advance();
                Ok(Any)
            }
            None => Err(CharLexerError::UnexpectedEof {
                expected: "Any".to_string(),
                span: cursor.current_span(),
            }),
        }
    }
}

impl<T: TokenTrait> ParseTokenStream<T> for Any {
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        match tokenstream.peek() {
            Some(_) => {
                tokenstream.advance();
                Ok(Any)
            }
            None => Err(TokenError::UnexpectedEof {
                expected: "Any".to_string(),
                span: tokenstream.current_span(),
            }),
        }
    }
}
