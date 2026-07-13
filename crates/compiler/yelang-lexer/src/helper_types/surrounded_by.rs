/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerError, ParseChars, ParseTokenStream, Span, TokenResult, TokenStream,
    TokenTrait,
};

pub struct SurroundedBy<TLeft, TContent, TRight> {
    parser: TContent,
    parser_span: Span,
    left: TLeft,
    right: TRight,
}

impl<TLeft, TContent, TRight> SurroundedBy<TLeft, TContent, TRight> {
    // pub fn new(parser: TContent, left: TLeft, right: TRight) -> Self {
    //     Self {
    //         parser,
    //         left,
    //         right,
    //     }
    // }

    pub fn content(&self) -> &TContent {
        &self.parser
    }

    pub fn content_owned(self) -> TContent {
        self.parser
    }

    pub fn content_mut(&mut self) -> &mut TContent {
        &mut self.parser
    }

    pub fn left(&self) -> &TLeft {
        &self.left
    }

    pub fn right(&self) -> &TRight {
        &self.right
    }
}

impl<TLeft, TContent, TRight> SurroundedBy<TLeft, TContent, TRight> {
    fn value(&self) -> &TContent {
        &self.parser
    }
}

impl<L, TContent, R> ParseChars for SurroundedBy<L, TContent, R>
where
    TContent: ParseChars,
    L: ParseChars,
    R: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let left = cursor.parse::<L>()?;
        let (parser, span) = cursor.parse_with_span::<TContent>()?;
        let right = cursor.parse::<R>()?;

        Ok(Self {
            parser,
            parser_span: span,
            left,
            right,
        })
    }
}

impl<L, TContent, R, TKind> ParseTokenStream<TKind> for SurroundedBy<L, TContent, R>
where
    TContent: ParseTokenStream<TKind>,
    L: ParseTokenStream<TKind>,
    R: ParseTokenStream<TKind>,
    TKind: TokenTrait,
{
    fn parse(tokenstream: &mut TokenStream<TKind>) -> TokenResult<Self> {
        let left = tokenstream.parse::<L>()?;
        let (parser, span) = tokenstream.parse_with_span::<TContent>()?;
        let right = tokenstream.parse::<R>()?;

        Ok(Self {
            parser,
            parser_span: span,
            left,
            right,
        })
    }
}
