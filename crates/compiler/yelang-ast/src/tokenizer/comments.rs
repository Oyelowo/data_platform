// Make my #file:ast align closely with #file:new_all.md and the expression structure being more closely to #file:expr_integrate.md . I have been migrating from using String or &str  in AST to Symbol. So continue to do that even wherever the instructions code may suggest String, Symbol is what is intended and the right thing to do. Also incldue all the code doc comments descriptoon and example that are mentioned
/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */
use crate::Symbol;
use yelang_lexer::{CharCursor, CharLexerError, Either, Eof, Newline, OneOf3, ParseChars, Span};
use std::fmt::{self, Display};

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SingleLineComment {
    span: Span,
}

impl Display for SingleLineComment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.span)
    }
}

impl SingleLineComment {}

impl ParseChars for SingleLineComment {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        cursor.verify("//")?;
        let (_, span) = cursor.until_b4::<char, Either<Newline, Eof>>()?;
        // let stri = span.as_slice(cursor);
        Ok(SingleLineComment { span })
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SingleLineCommentSqlStyle {
    value: Span,
}

impl Display for SingleLineCommentSqlStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

// TODO: Remove this, causes ambiguity with repeated unary operators e.g --5
// impl SingleLineCommentSqlStyle {
//     pub fn new(value: &'a str) -> Self {
//         Self { value }
//     }
//
//     pub fn value(&self) -> &'a str {
//         self.value
//     }
// }

// impl ParseChars for SingleLineCommentSqlStyle {
//     fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
//         cursor.verify("--")?;
//         let (_, span) = cursor.until_b4::<char, Either<Newline, Eof>>()?;
//
//         Ok(SingleLineCommentSqlStyle::new(span.as_slice(cursor)))
//     }
// }

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

        // let content = &cursor.span_since(start_checkpoint).as_slice(cursor);

        Ok(MultiLineComment {
            span: cursor.span_since(start_checkpoint),
        })
    }
}

impl fmt::Display for MultiLineComment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // write!(f, "/*{}*/", self.value)
        write!(f, "{}", self.span)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Comment {
    SingleLine(SingleLineComment),
    // SingleLineShellStyle(SingleLineCommentSqlStyle),
    MultiLine(MultiLineComment),
}

// impl Display for Comment<'_> {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         match self {
//             Comment::SingleLine(comment) => write!(f, "{}", comment),
//             // Comment::SingleLineShellStyle(comment) => write!(f, "{}", comment),
//             Comment::MultiLine(comment) => write!(f, "{}", comment),
//         }
//     }
// }

impl Comment {
    pub fn span(&self) -> &Span {
        match self {
            Comment::SingleLine(comment) => &comment.span,
            // Comment::SingleLineShellStyle(comment) => &comment.value,
            Comment::MultiLine(comment) => &comment.span,
        }
    }
}

impl ParseChars for Comment {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let comment = cursor.parse::<Either<SingleLineComment, MultiLineComment>>()?;

        let comment = match comment {
            Either::Left(comment) => Comment::SingleLine(comment),
            Either::Right(comment) => Comment::MultiLine(comment),
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
        let value = res.span.as_slice(&cursor);
        assert_eq!(value, expected);
    }

    #[rstest]
    #[case("this is it \n\t")]
    #[case("-// this is it \n\t")]
    fn test_single_line_comment_fail(#[case] input: &str) {
        let mut cursor = CharCursor::new(input);
        let comment = cursor.parse::<SingleLineComment>();
        assert!(comment.is_err());
    }

    // #[rstest]
    // #[case("--this is it \n\t", "--this is it ")]
    // #[case("-- this is it \n\t", "-- this is it ")]
    // #[case("--/////", "--/////")]
    // #[case("-- /////", "-- /////")]
    // #[case("-- This is a comment\n", "-- This is a comment")]
    // #[case("--Another comment", "--Another comment")]
    // #[case("-- \n", "-- ")]
    // #[case(
    //     "-- This is a shell-style comment\n",
    //     "-- This is a shell-style comment"
    // )]
    // #[case("--Another shell comment", "--Another shell comment")]
    // #[case("-- \n", "-- ")]
    // fn test_single_line_comment_sql_style(#[case] input: &str, #[case] expected: &str) {
    //     let mut cursor = CharCursor::new(input);
    //     let comment = cursor.parse::<SingleLineCommentSqlStyle>().unwrap();
    //     assert_eq!(comment.value(), expected);
    // }

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
        let value = comment.span.as_slice(&cursor);
        assert_eq!(value, expected);
    }

    #[rstest]
    #[case("// Single-line comment\n", "// Single-line comment")]
    // #[case("-- Sql comment\n", "-- Sql comment")]
    #[case("/* Multi-line */", "/* Multi-line */")]
    fn test_comment(#[case] input: &str, #[case] expected: &str) {
        let mut cursor = CharCursor::new(input);
        let comment = cursor.parse::<Comment>().unwrap();
        let comment = comment.span().as_slice(&cursor);
        assert_eq!(comment, expected);
    }
}
