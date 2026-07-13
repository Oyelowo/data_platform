/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */
use super::{CharCursor, CharLexerResult, CharStream, errors::CharLexerError};
use std::io::Read;

pub trait ParseChars {
    fn parse(cursor: &mut CharCursor<'_>) -> Result<Self, CharLexerError>
    where
        Self: Sized;

    fn parse_restored(cursor: &mut CharCursor<'_>) -> CharLexerResult<Self>
    where
        Self: Sized,
    {
        Self::parse(cursor)
    }
}

pub trait ParseCharStream {
    fn parse<R: Read>(stream: &mut CharStream<R>) -> std::io::Result<Self>
    where
        Self: Sized;
}

pub trait ParseCharStreamAsync {
    fn parse<R: tokio::io::AsyncRead + Unpin>(
        stream: &mut crate::chars::stream_async::AsyncCharStream<R>,
    ) -> tokio::io::Result<Self>
    where
        Self: Sized;
}
