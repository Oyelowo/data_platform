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

pub struct Eof;

impl ParseChars for Eof {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        if cursor.peek().is_none() {
            Ok(Eof)
        } else {
            Err(CharLexerError::UnexpectedChar {
                expected: "EOF".to_string(),
                found: cursor.peek().unwrap(),
                span: cursor.current_span(),
            })
        }
    }
}

impl<T: TokenTrait> ParseTokenStream<T> for Eof {
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        if tokenstream.is_eof() {
            Ok(Eof)
        } else {
            Err(TokenError::UnexpectedEof {
                expected: "EOF".to_string(),
                span: tokenstream.current_span(),
            })
        }
    }
}
