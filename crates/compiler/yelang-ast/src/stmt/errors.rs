/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */
use thiserror::Error;
use yelang_lexer::{CharLexerError, Span};

pub type ParserResult<T> = Result<T, ParserError>;

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("Syntax error: {message} at {span}")]
    SyntaxError {
        message: String,
        span: Span,
        #[source]
        source: Option<Box<ParserError>>,
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

    #[error(
        "Cannot reference query item binder '{binder}' in a top-level object projection; enter an element context first (e.g. users@{binder}[*].{{ ... }}). at {span}"
    )]
    QueryItemBinderOutOfScope { binder: String, span: Span },

    #[error("Custom error: {msg} at {span}")]
    CustomError { msg: String, span: Span },
}

impl ParserError {
    pub fn span(&self) -> Span {
        match self {
            Self::SyntaxError { span, .. } => *span,
            Self::UnexpectedToken { span, .. } => *span,
            Self::UnexpectedEof { span, .. } => *span,
            Self::LexError(e) => e.span(),
            Self::InsufficientRepetition { span, .. } => *span,
            Self::QueryItemBinderOutOfScope { span, .. } => *span,
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
