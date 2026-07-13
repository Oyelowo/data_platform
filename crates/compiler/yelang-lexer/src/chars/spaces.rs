/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 30/01/2025
 */
use super::{CharCursor, ParseChars};

pub struct Spaces {
    spaces: usize,
}

impl Spaces {
    pub fn count(&self) -> usize {
        self.spaces
    }
}

impl ParseChars for Spaces {
    fn parse(cursor: &mut CharCursor) -> Result<Self, super::errors::CharLexerError> {
        let len = cursor.consume_while_m(1, |c| c.is_whitespace())?.len();
        Ok(Self { spaces: len })
    }
}

#[cfg(test)]
mod test_spaces {
    use super::Spaces;
    use rstest::rstest;

    #[rstest]
    #[case(" ", 1)]
    #[case("  ", 2)]
    #[case("   ", 3)]
    #[case("    ", 4)]
    #[case("     ", 5)]
    #[case(" \n  \r \t \n", 9)]
    #[case("\n", 1)]
    #[case("\n\r", 2)]
    #[case("\r", 1)]
    #[case("\t", 1)]
    fn test_spaces(#[case] input: &str, #[case] expected: usize) {
        let mut cursor = super::CharCursor::new(input);
        let result = cursor.parse::<Spaces>();
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.spaces, expected);
    }
}
