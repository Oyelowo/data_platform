/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{CharCursor, CharLexerResult, ParseChars};

pub struct Newline;

impl ParseChars for Newline {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        cursor
            // Line feed
            .consume("\n")
            // Carriage return + line feed
            .or(cursor.consume("\r\n"))
            // Carriage return
            .or(cursor.consume("\r"))
            // Vertical tab
            .or(cursor.consume("\x0B"))
            // Form feed
            .or(cursor.consume("\x0C"))
            // Next line
            .or(cursor.consume("\u{0085}"))
            // Line separator
            .or(cursor.consume("\u{2028}"))
            // Paragraph separator
            .or(cursor.consume("\u{2029}"))
            .map(|_| Newline)
    }
}
