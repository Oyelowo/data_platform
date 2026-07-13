/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use std::fmt::{self, Display, Formatter};

use super::{alpha::{ByteAlpha, ByteDigit}, ByteCursor, ByteLexerError, ByteSpan, ParseBytes};

#[derive(Debug)]
pub enum ByteEither<A, B> {
    A(A),
    B(B),
}

impl<A: Display, B: Display> Display for ByteEither<A, B> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ByteEither::A(a) => write!(f, "{a}"),
            ByteEither::B(b) => write!(f, "{b}"),
        }
    }
}

pub type ByteAlphaNum<'a> = ByteEither<ByteAlpha<'a>, ByteDigit<'a>>;

impl<'a> ParseBytes<'a> for ByteAlphaNum<'a> {
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
        let checkpoint = cursor.checkpoint();
        if let Ok(a) = cursor.parse::<ByteAlpha>() {
            return Ok(ByteEither::A(a));
        }
        cursor.restore(checkpoint);

        if let Ok(d) = cursor.parse::<ByteDigit>() {
            return Ok(ByteEither::B(d));
        }
        Err(ByteLexerError::UnexpectedEof {
            expected: "alpha or digit".to_owned(),
            span: ByteSpan::new(checkpoint.current_pos(), cursor.position()),
        })
    }
}
