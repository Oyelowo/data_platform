/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::{CharCursor, CharLexerResult, ParseChars};

struct Padded<P> {
    parser: P,
}

impl<P: ParseChars> ParseChars for Padded<P> {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        cursor.consume_while(|c| c.is_whitespace());
        let parser = cursor.parse::<P>()?;
        cursor.consume_while(|c| c.is_whitespace());
        Ok(Self { parser })
    }
}
