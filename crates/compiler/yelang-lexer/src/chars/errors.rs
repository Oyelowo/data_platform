/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */

use super::cursor::Span;
use std::io;

pub type CharLexerResult<T> = std::result::Result<T, CharLexerError>;

#[derive(Debug, thiserror::Error, Clone)]
#[non_exhaustive]
pub enum CharLexerError {
    #[error("Unexpected character '{found}' at {span}. Expected `{expected}`")]
    UnexpectedChar {
        expected: String,
        found: char,
        span: Span,
    },

    #[error("Unexpected character '{found}' at {span}. Expected `{expected}`")]
    UnexpectedStr {
        expected: String,
        found: String,
        span: Span,
    },

    #[error("Unterminated string literal at {span}")]
    UnterminatedString { span: Span },

    #[error("Invalid escape sequence '\\{sequence}' at {span}")]
    InvalidEscape { sequence: String, span: Span },

    #[error("Unicode escape error at {span}: {message}")]
    UnicodeError { message: String, span: Span },

    #[error("Unexpected end of input at {span}. Expected {expected}")]
    UnexpectedEof { expected: String, span: Span },

    #[error("Unknown keyword '{keyword} at {span}'. Expected: {expected}")]
    UnknownKeyword {
        keyword: String,
        span: Span,
        expected: String,
    },

    #[error("Invalid keyword suffix '{suffix}' for keyword '{keyword}' at {span}")]
    InvalidKeywordSuffix {
        keyword: String,
        suffix: String,
        span: Span,
    },

    #[error("Invalid string tag at {span}")]
    InvalidStringTag { span: Span },

    #[error("Invalid modifiers")]
    InvalidModifiers { span: Span },

    #[error("Duplicate modifier '{modifier}' at {span}")]
    DuplicateModifier { modifier: char, span: Span },

    #[error("Too many delimiters (max 255)")]
    TooManyDelimiters(Span),

    #[error("Unknown string tag '{name}'")]
    UnknownTag { name: String, span: Span },

    #[error("Unsupported modifier combination")]
    InvalidModifierCombination { span: Span },

    #[error("Expected whitespace at {span}")]
    ExpectedWhitespace { span: Span },

    #[error("Expected newline at {span}")]
    ExpectedHorizontalSpace { span: Span },

    #[error("Expected newline at {span}")]
    ExpectedVerticalSpace { span: Span },

    #[error("Expected single space at {span}")]
    ExpectedSingleSpace { span: Span },

    #[error("Invalid length at {span}. Found: {found}, Must be between {min} and {max} length")]
    InvalidLength {
        found: usize,
        min: usize,
        max: usize,
        span: Span,
        // expected: usize,
    },

    #[error("Expected number: {span}")]
    InvalidNumber { span: Span },

    #[error("Invalid symbol at {span}")]
    UnexpectedSymbol {
        expected: String,
        found: char,
        span: Span,
    },
    #[error("Invalid repetition at {span}")]
    InsufficientRepetition {
        expected: usize,
        found: usize,
        span: Span,
    },
    #[error("Expected number at {span}")]
    ExpectedNumber { span: Span },
    #[error("Expected Comment at {span}")]
    UnterminatedComment { span: Span },

    #[error("Multiple errors found ({}):\n{}", .errors.len(), .formatted_errors)]
    CompositeError {
        errors: Vec<CharLexerError>,
        span: Span,
        formatted_errors: String,
    },
    #[error("Expected float at {span}")]
    ExpectedFloat { span: Span },

    #[error("Expected string at {span}")]
    EmptyExpectedString { span: Span },

    #[error("Invalid identifier at {span}")]
    InvalidIdent { found: String, span: Span },

    #[error("Expected terminator at {span}")]
    EmptyTerminator { span: Span },

    #[error(
        "Unmatched string delimeter at {span}. You opened with {opening} #s and must close the string with the same number of delimeters"
    )]
    UnMatchedStringDelimeter { opening: usize, span: Span },
    #[error("non-raw cannot have string delimeter at {span} cannot have modifiers")]
    InvalidStringDelimeterWithModifier { span: Span },
}

