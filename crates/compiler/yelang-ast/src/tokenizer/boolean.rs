/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use yelang_lexer::{CharCursor, CharLexerResult, ParseChars, ParseTokenStream, Span};
use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Boolean {
    pub value: bool,
    pub span: Span,
}

impl Boolean {
    pub fn value(&self) -> bool {
        self.value
    }

    pub fn span(&self) -> Span {
        self.span
    }
}

impl ParseChars for Boolean {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let start = cursor.checkpoint();
        let value = match cursor.consume("true") {
            Ok(_) => true,
            Err(_) => {
                cursor.consume("false")?;
                false
            }
        };
        let span = cursor.span_since(start);
        Ok(Boolean { value, span })
    }
}

impl Display for Boolean {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("true", true, (0, 4), 0)]
    #[case("false", false, (0, 5), 0)]
    #[case("true.x", true, (0, 4), 2)]
    #[case("false.", false, (0, 5), 1)]
    #[case("truex", true, (0, 4), 1)]
    #[case("false.", false, (0, 5), 1)]
    fn test_boolean(
        #[case] input: &str,
        #[case] expected: bool,
        #[case] span_expected: (usize, usize),
        #[case] remaining: usize,
    ) {
        let mut cursor = CharCursor::new(input);
        let checkpont = cursor.checkpoint();
        let boolean = cursor.parse::<Boolean>().unwrap();
        let span = cursor.span_since(checkpont);

        assert_eq!(boolean.value(), expected);
        assert_eq!(boolean.to_string(), expected.to_string());
        assert_eq!(span.start().absolute, span_expected.0);
        assert_eq!(span.end().absolute, span_expected.1);
        assert_eq!(cursor.remaining().len(), remaining);
    }

    #[rstest]
    #[case("tru", (0, 0), 3)]
    #[case("fals", (0, 0), 4)]
    #[case("tr.ue", (0, 0), 5)]
    #[case(".false", (0, 0), 6)]
    #[case("txrue", (0, 0), 5)]
    fn test_boolean_error(
        #[case] input: &str,
        #[case] span_expected: (usize, usize),
        #[case] remaining: usize,
    ) {
        let mut cursor = CharCursor::new(input);
        let checkpont = cursor.checkpoint();
        let boolean = cursor.parse::<Boolean>();
        let span = cursor.span_since(checkpont);

        assert!(boolean.is_err());
        assert_eq!(span.start().absolute, span_expected.0);
        assert_eq!(span.end().absolute, span_expected.1);
        assert_eq!(cursor.remaining().len(), remaining);
    }
}
