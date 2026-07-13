/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerError, CharLexerResult, ParseChars, ParseTokenStream, TokenResult,
};

use super::{Either, RepeatExact, RepeatMinMax, SeparatedList, SurroundedBy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Byte<const B: u8>;

impl<const B: u8> ParseChars for Byte<B> {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        match cursor.peek() {
            Some(c) if c as u8 == B => {
                cursor.advance();
                Ok(Byte)
            }
            Some(c) => Err(CharLexerError::UnexpectedChar {
                expected: format!("{}", B as char),
                found: c,
                span: cursor.current_span(),
            }),
            None => Err(CharLexerError::UnexpectedEof {
                expected: format!("{}", B as char),
                span: cursor.current_span(),
            }),
        }
    }
}

// impl<const B: u8> TokenTrait for Byte<B> {
//     type Kind = u8;
//
//     fn kind(&self) -> &Self::Kind {
//         todo!()
//     }
//
//     fn span(&self) -> crate::Span {
//         todo!()
//     }
// }

type Comma = Byte<b'a'>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Char<const C: char>;

impl<const C: char> Char<C> {
    pub const fn new() -> Self {
        Char
    }

    pub fn as_char(&self) -> char {
        C
    }

    pub fn to_string(&self) -> String {
        self.as_char().to_string()
    }
}

pub const fn char<const C: char>(c: char) -> Char<C> {
    Char
}

impl<const C: char> ParseChars for Char<C> {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        match cursor.peek() {
            Some(ch) if ch == C => {
                cursor.advance();
                Ok(Char)
            }
            Some(ch) => Err(CharLexerError::UnexpectedChar {
                expected: C.to_string(),
                found: ch,
                span: cursor.current_span(),
            }),
            None => Err(CharLexerError::UnexpectedEof {
                expected: C.to_string(),
                span: cursor.current_span(),
            }),
        }
    }
}

// TODO:: since tokenstream.consume() now takes Into<TokenKind> rather than TokenKind
// maybe allow user to do this themselves? any Char impl to user token can
// be used in parsing user tokenstream and user can even determine more complex stuff
// e.g (Char<'a'>, Char<'s'>) can be determined to be a single token Token::As, rather than
// two tokens Token::A and Token::S
// and they dont even have to implement ParseTokenStream, just impl From<(Char<'a'>, Char<'s'>)>
// for TokenKind. and it will also be cheaper since we are not doing string comparison
// but just pattern matching.
// impl<const C: char, T: TokenTrait> ParseTokenStream<T> for Char<C> {
//     fn parse(cursor: &mut TokenStream<T>) -> TokenResult<Self> {
//         // TODO: the plan is to implement default tokens
//         // for alot or alost all the ascii characters
//         // So, there will be a very giant Token enum provided by default
//         // and the user can choose to use it or not by mapping to
//         // their own Token enum
//         // Char<C> will implement ParseTokenStream for all the characters of the giant Token enum
//         // Char<C> will implement for everything that implements into<Token> or ParseTokenStream
//         // i.e impl<const C: char, T: Into<Token>> ParseTokenStream<T> for Char<C>
//         // B
//         // Char<C> will then be implement for all the characters of then giant Token enum
//         // Bytes<B> will also be implemented for all the ascii characters that fits
//         // todo!()
//
//         match cursor.peek() {
//             Some(ch) if ch.kind().to_string() == C.to_string() => {
//                 cursor.advance();
//                 Ok(Char)
//             }
//             Some(ch) => Err(TokenError::UnexpectedToken {
//                 expected: C.to_string(),
//                 found: ch.kind().to_string(),
//                 span: cursor.span(),
//             }),
//             None => Err(TokenError::UnexpectedEof {
//                 expected: C.to_string().to_string(),
//                 span: cursor.span(),
//             }),
//         }
//     }
// }

// impl ParseTokenStream for anything i.e T that impl Into<TokenKind>
// impl<T> ParseTokenStream<T> for T
// where
//     // Tt: TokenTrait,
//     T: TokenTrait,
// {
//     fn parse(cursor: &mut TokenStream<T>) -> TokenResult<Self> {
//         // cursor.consume().map(|t| t.kind())
//         todo!()
//     }
// }

/// Emote is a token representing the emoji 😀
type Emote = Char<'😀'>;
type Emote2 = Char<'😀'>;

impl Char<'😀'> {
    pub fn parse(cursor: &mut CharCursor) -> TokenResult<Self> {
        todo!()
    }
}

type _1Bit = Either<Char<'0'>, Char<'1'>>;

type Binary = Either<Byte<b'0'>, Byte<b'1'>>;

type _4Bit = RepeatExact<4, SurroundedBy<_1Bit, Option<Char<'_'>>, Option<Char<'_'>>>>;

type Special = Either<Emote, Comma>;

fn xx() {
    let mut cursor = CharCursor::new("😀");
    let x = cursor.parse::<Emote>();
    let xx: Emote = x.unwrap();
    let z: Char<'😀'> = xx;
    // let _ = Emote::parse(&mut cursor);
}

// impl<const C: char> TokenTrait for Char<C> {
//     type Kind = char;
//
//     fn kind(&self) -> &Self::Kind {
//         todo!()
//     }
//
//     fn span(&self) -> crate::Span {
//         todo!()
//     }
// }

// impl<const C: char> ParseTokenStream<char> for LiteralCharToken<C> {
//     fn parse(cursor: &mut TokenStream<char>) -> TokenResult<Self> {
//         match cursor.peek() {
//             Some(ch) if ch == C => {
//                 cursor.advance();
//                 Ok(LiteralCharToken)
//             }
//             Some(ch) => Err(TokenError::UnexpectedToken {
//                 expected: C.to_string(),
//                 found: ch.to_string(),
//                 span: cursor.current_span(),
//             }),
//             None => Err(TokenError::UnexpectedEof {
//                 expected: C.to_string(),
//                 span: cursor.current_span(),
//             }),
//         }
//     }
// }
//
//
