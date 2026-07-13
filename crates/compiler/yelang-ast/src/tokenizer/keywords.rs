/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 * Date 31/12/2024
 */
// use std::fmt::{self, Display, Formatter};
//
// macro_rules! keyword_enum {
//     ($($name:ident => $str:expr),+ $(,)?) => {
//         #[derive(Debug, PartialEq, Eq, Clone, Copy)]
//         pub enum Keyword {
//             $($name),+
//         }
//
//         impl Keyword {
//             pub const fn as_str(self) -> &'static str {
//                 match self {
//                     $(Keyword::$name => $str),+
//                 }
//             }
//
//             pub const fn variants() -> &'static [Keyword] {
//                 &[
//                     $(Keyword::$name),+
//                 ]
//             }
//
//             pub const fn variants_str() -> &'static [&'static str] {
//                 &[
//                     $($str),+
//                 ]
//             }
//
//             pub fn from_str_slice(s: &str) -> Option<Self> {
//                 match s.to_ascii_uppercase().as_str() {
//                     $($str => Some(Keyword::$name)),+,
//                     _ => None
//                 }
//             }
//         }
//
//
//         $(
//             #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//             pub struct $name;
//
//             impl $name {
//                 pub fn as_symbol(&self) -> Keyword {
//                     Keyword::$name
//                 }
//             }
//
//             impl ParseInput for $name {
//                 fn parse(cursor: &mut Cursor) -> ::crate::lexer::CharLexerResult<Self> {
//                     cursor.consume($str)?;
//                     Ok($name)
//                 }
//             }
//
//             impl ParseTokenStream<TokenMeta> for $name {
//                 fn parse(tokenstream: &mut TokenStream<$crate::parser::TokenMeta>) -> ParserResult<Self> {
//                     if tokenstream.peek().map(|t| t.kind() == &TokenKind::Keyword(Keyword::$name)).unwrap_or(false) {
//                         tokenstream.advance();
//                         Ok($name)
//                     } else {
//                         Err(ParserError::UnexpectedToken {
//                             expected: Keyword::$name.to_string(),
//                             found: tokenstream.peek().map(|t| t.kind().to_string()).unwrap_or_else(|| "EOF".to_string()),
//                             span: tokenstream.span(),
//                         })
//                     }
//                 }
//             }
//
//
//         )+
//
//
//     };
// }
//
// keyword_enum! {
//     Select => "SELECT",
//     From => "FROM",
//     Where => "WHERE",
//     Group => "GROUP",
//     By => "BY",
//     Order => "ORDER",
//     Let => "LET",
//     As => "AS",
//     Or => "OR",
//     Limit => "LIMIT",
//     Link => "LINK",
//     Create => "CREATE",
//     Update => "UPDATE",
//     Insert => "INSERT",
//     Delete => "DELETE",
//     For => "FOR",
//     Range => "RANGE",
//     Enumerate => "ENUMERATE",
//     If => "IF",
//     Else => "ELSE",
//     While => "WHILE",
//     Loop => "LOOP",
//     Continue => "CONTINUE",
//     Break => "BREAK",
//     Yield => "YIELD",
//     Return => "RETURN",
//     And => "AND",
//     Not => "NOT",
//     Xor => "XOR",
//     Is => "IS",
//     In => "IN",
//     Null => "NULL",
//     True => "TRUE",
//     False => "FALSE",
// }
//
// impl Display for Keyword {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self.as_str())
//     }
// }
//
// impl ParseInput for Keyword {
//     fn parse(cursor: &mut Cursor) -> Result<Self, LexerError> {
//         let start = cursor.checkpoint();
//         let (_, span) = cursor.parse_with_span::<Repeat<Either<Alpha, T!["_"]>>>()?;
//         let ident = span.as_slice(cursor);
//
//         // let ident = cursor.consume_while(|c| c.is_ascii_alphabetic() || c == '_');
//         let normalized = ident.to_ascii_uppercase();
//         let keyword =
//             Keyword::from_str_slice(&normalized).ok_or_else(|| LexerError::UnknownKeyword {
//                 expected: Keyword::variants_str().join(", "),
//                 keyword: ident.to_string(),
//                 span: cursor.span_since(start),
//             })?;
//
//         // TODO: Do I really want to do this here? Perhaps at the top-level lexing phase?
//         // if let Some(next_char) = cursor.peek() {
//         //     if next_char.is_alphanumeric() || next_char == '_' {
//         //         return Err(LexerError::InvalidKeywordSuffix {
//         //             keyword: keyword.as_str().to_string(),
//         //             suffix: cursor
//         //                 .consume_while(|c| c.is_alphanumeric() || c == '_')
//         //                 .to_string(),
//         //             span: cursor.span_since(start),
//         //         });
//         //     }
//         // }
//         Ok(keyword)
//     }
// }
//
// #[derive(Debug, PartialEq)]
// pub struct ElseIf;
//
// impl ParseInput for ElseIf {
//     fn parse(cursor: &mut Cursor) -> Result<Self, LexerError> {
//         cursor.parse::<Keyword>()?.expect(Keyword::Else)?;
//         cursor.consume_space0();
//         cursor.parse::<Keyword>()?.expect(Keyword::If)?;
//         Ok(ElseIf)
//     }
// }
//
// trait KeywordExpect {
//     fn expect(self, expected: Keyword) -> Result<(), LexerError>;
// }
//
// impl KeywordExpect for Keyword {
//     fn expect(self, expected: Keyword) -> Result<(), LexerError> {
//         if self == expected {
//             Ok(())
//         } else {
//             Err(LexerError::UnknownKeyword {
//                 expected: expected.to_string(),
//                 keyword: self.as_str().to_string(),
//                 span: Span::default(),
//             })
//         }
//     }
// }
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use rstest::rstest;
//
//     #[rstest]
//     #[case("select", Keyword::Select)]
//     #[case("SELECT", Keyword::Select)]
//     #[case("SeLeCt", Keyword::Select)]
//     #[case("from", Keyword::From)]
//     #[case("WHERE", Keyword::Where)]
//     #[case("NuLl", Keyword::Null)]
//     fn test_keyword_parsing(#[case] input: &str, #[case] expected: Keyword) {
//         let mut cursor = Cursor::new(input);
//         let keyword = cursor.parse::<Keyword>().unwrap();
//         assert_eq!(keyword, expected);
//
//         cursor.reset_dangerous();
//         let keyword = cursor.parse_exact::<Keyword>().unwrap();
//         assert_eq!(keyword, expected);
//     }
//
//     #[rstest]
//     #[case("slect ")]
//     #[case("SxELECTx")]
//     #[case("SeLeCt_")]
//     #[case("fom")]
//     #[case(",WHERE")]
//     #[case("N5uLl")]
//     fn test_parse_exact_fail(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         let keyword = cursor.parse::<Keyword>().map_err(|s| s.to_string());
//         assert!(keyword.is_err());
//     }
//
//     #[rstest]
//     #[case("elect ")]
//     #[case("select(")]
//     #[case("select1")]
//     #[case("select_")]
//     #[case("select1")]
//     #[case("select_")]
//     #[case("select1")]
//     fn test_invalid_exact(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         let keyword = cursor.parse_exact::<Keyword>().map_err(|s| s.to_string());
//         assert!(keyword.is_err());
//     }
//
//     #[rstest]
//     #[case("selects")]
//     #[case("fromX")]
//     #[case("grouping")]
//     #[case("trueish")]
//     fn test_invalid_suffix(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         assert!(cursor.parse::<Keyword>().is_err());
//     }
//
//     #[rstest]
//     #[case("else if", ElseIf)]
//     #[case("ELSE IF", ElseIf)]
//     #[case("Else If", ElseIf)]
//     #[case("else   IF", ElseIf)]
//     fn test_else_if_valid(#[case] input: &str, #[case] expected: ElseIf) {
//         let mut cursor = Cursor::new(input);
//         assert_eq!(cursor.parse::<ElseIf>().unwrap(), expected);
//     }
//
//     #[rstest]
//     #[case("else")]
//     #[case("if")]
//     #[case("elseif")]
//     #[case("else.if")]
//     fn test_else_if_invalid(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         assert!(cursor.parse::<ElseIf>().is_err());
//     }
//
//     #[rstest]
//     #[case("select", Keyword::Select)]
//     #[case("from", Keyword::From)]
//     #[case("where", Keyword::Where)]
//     #[case("group", Keyword::Group)]
//     #[case("by", Keyword::By)]
//     #[case("order", Keyword::Order)]
//     #[case("let", Keyword::Let)]
//     #[case("as", Keyword::As)]
//     #[case("or", Keyword::Or)]
//     #[case("limit", Keyword::Limit)]
//     #[case("link", Keyword::Link)]
//     #[case("create", Keyword::Create)]
//     #[case("update", Keyword::Update)]
//     #[case("insert", Keyword::Insert)]
//     #[case("delete", Keyword::Delete)]
//     #[case("for", Keyword::For)]
//     #[case("range", Keyword::Range)]
//     #[case("enumerate", Keyword::Enumerate)]
//     #[case("if", Keyword::If)]
//     #[case("else", Keyword::Else)]
//     #[case("while", Keyword::While)]
//     #[case("loop", Keyword::Loop)]
//     #[case("continue", Keyword::Continue)]
//     #[case("break", Keyword::Break)]
//     #[case("yield", Keyword::Yield)]
//     #[case("return", Keyword::Return)]
//     #[case("and", Keyword::And)]
//     #[case("not", Keyword::Not)]
//     #[case("xor", Keyword::Xor)]
//     #[case("is", Keyword::Is)]
//     #[case("in", Keyword::In)]
//     #[case("null", Keyword::Null)]
//     #[case("true", Keyword::True)]
//     #[case("false", Keyword::False)]
//     fn test_all_keywords(#[case] input: &str, #[case] expected: Keyword) {
//         let mut cursor = Cursor::new(input);
//         assert_eq!(
//             cursor.parse::<Keyword>().unwrap(),
//             expected,
//             "Failed for input: {}",
//             input
//         );
//
//         let ascii_uppercase = input.to_ascii_uppercase();
//         let mut cursor = Cursor::new(ascii_uppercase.as_str());
//         assert_eq!(
//             cursor.parse::<Keyword>().unwrap(),
//             expected,
//             "Failed for input: {}",
//             input
//         );
//     }
// }
