/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::{ByteCursor, ByteLexerError};

pub trait ParseBytes<'a, TOutput = Self>
where
    Self: Sized,
    TOutput: ParseBytes<'a>,
{
    fn parse(cursor: &mut ByteCursor<'a>) -> Result<Self, ByteLexerError>;
}
