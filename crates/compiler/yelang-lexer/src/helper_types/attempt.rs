/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/01/2026
 */

use crate::{
    ByteCursor, ByteLexerError, CharCursor, CharLexerResult, ParseBytes, ParseChars,
    ParseTokenStream, TokenResult, TokenStream, TokenTrait,
};
use std::fmt::Debug;

/// Backtracking parser wrapper.
///
/// - On success: commits the parse (cursor stays advanced).
/// - On failure: restores the cursor/tokenstream to the checkpoint.
///
/// This is useful to express "try A, else try B" style alternatives declaratively
/// without leaking partial consumption from a failed branch.
#[derive(Debug, Clone)]
pub struct Attempt<P>(pub P);

impl<P> Attempt<P> {
    pub fn into_inner(self) -> P {
        self.0
    }
}

impl<P> ParseChars for Attempt<P>
where
    P: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(res) => Ok(Attempt(res)),
            Err(e) => {
                cursor.restore(checkpoint);
                Err(e)
            }
        }
    }
}

impl<'a, P> ParseBytes<'a> for Attempt<P>
where
    P: ParseBytes<'a>,
{
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<P>() {
            Ok(res) => Ok(Attempt(res)),
            Err(e) => {
                cursor.restore(checkpoint);
                Err(e)
            }
        }
    }
}

impl<A, T: TokenTrait> ParseTokenStream<T> for Attempt<A>
where
    A: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        match tokenstream.parse::<A>() {
            Ok(res) => Ok(Attempt(res)),
            Err(e) => {
                tokenstream.restore(checkpoint);
                Err(e)
            }
        }
    }
}
