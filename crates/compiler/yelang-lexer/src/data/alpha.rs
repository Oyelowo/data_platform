/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::TDigit;
use crate::{CharCursor, CharLexerError, CharLexerResult, ParseChars};
use crate::{Either, Repeat};

pub struct Alpha;

impl Alpha {}

impl ParseChars for Alpha {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        let end = cursor.consume_exact(1, |c| c.is_ascii_alphabetic())?;
        // let end = cursor.consume_while(|c| c.is_ascii_alphabetic());
        let span = cursor.span_since(checkpoint);
        if end.is_empty() {
            Err(CharLexerError::UnexpectedEof {
                expected: "str".to_string(),
                span,
            })
        } else {
            Ok(Alpha)
        }
    }
}

// impl<'a> ParseBytes<'a> for Alpha<'a> {
//     fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
//         let start = cursor.checkpoint();
//         // Try to consume at least 1 byte that is_ascii_alphabetic()
//         let slice = cursor.consume_while_m_n(1, None, |b| b.is_ascii_alphabetic())?;
//         let span = cursor.span_since(start);
//         Ok(Alpha(cursor.slice(span.start(), span.end())))
//     }
// }
//

pub type AlphaWord = Repeat<Alpha>;

// pub struct AlphaWord<'a>(&'a str);
//
// impl<'a> AlphaWord<'a> {
//     pub fn as_str(&self) -> &'a str {
//         self.0
//     }
// }
//
// impl<'a> Display for AlphaWord<'a> {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }
//
// impl<'a> ParseChars<'a> for AlphaWord<'a> {
//     fn parse(cursor: &mut Cursor<'a>) -> CharLexerResult<Self> {
//         let checkpoint = cursor.checkpoint();
//         let end = cursor.consume_while(|c| c.is_ascii_alphabetic());
//         let span = cursor.span_since(checkpoint);
//         if end.is_empty() {
//             Err(LexerError::UnexpectedEof {
//                 expected: "str".to_string(),
//                 span,
//             })
//         } else {
//             Ok(AlphaWord(span.as_slice(cursor)))
//         }
//     }
// }

// pub struct AlphaNum<'a>(&'a str);
//
// impl<'a> AlphaNum<'a> {
//     pub fn as_str(&self) -> &'a str {
//         self.0
//     }
// }
//
// impl<'a> Display for AlphaNum<'a> {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }
//
// impl<'a> ParseChars<'a> for AlphaNum<'a> {
//     fn parse(cursor: &mut Cursor<'a>) -> CharLexerResult<Self> {
//         let checkpoint = cursor.checkpoint();
//         let end = cursor.consume_exact(1, |c| c.is_ascii_alphanumeric())?;
//         let span = cursor.span_since(checkpoint);
//         if end.is_empty() {
//             Err(LexerError::UnexpectedEof {
//                 expected: "str".to_string(),
//                 span,
//             })
//         } else {
//             Ok(AlphaNum(span.as_slice(cursor)))
//         }
//     }
// }

// pub struct Num<'a>(&'a str);

// impl<'a> Num<'a> {
//     pub fn as_str(&self) -> &'a str {
//         self.0
//     }
// }

// impl<'a> Display for Num<'a> {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }
//
// impl<'a> ParseChars<'a> for Num<'a> {
//     fn parse(cursor: &mut Cursor<'a>) -> CharLexerResult<Self> {
//         let checkpoint = cursor.checkpoint();
//         let end = cursor.consume_exact(1, |c| c.is_ascii_digit())?;
//         let span = cursor.span_since(checkpoint);
//         if end.is_empty() {
//             Err(LexerError::UnexpectedEof {
//                 expected: "str".to_string(),
//                 span,
//             })
//         } else {
//             Ok(Num(span.as_slice(cursor)))
//         }
//     }
// }

pub type AlphaNum = Either<Alpha, TDigit>;

// impl AlphaNum<'_> {
//     pub fn as_str(&self) -> &str {
//         match self {
//             Either::Left(alpha) => alpha.as_str(),
//             Either::Right(digit) => digit.as_c(),
//         }
//     }
// }

pub type AlphaNumWord = Repeat<AlphaNum>;

// pub struct AlphaNumWord<'a>(&'a str);

// pub type hh

// impl<'a> AlphaNumWord<'a> {
//     pub fn as_str(&self) -> &'a str {
//         self.0
//     }
// }
//
// impl<'a> Display for AlphaNumWord<'a> {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }
//
// impl<'a> ParseChars<'a> for AlphaNumWord<'a> {
//     fn parse(cursor: &mut Cursor<'a>) -> CharLexerResult<Self> {
//         let checkpoint = cursor.checkpoint();
//         let end = cursor.consume_while(|c| c.is_ascii_alphanumeric());
//         let span = cursor.span_since(checkpoint);
//         if end.is_empty() {
//             Err(LexerError::UnexpectedEof {
//                 expected: "str".to_string(),
//                 span,
//             })
//         } else {
//             Ok(AlphaNumWord(span.as_slice(cursor)))
//         }
//     }
// }
