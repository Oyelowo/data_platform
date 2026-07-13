/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::{ByteCursor, ByteLexerError, ParseBytes};
use crate::ByteSpan;

/// Parse ASCII letters [A-Za-z], returning a slice of the matched bytes.
#[derive(Debug)]
pub struct ByteAlpha(ByteSpan);

impl ByteAlpha {}

impl<'a> ParseBytes<'a> for ByteAlpha {
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
        let start = cursor.checkpoint();
        // Try to consume at least 1 byte that is_ascii_alphabetic()
        let slice = cursor.consume_while_m_n(1, None, |b| b.is_ascii_alphabetic())?;
        let span = cursor.span_since(start);
        // Ok(ByteAlpha(cursor.slice(span.start(), span.end())))
        Ok(ByteAlpha(span))
    }
}

/// Parse ASCII digits [0-9], returning a slice of matched bytes.
#[derive(Debug)]
pub struct ByteDigit(ByteSpan);

impl ByteDigit {}

impl<'a> ParseBytes<'a> for ByteDigit {
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
        let start = cursor.checkpoint();
        let slice = cursor.consume_while_m_n(1, None, |b| b.is_ascii_digit())?;
        let span = cursor.span_since(start);
        Ok(ByteDigit(span))
    }
}
