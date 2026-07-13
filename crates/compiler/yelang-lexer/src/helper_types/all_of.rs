/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 05/02/2025
 */

use crate::{
    ByteCursor, ByteLexerError, CharCursor, CharLexerError, CharLexerResult, ParseBytes,
    ParseChars, ParseTokenStream, TokenError, TokenResult, TokenStream, TokenTrait,
};
use std::{fmt::Debug, marker::PhantomData, ops::Deref};

#[derive(Debug, Clone)]
pub struct All<P>(P);

impl<P> All<P> {
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
        All(inner)
    }
}

impl<P> Deref for All<P> {
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<P> ParseChars for All<P>
where
    P: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(res) => {
                if cursor.is_eof() {
                    Ok(All(res))
                } else {
                    cursor.restore(checkpoint);
                    Err(CharLexerError::UnexpectedEof {
                        expected: "EOF".to_string(),
                        span: cursor.span_since(checkpoint),
                    })
                }
            }
            Err(e) => {
                cursor.restore(checkpoint);
                Err(e)
            }
        }
    }
}

impl<'a, P> ParseBytes<'a> for All<P>
where
    P: ParseBytes<'a>,
{
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(res) => {
                if cursor.is_eof() {
                    Ok(All(res))
                } else {
                    cursor.restore(checkpoint);
                    Err(ByteLexerError::UnexpectedEof {
                        expected: "EOF".to_string(),
                        span: cursor.span_since(checkpoint),
                    })
                }
            }
            Err(e) => {
                cursor.restore(checkpoint);
                Err(e)
            }
        }
    }
}

impl<A, T: TokenTrait> ParseTokenStream<T> for All<A>
where
    A: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        match tokenstream.parse::<A>() {
            Ok(res) => {
                if tokenstream.is_eof() {
                    Ok(All(res))
                } else {
                    tokenstream.restore(checkpoint);
                    Err(TokenError::UnexpectedEof {
                        expected: "EOF".to_string(),
                        span: tokenstream.span_since(checkpoint),
                    })
                }
            }
            Err(e) => {
                tokenstream.restore(checkpoint);
                Err(e)
            }
        }
    }
}
