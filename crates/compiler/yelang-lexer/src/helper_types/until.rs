/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerError, ParseChars, ParseTokenStream, TokenError, TokenResult, TokenStream,
    TokenTrait,
};

pub struct Until<A>(A);

impl<A> ParseChars for Until<A>
where
    A: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        // let mut cursor = cursor.clone();
        while cursor.parse::<A>().is_err() {
            cursor.advance();
        }

        // Ok(Until(A::parse(&mut cursor)?))
        Ok(Until(cursor.parse::<A>()?))
    }
}

impl<A, T: TokenTrait> ParseTokenStream<T> for Until<A>
where
    A: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        // let mut tokenstream = tokenstream.clone();
        while tokenstream.parse::<A>().is_err() {
            tokenstream.advance();
        }

        // Ok(Until(A::parse(&mut tokenstream)?))
        Ok(Until(tokenstream.parse::<A>()?))
    }
}

pub struct AUntilB<A, B>(Vec<A>, B);

impl<A, B> ParseChars for AUntilB<A, B>
where
    A: ParseChars,
    B: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let mut a_vec = Vec::new();
        while let Ok(a) = cursor.parse::<A>() {
            a_vec.push(a);
        }

        if cursor.is_eof() {
            return Err(CharLexerError::UnexpectedEof {
                expected: "B".to_string(),
                span: cursor.current_span(),
            });
        }

        let b = cursor.parse::<B>()?;
        Ok(AUntilB(a_vec, b))
    }
}

impl<A, B, T: TokenTrait> ParseTokenStream<T> for AUntilB<A, B>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        let mut a_vec = Vec::new();
        while let Ok(a) = tokenstream.parse::<A>() {
            a_vec.push(a);
        }

        if tokenstream.is_eof() {
            return Err(TokenError::UnexpectedEof {
                expected: "B".to_string(),
                span: tokenstream.current_span(),
            });
        }

        let b = tokenstream.parse::<B>()?;
        Ok(AUntilB(a_vec, b))
    }
}

pub struct AUntilB4B<A, B>(Vec<A>, B);

impl<A, B, T: TokenTrait> ParseTokenStream<T, Vec<A>> for AUntilB4B<A, B>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Vec<A>> {
        let mut a_vec = Vec::new();
        while let Ok(a) = tokenstream.parse::<A>() {
            a_vec.push(a);
        }

        if tokenstream.is_eof() {
            return Err(TokenError::UnexpectedEof {
                expected: "B".to_string(),
                span: tokenstream.current_span(),
            });
        }

        tokenstream.verify_type::<B>()?;

        Ok(a_vec)
    }
}
