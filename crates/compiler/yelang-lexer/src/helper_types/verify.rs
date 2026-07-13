/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 05/02/2025
 */

use crate::{
    ByteCursor, ByteLexerError, CharCursor, CharLexerResult, ParseBytes, ParseChars,
    ParseTokenStream, TokenResult, TokenStream, TokenTrait,
};
use std::{fmt::Debug, ops::Deref};

#[derive(Debug, Clone)]
pub struct Verify<P>(P);

impl<P> Verify<P> {
    pub fn into_inner(self) -> P {
        self.0
    }

    pub fn inner(&self) -> &P {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut P {
        &mut self.0
    }

    pub fn new(inner: P) -> Self {
        Verify(inner)
    }
}

impl<P> Deref for Verify<P> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<P> ParseChars for Verify<P>
where
    P: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(res) => {
                cursor.restore(checkpoint);
                Ok(Verify(res))
            }
            Err(e) => {
                cursor.restore(checkpoint);
                Err(e)
            }
        }
    }
}

impl<'a, P> ParseBytes<'a> for Verify<P>
where
    P: ParseBytes<'a>,
{
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(res) => {
                cursor.restore(checkpoint);
                Ok(Verify(res))
            }
            Err(e) => {
                cursor.restore(checkpoint);
                Err(e)
            }
        }
    }
}

impl<A, T: TokenTrait> ParseTokenStream<T> for Verify<A>
where
    A: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        match tokenstream.parse::<A>() {
            Ok(res) => {
                tokenstream.restore(checkpoint);
                Ok(Verify(res))
            }
            Err(e) => {
                tokenstream.restore(checkpoint);
                Err(e)
            }
        }
    }
}
