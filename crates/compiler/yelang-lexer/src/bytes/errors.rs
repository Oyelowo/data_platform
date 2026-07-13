/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::ByteSpan;
use thiserror::Error;

pub type ByteLexerResult<T> = Result<T, ByteLexerError>;

#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum ByteLexerError {
    #[error("Unexpected end of file. Expected {expected}")]
    UnexpectedEof { expected: String, span: ByteSpan },
    #[error("Unexpected byte. Expected {expected}, found {found}")]
    UnexpectedByte {
        expected: String,
        found: u8,
        span: ByteSpan,
    },
    // #[error("Unexpected byte. Expected {expected}, found {found}")]
    // ExpectedEof { span: ByteSpan, expected: String, found: u8 },
    #[error("Invalid length. Expected length between {min} and {max}, found {found}")]
    InvalidLength {
        found: usize,
        min: usize,
        max: usize,
        span: ByteSpan,
    },
    #[error("Empty expected string")]
    EmptyExpectedString { span: ByteSpan },
}

impl ByteLexerError {
    pub fn merge(self, other: ByteLexerError) -> ByteLexerError {
        match (self, other) {
            (s, _) => s,
        }
    }
}
