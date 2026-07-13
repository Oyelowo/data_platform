/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerResult, ParseChars, ParseTokenStream, TokenResult, TokenStream, TokenTrait,
};

#[derive(Debug, Clone)]
pub enum Either<A, B> {
    Left(A),
    Right(B),
}

impl<A, B> Either<A, B> {
    pub fn is_left(&self) -> bool {
        matches!(self, Either::Left(_))
    }

    pub fn is_right(&self) -> bool {
        matches!(self, Either::Right(_))
    }

    pub fn as_left(&self) -> Option<&A> {
        if let Either::Left(a) = self {
            Some(a)
        } else {
            None
        }
    }

    pub fn as_right(&self) -> Option<&B> {
        if let Either::Right(b) = self {
            Some(b)
        } else {
            None
        }
    }

    pub fn into_left(self) -> Option<A> {
        if let Either::Left(a) = self {
            Some(a)
        } else {
            None
        }
    }

    pub fn into_right(self) -> Option<B> {
        if let Either::Right(b) = self {
            Some(b)
        } else {
            None
        }
    }
}

impl<A, B> ParseChars for Either<A, B>
where
    A: ParseChars,
    B: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        cursor
            .parse::<A>()
            .map(Either::Left)
            // Important to evaluate this lazily so we use a closure or use above approach if later
            // preferred
            .or_else(|_| cursor.parse::<B>().map(Either::Right))
    }
}

// TODO: Consider using macro to generate these arbitrarily
impl<A, B, T: TokenTrait> ParseTokenStream<T> for Either<A, B>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        tokenstream
            .parse::<A>()
            .map(Either::Left)
            .or_else(|_| tokenstream.parse::<B>().map(Either::Right))
    }
}

// impl<A, B> ParseChars for Either<A, B>
// where
//     A: ParseChars,
//     B: ParseChars,
// {
//     fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
//         let a = match cursor.parse::<A>() {
//             Ok(val) => return Ok(Self::Left(val)),
//             Err(e1) => e1,
//         };
//
//         let b = match cursor.parse::<B>() {
//             Ok(val) => return Ok(Self::Right(val)),
//             Err(e2) => e2,
//         };
//
//         Err(a.merge(&b))
//     }
// }
//
