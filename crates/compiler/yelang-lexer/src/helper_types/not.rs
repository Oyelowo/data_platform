/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/02/2025
 */

use crate::{
    ByteCursor, ByteLexerError, CharCursor, CharLexerError, CharLexerResult, ParseBytes,
    ParseChars, ParseTokenStream, TokenError, TokenResult, TokenStream, TokenTrait,
};
use std::{fmt::Debug, marker::PhantomData};

// TODO: should I implement VerifyNot? of just Verify<Not<P>>? or maybe Restore<Not<P>>?
#[derive(Debug, Clone, PartialEq)]
pub struct PeekNot<P>(PhantomData<P>);

impl<P> ParseChars for PeekNot<P>
where
    P: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(s) => {
                let slice = cursor.span_since(checkpoint);
                cursor.restore(checkpoint);
                Err(CharLexerError::UnexpectedChar {
                    expected: format!("anything but {}", slice),
                    found: cursor.peek().unwrap(),
                    span: cursor.current_span(),
                })
            }
            Err(_) => Ok(PeekNot(PhantomData)),
        }
    }
}

impl<'a, P> ParseBytes<'a> for PeekNot<P>
where
    P: ParseBytes<'a> + Debug,
{
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(s) => {
                let slice = cursor.span_since(checkpoint);
                cursor.restore(checkpoint);
                Err(ByteLexerError::UnexpectedByte {
                    expected: format!("anything but {:?}", slice),
                    found: cursor.peek().unwrap(),
                    span: cursor.current_span(),
                })
            }
            Err(_) => Ok(PeekNot(PhantomData)),
        }
    }
}

impl<A, T> ParseTokenStream<T> for PeekNot<A>
where
    A: ParseTokenStream<T>,
    T: TokenTrait,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        match tokenstream.parse::<A>() {
            Ok(s) => {
                tokenstream.restore(checkpoint);
                let slice = tokenstream.slice_since(checkpoint);
                Err(TokenError::UnexpectedToken {
                    expected: format!("anything but {:?}", slice),
                    found: format!("{:?}", tokenstream),
                    span: tokenstream.current_span(),
                })
            }
            Err(_) => Ok(PeekNot(PhantomData)),
        }
    }
}
