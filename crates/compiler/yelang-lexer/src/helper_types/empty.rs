/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */

use crate::{CharCursor, CharLexerResult, ParseChars};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Empty;

impl ParseChars for Empty {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        Ok(Empty)
    }
}
