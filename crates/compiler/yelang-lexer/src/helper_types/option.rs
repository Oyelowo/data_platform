/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    ByteCursor, ByteLexerResult, CharCursor, CharLexerResult, ParseBytes, ParseChars,
    ParseTokenStream, TokenResult, TokenStream, TokenTrait,
};

impl<T: ParseChars> ParseChars for Option<T> {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<T>() {
            Ok(item) => Ok(Some(item)),
            Err(_) => {
                cursor.restore(checkpoint);
                Ok(None)
            }
        }
    }
}

impl<'a, T: ParseBytes<'a>> ParseBytes<'a> for Option<T> {
    fn parse(cursor: &mut ByteCursor<'a>) -> ByteLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        match cursor.parse::<T>() {
            Ok(item) => Ok(Some(item)),
            Err(_) => {
                cursor.restore(checkpoint);
                Ok(None)
            }
        }
    }
}

impl<T, TKind> ParseTokenStream<TKind> for Option<T>
where
    T: ParseTokenStream<TKind>,
    TKind: TokenTrait,
{
    fn parse(stream: &mut TokenStream<TKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        match stream.parse::<T>() {
            Ok(value) => Ok(Some(value)),
            Err(_) => {
                stream.restore(checkpoint);
                Ok(None)
            }
        }
    }
}
