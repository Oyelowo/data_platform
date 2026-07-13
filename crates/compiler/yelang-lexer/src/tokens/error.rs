/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::chars::{CharLexerError, Span};
use thiserror::Error;

pub type TokenResult<T> = Result<T, TokenError>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TokenError {
    #[error("Syntax error: {message} at {span}")]
    SyntaxError {
        message: String,
        span: Span,
        #[source]
        source: Option<Box<TokenError>>,
    },

    #[error("Unexpected token '{found}', expected {expected} at {span}")]
    UnexpectedToken {
        expected: String,
        found: String,
        span: Span,
    },

    #[error("Unexpected end of input, expected {expected} at {span}")]
    UnexpectedEof { expected: String, span: Span },

    #[error(transparent)]
    LexError(#[from] CharLexerError),

    #[error("Insufficient repetition: expected {expected}, found {found}")]
    InsufficientRepetition {
        expected: usize,
        found: usize,
        span: Span,
    },

    #[error("Custom error: {msg} at {span}")]
    CustomError { msg: String, span: Span },
}

impl TokenError {
    pub fn span(&self) -> Span {
        match self {
            Self::SyntaxError { span, .. } => *span,
            Self::UnexpectedToken { span, .. } => *span,
            Self::UnexpectedEof { span, .. } => *span,
            Self::LexError(e) => e.span(),
            Self::InsufficientRepetition { span, .. } => *span,
            Self::CustomError { span, .. } => *span,
        }
    }

    pub fn merge(self, other: Self) -> Self {
        let combined_span = self.span().merge(other.span());

        match (self, other) {
            (
                Self::SyntaxError {
                    message,
                    span,
                    source,
                },
                other,
            ) => Self::SyntaxError {
                message: format!("{}\nAlso: {}", message, other),
                span: combined_span,
                source: Some(Box::new(other)),
            },

            (first, second) => Self::SyntaxError {
                message: format!("Multiple errors:\n1. {}\n2. {}", first, second),
                span: combined_span,
                source: Some(Box::new(second)),
            },
        }
    }
}