impl CharLexerError {
    pub fn span(&self) -> Span {
        match self {
            Self::UnexpectedChar { span, .. } => *span,
            Self::UnterminatedString { span } => *span,
            Self::InvalidEscape { span, .. } => *span,
            Self::UnicodeError { span, .. } => *span,
            Self::UnexpectedEof { span, .. } => *span,
            Self::UnknownKeyword { span, .. } => *span,
            Self::InvalidKeywordSuffix { span, .. } => *span,
            Self::InvalidStringTag { span, .. } => *span,
            Self::InvalidModifiers { span, .. } => *span,
            Self::DuplicateModifier { span, .. } => *span,
            Self::TooManyDelimiters(span) => *span,
            Self::UnknownTag { span, .. } => *span,
            Self::InvalidModifierCombination { span, .. } => *span,
            Self::ExpectedWhitespace { span, .. } => *span,
            Self::ExpectedHorizontalSpace { span, .. } => *span,
            Self::ExpectedVerticalSpace { span, .. } => *span,
            Self::ExpectedSingleSpace { span, .. } => *span,
            Self::InvalidLength { span, .. } => *span,
            Self::InvalidNumber { span, .. } => *span,
            Self::UnexpectedSymbol { span, .. } => *span,
            Self::InsufficientRepetition { span, .. } => *span,
            Self::ExpectedNumber { span, .. } => *span,
            Self::UnterminatedComment { span, .. } => *span,
            Self::CompositeError { span, .. } => *span,
            Self::ExpectedFloat { span, .. } => *span,
            Self::EmptyExpectedString { span, .. } => *span,
            Self::InvalidIdent { span, .. } => *span,
            Self::EmptyTerminator { span, .. } => *span,
            Self::UnMatchedStringDelimeter { span, .. } => *span,
            Self::UnexpectedStr { span, .. } => *span,
            Self::InvalidStringDelimeterWithModifier { span, .. } => *span,
        }
    }

    pub fn merge_old(&self, other: &Self) -> Self {
        let merged_span = self.span().merge(other.span());
        let mut errors = Vec::new();

        // Flatten self
        match self {
            Self::CompositeError {
                errors: existing, ..
            } => {
                errors.extend(existing.clone());
            }
            _ => errors.push(self.clone()),
        }

        // Flatten other
        match other {
            Self::CompositeError {
                errors: existing, ..
            } => {
                errors.extend(existing.clone());
            }
            _ => errors.push(other.clone()),
        }

        CharLexerError::CompositeError {
            errors,
            span: merged_span,
            formatted_errors: "".to_string(),
        }
    }

    /// Merges errors while flattening composites and deduplicating
    pub fn merge(self, other: &Self) -> Self {
        let merged_span = self.span().merge(other.span());
        let mut errors = Vec::new();

        self.flatten_into(&mut errors);
        other.clone().flatten_into(&mut errors);

        // Deduplicate while preserving order
        Self::deduplicate_errors(&mut errors);

        match errors.len() {
            0 => panic!("Merged two errors but got empty result"),
            1 => errors.into_iter().next().unwrap(),
            _ => Self::composite(errors, merged_span),
        }
    }

    fn flatten_into(self, errors: &mut Vec<Self>) {
        match self {
            Self::CompositeError {
                errors: mut inner_errors,
                ..
            } => {
                errors.append(&mut inner_errors);
            }
            _ => errors.push(self),
        }
    }

    pub fn composite(errors: Vec<Self>, span: Span) -> Self {
        let formatted_errors = errors
            .iter()
            .map(|e| format!("• {} (at {})", e, e.span()))
            .collect::<Vec<_>>()
            .join("\n");

        Self::CompositeError {
            errors,
            span,
            formatted_errors,
        }
    }

    fn deduplicate_errors(errors: &mut Vec<CharLexerError>) {
        let mut seen = std::collections::HashSet::new();
        errors.retain(|e| {
            let key = match e {
                CharLexerError::UnexpectedChar {
                    expected,
                    found,
                    span,
                } => format!("UnexpectedChar|{expected}|{found}|{span:?}"),
                _ => format!("{}|{:?}", e, e.span()),
            };
            seen.insert(key)
        });
    }
}

// Error recovery
// #[test]
// fn test_error_recovery() {
//     let input = "123 ~!@ 456";
//     let mut cursor = Cursor::new(input);
//     let mut numbers = vec![];
//     let mut errors = vec![];
//
//     while !cursor.is_eof() {
//         match cursor.parse::<u32>() {
//             Ok(n) => numbers.push(n),
//             Err(e) => {
//                 errors.push(e);
//                 cursor.recover_until(char::is_numeric);
//             }
//         }
//     }
//
//     assert_eq!(numbers, vec![123, 456]);
//     assert_eq!(errors.len(), 1);
// }

pub enum StreamError {
    Partial {
        requested: usize,
        available: usize,
        buffer: String,
    },
    Io(io::Error),
    Utf8(std::str::Utf8Error),
}
