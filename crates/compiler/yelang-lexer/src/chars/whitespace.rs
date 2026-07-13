/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{CharCursor, CharLexerResult, ParseChars};

pub struct Whitespace;

impl ParseChars for Whitespace {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        cursor.consume_while_m(1, |c| c.is_whitespace())?;
        Ok(Whitespace)
    }
}

pub struct Tab;

impl ParseChars for Tab {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        cursor.consume_char('\t').map(|_| Tab)
    }
}

pub struct CarriageReturn;

impl ParseChars for CarriageReturn {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        cursor.consume_char('\r').map(|_| CarriageReturn)
    }
}
