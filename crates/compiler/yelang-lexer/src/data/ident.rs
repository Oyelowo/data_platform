/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 */
use crate::{
    Alpha, AlphaNum, Char, CharCursor, CharLexerError, CharLexerResult, Either, ParseChars, Repeat,
    Span,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentLexed {
    pub span: Span,
    pub is_raw: bool,
}

impl IdentLexed {
    pub fn is_raw(&self) -> bool {
        self.is_raw
    }

    pub fn span(&self) -> Span {
        self.span
    }
}

impl ParseChars for IdentLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let is_raw = cursor.consume("r#").is_ok();

        let (_, span) = cursor.parse_with_span::<(
            Either<Alpha, Char<'_'>>,
            Option<Repeat<Either<AlphaNum, Char<'_'>>>>,
        )>()?;

        // We do NOT check for keywords here.
        // IdentLexed simply parses any valid identifier shape.
        // The Tokenizer (tokens.rs) is responsible for checking if this text
        // matches a Keyword (e.g. "select") or is a generic Ident.

        Ok(IdentLexed { span, is_raw })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("_hello", "_hello", false, (0, 6))]
    #[case("r#_hello", "_hello", true, (2, 8))]
    #[case("r#true", "true", true, (2, 6))]
    #[case("__hello", "__hello", false, (0, 7))]
    #[case("_8hello", "_8hello", false, (0, 7))]
    #[case("___hello", "___hello", false, (0, 8))]
    #[case("hello", "hello", false, (0, 5))]
    #[case("world", "world", false, (0, 5))]
    #[case("hello_world", "hello_world", false, (0, 11))]
    #[case("helloWorld", "helloWorld", false, (0, 10))]
    #[case("hello_world_123", "hello_world_123", false, (0, 15))]
    #[case("hello_world_123_", "hello_world_123_", false, (0, 16))]
    // Keywords are valid identifier SHAPES, so they should pass here.
    // They will be distinguished in the Tokenizer.
    #[case("let", "let", false, (0, 3))]
    #[case("select", "select", false, (0, 6))]
    fn test_ident_success(
        #[case] input: &str,
        #[case] expected_str: &str,
        #[case] is_raw: bool,
        #[case] expected_span: (usize, usize),
    ) {
        let mut cursor = CharCursor::new(input);
        let ident = cursor.parse::<IdentLexed>().unwrap();
        let value = cursor.str_from_span(ident.span);
        assert_eq!(value, expected_str);
        assert_eq!(ident.is_raw, is_raw);
        assert_eq!(ident.span.start().absolute, expected_span.0);
        assert_eq!(ident.span.end().absolute, expected_span.1);
        assert_eq!(ident.span.end().line, 1);
        assert!(cursor.is_eof());
        assert_eq!(cursor.peek(), None);
    }

    #[rstest]
    #[case("123name")]
    #[case("8name")]
    #[case(".hello+world")]
    #[case("-hello_world")]
    #[case("-hello")]
    #[case("/hello")]
    fn test_ident_fail(#[case] input: &str) {
        let mut cursor = CharCursor::new(input);
        let ident = cursor.parse::<IdentLexed>();
        assert!(ident.is_err());
    }

    #[rstest]
    #[case("123name")]
    #[case("8name")]
    #[case("hello world")]
    #[case("hello-world")]
    #[case("-hello_world")]
    #[case("hello.world")]
    #[case("hello,world")]
    #[case("hello;world")]
    #[case("hello:world")]
    #[case("hello/world")]
    #[case("hello\\world")]
    fn test_ident_fail_exact(#[case] input: &str) {
        let mut cursor = CharCursor::new(input);
        let ident = cursor.parse_exact::<IdentLexed>();
        assert!(ident.is_err());
    }
}
