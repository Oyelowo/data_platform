/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::CharLexerResult;
use crate::{CharCursor, CharLexerError, ParseChars};

impl ParseChars for char {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let start = cursor.position();
        let checkpoint = cursor.checkpoint();
        match cursor.peek() {
            Some(c) => {
                cursor.advance();
                Ok(c)
            }
            None => {
                let span = cursor.span_since(checkpoint);
                let ch = cursor.str_from_span(span);

                Err(CharLexerError::UnexpectedEof {
                    expected: ch.to_string(),
                    span,
                })
            }
        }
    }
}
