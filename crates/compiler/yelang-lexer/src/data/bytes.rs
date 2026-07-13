/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 7/12/2025
 */
use crate::helper_types::bytes::Char;
use crate::{CharCursor, CharLexerError, CharLexerResult, ParseChars, Span};
use std::fmt::Display;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct ByteLexed {
    span: Span,
    value: Arc<[u8]>,
}

impl ByteLexed {
    pub fn span(&self) -> Span {
        self.span
    }

    pub fn value(&self) -> &Arc<[u8]> {
        &self.value
    }
}

fn process_byte_escapes(content: &str, span: Span) -> Result<Arc<[u8]>, CharLexerError> {
    let mut result = Vec::new();
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped_char) = chars.next() {
                match escaped_char {
                    'n' => result.push(b'\n'),
                    'r' => result.push(b'\r'),
                    't' => result.push(b'\t'),
                    '\\' => result.push(b'\\'),
                    '"' => result.push(b'"'),
                    '\'' => result.push(b'\''),
                    'x' => {
                        // \xFF
                        let mut hex = String::new();
                        for _ in 0..2 {
                            if let Some(h) = chars.next() {
                                hex.push(h);
                            } else {
                                return Err(CharLexerError::UnexpectedEof {
                                    expected: "hex digit".to_string(),
                                    span,
                                });
                            }
                        }
                        if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                            result.push(byte);
                        } else {
                            return Err(CharLexerError::InvalidEscape {
                                sequence: format!("x{}", hex),
                                span,
                            });
                        }
                    }
                    _ => {
                        return Err(CharLexerError::InvalidEscape {
                            sequence: escaped_char.to_string(),
                            span,
                        });
                    }
                }
            } else {
                return Err(CharLexerError::UnexpectedEof {
                    expected: "escaped character".to_string(),
                    span,
                });
            }
        } else {
            if let Ok(byte) = ch.try_into() {
                result.push(byte);
            } else {
                return Err(CharLexerError::UnexpectedChar {
                    expected: "ASCII character".to_string(),
                    found: ch,
                    span,
                });
            }
        }
    }

    Ok(result.into())
}

impl ParseChars for ByteLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let start = cursor.checkpoint();
        cursor.parse::<Char<'b'>>()?;
        cursor.parse::<Char<'"'>>()?;
        let (content, content_span) = cursor.until_b4_str("\"")?;
        cursor.consume("\"")?;
        let processed = process_byte_escapes(&content, content_span)?;
        Ok(ByteLexed {
            span: cursor.span_since(start),
            value: processed,
        })
    }
}
