/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */
use crate::{CharCursor, CharLexerError, OneOf3, ParseChars, Span};
use std::fmt;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SingleLineComment {
    span: Span,
}

impl SingleLineComment {}

fn parse_single_line_comment_span(
    cursor: &mut CharCursor,
    leader: &'static str,
) -> Result<Span, CharLexerError> {
    let start = cursor.checkpoint();
    cursor.consume(leader)?;

    // IMPORTANT (lossless tokenization): do NOT consume the newline terminator.
    // The lossless tokenizer emits a separate `Whitespace` token for it.
    cursor.consume_while(|c| c != '\n' && c != '\r');

    Ok(cursor.span_since(start))
}

impl ParseChars for SingleLineComment {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let span = parse_single_line_comment_span(cursor, "//")?;
        Ok(SingleLineComment { span })
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SingleLineCommentSqlStyle {
    span: Span,
}

impl SingleLineCommentSqlStyle {}

impl ParseChars for SingleLineCommentSqlStyle {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let span = parse_single_line_comment_span(cursor, "--")?;

        Ok(SingleLineCommentSqlStyle { span })
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct MultiLineComment {
    span: Span,
}

impl MultiLineComment {}
/*/*fgfg*/*/
impl ParseChars for MultiLineComment {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let start_checkpoint = cursor.checkpoint();
        cursor.consume("/*").map_err(|_e| {
            let span = cursor.span_since(start_checkpoint);
            CharLexerError::UnexpectedChar {
                expected: "/*".to_string(),
                found: cursor.peek().unwrap_or('\0'),
                span,
            }
        })?;

        let mut nesting_level = 1;

        while nesting_level > 0 {
            if cursor.peek_n_char(2).is_some() {
                if cursor.consume("/*").is_ok() {
                    nesting_level += 1;
                } else if cursor.consume("*/").is_ok() {
                    nesting_level -= 1;
                } else {
                    cursor
                        .advance()
                        .ok_or_else(|| CharLexerError::UnterminatedComment {
                            span: cursor.span_since(start_checkpoint),
                        })?;
                }
            } else if cursor.advance().is_none() {
                return Err(CharLexerError::UnterminatedComment {
                    span: cursor.span_since(start_checkpoint),
                });
            }
        }

        let span = cursor.span_since(start_checkpoint);

        Ok(MultiLineComment { span })
    }
}

impl fmt::Display for MultiLineComment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/* comment */")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Comment {
    SingleLine(SingleLineComment),
    SingleLineShellStyle(SingleLineCommentSqlStyle),
    MultiLine(MultiLineComment),
}

impl Comment {
    pub fn span(&self) -> Span {
        match self {
            Comment::SingleLine(comment) => comment.span,
            Comment::SingleLineShellStyle(comment) => comment.span,
            Comment::MultiLine(comment) => comment.span,
        }
    }
}

impl ParseChars for Comment {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let comment =
            cursor
                .parse::<OneOf3<SingleLineComment, SingleLineCommentSqlStyle, MultiLineComment>>(
                )?;

        let comment = match comment {
            OneOf3::_1(comment) => Comment::SingleLine(comment),
            OneOf3::_2(comment) => Comment::SingleLineShellStyle(comment),
            OneOf3::_3(comment) => Comment::MultiLine(comment),
        };

        // let comment = cursor
        //     .parse::<SingleLineComment>()
        //     .map(Comment::SingleLine)
        //     .or_else(|_| {
        //         cursor
        //             .parse::<SingleLineCommentShellStyle>()
        //             .map(Comment::SingleLineShellStyle)
        //     })
        //     .or_else(|_| cursor.parse::<MultiLineComment>().map(Comment::MultiLine))?;

        Ok(comment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("//this is it", "//this is it")]
    #[case("//this is it \n\t", "//this is it ")]
    #[case("// this is it \n\t", "// this is it ")]
    #[case("///////", "///////")]
    #[case("// /////", "// /////")]
    #[case("// This is a comment\n", "// This is a comment")]
    #[case("// This is a comment\n", "// This is a comment")]
    #[case("//Another comment", "//Another comment")]
    #[case("// \n", "// ")]
    fn test_single_line_comment(#[case] input: &str, #[case] expected: &str) {
        let mut cursor = CharCursor::new(input);
        let res = cursor
            .parse::<SingleLineComment>()
            .map_err(|e| e.to_string())
            .unwrap();
        let comment = res.span.as_slice(&cursor);
        assert_eq!(comment, expected);
    }

    #[rstest]
    #[case("this is it \n\t")]
    #[case("-// this is it \n\t")]
    fn test_single_line_comment_fail(#[case] input: &str) {
        let mut cursor = CharCursor::new(input);
        let comment = cursor.parse::<SingleLineComment>();
        assert!(comment.is_err());
    }

    #[rstest]
    #[case("--this is it \n\t", "--this is it ")]
    #[case("-- this is it \n\t", "-- this is it ")]
    #[case("--/////", "--/////")]
    #[case("-- /////", "-- /////")]
    #[case("-- This is a comment\n", "-- This is a comment")]
    #[case("--Another comment", "--Another comment")]
    #[case("-- \n", "-- ")]
    #[case(
        "-- This is a shell-style comment\n",
        "-- This is a shell-style comment"
    )]
    #[case("--Another shell comment", "--Another shell comment")]
    #[case("-- \n", "-- ")]
    fn test_single_line_comment_sql_style(#[case] input: &str, #[case] expected: &str) {
        let mut cursor = CharCursor::new(input);
        let comment = cursor.parse::<SingleLineCommentSqlStyle>().unwrap();
        let comment = comment.span.as_slice(&cursor);
        assert_eq!(comment, expected);
    }

    #[rstest]
    #[case("this is it \n\t")]
    #[case("-// this is it \n\t")]
    fn test_single_line_comment_shell_style_fail(#[case] input: &str) {
        let mut cursor = CharCursor::new(input);
        let comment = cursor.parse::<SingleLineComment>();
        assert!(comment.is_err());
    }

    #[rstest]
    #[case("/* Multi-line comment */", "/* Multi-line comment */")]
    #[case("/*Another comment*/", "/*Another comment*/")]
    #[case("/* Multi-line comment */", "/* Multi-line comment */")]
    #[case("/*Another comment*/", "/*Another comment*/")]
    #[case("/* Multi-line comment */", "/* Multi-line comment */")]
    #[case("/*Another comment*/", "/*Another comment*/")]
    fn test_multi_line_comment(#[case] input: &str, #[case] expected: &str) {
        let mut cursor = CharCursor::new(input);
        let comment = cursor.parse::<MultiLineComment>().unwrap();
        let value = cursor.str_from_span(comment.span);
        assert_eq!(value, expected);
    }

    #[rstest]
    #[case("// Single-line comment\n", "// Single-line comment")]
    #[case("-- Sql comment\n", "-- Sql comment")]
    #[case("/* Multi-line */", "/* Multi-line */")]
    fn test_comment(#[case] input: &str, #[case] expected: &str) {
        let mut cursor = CharCursor::new(input);
        let comment = cursor.parse::<Comment>().unwrap();

        assert_eq!(cursor.str_from_span(comment.span()), expected)
    }
}
