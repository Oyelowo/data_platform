/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
pub struct ConsumeWhile<F>(F);

pub struct Map<P, F> {
    parser: P,
    f: F,
}

impl<'a, P: ParseChars<'a>, F, T> Parser<'a> for Map<P, F>
where
    F: Fn(P) -> T + Copy,
{
    type Output = T;

    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output> {
        let result = cursor.parse::<P>()?;
        Ok((self.f)(result))
    }
}

pub struct RepeatMN<P> {
    parser: P,
    min: usize,
    max: usize,
}

impl<'a, P: ParseChars<'a>> ParseChars<'a> for RepeatMN<P> {
    fn parse(cursor: &mut Cursor<'a>) -> Result<Self, LexerError> {
        todo!()
    }
}

impl<'a, P: ParseChars<'a>> Parser<'a> for RepeatMN<P> {
    type Output = Vec<P>;
    fn parse(&self, cursor: &mut Cursor<'a>) -> Result<Vec<P>, LexerError> {
        let mut results = Vec::with_capacity(self.min);
        let start = cursor.checkpoint();

        for _ in 0..self.max {
            match cursor.parse::<P>() {
                Ok(item) => results.push(item),
                Err(_) => break,
            }
        }

        if results.len() < self.min {
            Err(LexerError::InsufficientRepetition {
                expected: self.min,
                found: results.len(),
                span: cursor.span_since(start),
            })
        } else {
            Ok(results)
        }
    }
}

pub struct RepeatMin<P> {
    parser: P,
    min: usize,
}

impl<'a, P: ParseChars<'a>> Parser<'a> for RepeatMin<P> {
    type Output = Vec<P>;

    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output> {
        let mut results = Vec::new();
        let start = cursor.checkpoint();

        while let Ok(item) = cursor.parse::<P>() {
            results.push(item);
        }

        if results.len() < self.min {
            Err(LexerError::InsufficientRepetition {
                expected: self.min,
                found: results.len(),
                span: cursor.span_since(start),
            })
        } else {
            Ok(results)
        }
    }
}

pub struct SurroundedBy<P, L, R> {
    parser: P,
    left: L,
    right: R,
}

// pub trait ParserGeneralize<'a>: Sized {
//     type Input;
//     type Output;
//     type Error;
//
//     fn parse(&self, cursor: &mut Self::Input) -> Result<Self::Output, Self::Error>;
//
//     fn then<P>(self, other: P) -> Then<Self, P> {
//         Then {
//             first: self,
//             second: other,
//         }
//     }
//
//     fn or<P>(self, other: P) -> Or<Self, P> {
//         Or {
//             first: self,
//             second: other,
//         }
//     }
// }
//

pub trait Parser<'a>: Sized {
    type Output;

    /// Parse the input and return the result or an error.
    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output>;

    /// Combine two parsers sequentially.
    fn then<P>(self, other: P) -> Then<Self, P>
    where
        P: Parser<'a>,
    {
        Then {
            first: self,
            second: other,
        }
    }

    /// Try multiple parsers and return the first successful result.
    fn or<P>(self, other: P) -> Or<Self, P>
    where
        P: Parser<'a, Output = Self::Output>,
    {
        Or {
            first: self,
            second: other,
        }
    }

    // fn repeat_m_n(self, min: usize, max: usize) -> RepeatMN<Self> {
    //     RepeatMN {
    //         parser: self,
    //         min,
    //         max,
    //     }
    // }
    //
    // fn repeat_exact(self, n: usize) -> RepeatMN<Self> {
    //     self.repeat_m_n(n, n)
    // }
    //
    // fn repeat_min(self, min: usize) -> RepeatMin<Self> {
    //     RepeatMin { parser: self, min }
    // }
    //
    // fn repeat_max(self, max: usize) -> RepeatMax<Self> {
    //     RepeatMax { parser: self, max }
    // }
    //
    // fn surrounded_by<L, R>(self, left: L, right: R) -> SurroundedBy<Self, L, R> {
    //     SurroundedBy {
    //         parser: self,
    //         left,
    //         right,
    //     }
    // }
    //
    // fn separated_list<S>(self, separator: S) -> SeparatedList<Self, S> {
    //     SeparatedList {
    //         parser: vec![],
    //         // separator,
    //         separator_count: 0,
    //         separator_marker: PhantomData,
    //         span: Span::default(),
    //     }
    // }
    //
    // fn map<F, T>(self, f: F) -> Map<Self, F>
    // where
    //     F: Fn(Self) -> T,
    // {
    //     Map { parser: self, f }
    // }
}

/// Combinator for sequencing parsers
pub struct Then<A, B> {
    first: A,
    second: B,
}

impl<'a, A, B> Parser<'a> for Then<A, B>
where
    A: Parser<'a>,
    B: Parser<'a>,
{
    type Output = (A::Output, B::Output);

    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output> {
        let a = self.first.parse(cursor)?;
        let b = self.second.parse(cursor)?;
        Ok((a, b))
    }
}

/// Combinator for alternation
pub struct Or<A, B> {
    first: A,
    second: B,
}

impl<'a, A, B> Parser<'a> for Or<A, B>
where
    A: Parser<'a>,
    B: Parser<'a, Output = A::Output>,
{
    type Output = A::Output;

    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output> {
        let checkpoint = cursor.checkpoint();
        match self.first.parse(cursor) {
            Ok(result) => Ok(result),
            Err(_) => {
                cursor.restore(checkpoint);
                self.second.parse(cursor)
            }
        }
    }
}

/// Fundamental parser for a specific character
pub struct CharParser(pub(crate) char);

impl<'a> Parser<'a> for CharParser {
    type Output = char;

    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output> {
        let start = cursor.position();
        match cursor.peek() {
            Some(c) if c == self.0 => {
                cursor.advance();
                Ok(c)
            }
            Some(found) => Err(LexerError::UnexpectedChar {
                expected: self.0.to_string(),
                found,
                span: Span::new(start, cursor.position()),
            }),
            None => Err(LexerError::UnexpectedEof {
                expected: self.0.to_string(),
                span: Span::new(start, cursor.position()),
            }),
        }
    }
}

/// Parser for sequences matching a predicate
pub struct TakeWhileParser<F>(pub(crate) F);

impl<'a, F> Parser<'a> for TakeWhileParser<F>
where
    F: Fn(char) -> bool + Copy,
{
    type Output = &'a str;

    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output> {
        let start = cursor.position().absolute;
        cursor.consume_while(self.0);
        let end = cursor.position().absolute;
        Ok(&cursor.input()[start..end])
    }
}

impl<'a, P: ParseChars<'a>, L: ParseChars<'a>, R: ParseChars<'a>> Parser<'a>
    for SurroundedBy<P, L, R>
{
    type Output = P;

    fn parse(&self, cursor: &mut Cursor<'a>) -> Result<P, LexerError> {
        cursor.parse::<L>()?;
        let result = cursor.parse::<P>()?;
        cursor.parse::<R>()?;
        Ok(result)
    }
}

pub struct RepeatMax<P> {
    parser: P,
    max: usize,
}

impl<'a, P: ParseChars<'a>> Parser<'a> for RepeatMax<P> {
    type Output = Vec<P>;

    fn parse(&self, cursor: &mut Cursor<'a>) -> CharLexerResult<Self::Output> {
        let mut results = Vec::with_capacity(self.max);
        let start = cursor.checkpoint();

        for _ in 0..self.max {
            match cursor.parse::<P>() {
                Ok(item) => results.push(item),
                Err(_) => break,
            }
        }

        Ok(results)
    }
}

// Reusable parser for lists like `[a, b, c]`
// fn list_parser<T>(
//     item_parser: impl Fn(&mut Cursor) -> Result<T, TokenError>,
// ) -> impl Fn(&mut Cursor) -> Result<Vec<T>, TokenError> {
//     move |cursor| {
//         cursor.expect('[')?;
//         let items = separated_list(',', &item_parser)(cursor)?;
//         cursor.expect(']')?;
//         Ok(items)
//     }
// }
//
// // let array_parser = list_parser(number_literal_parser);

// impl<F, T> Parser<Cursor<'_>, char> for F
// where
//     F: Fn(&mut Cursor) -> Result<T, LexError>,
// {
//     type Output = T;
//
//     fn parse(&self, cursor: &mut Cursor) -> Result<Self::Output, TokenError> {
//         (self)(cursor).map_err(TokenError::from)
//     }
// }
//
// // impl<F, T> Parser<TokenStream<'_>, Token> for F
// // where
// //     F: Fn(&mut TokenStream) -> Result<T, TokenError>,
// // {
// //     type Output = T;
// //
// //     fn parse(&self, cursor: &mut TokenStream) -> Result<Self::Output, TokenError> {
// //         (self)(cursor)
// //     }
// // }
//
// pub struct Then<A, B> {
//     first: A,
//     second: B,
// }
//
// impl<C, T, A, B> Parser<C, T> for Then<A, B>
// where
//     A: Parser<C, T>,
//     B: Parser<C, T>,
// {
//     type Output = (A::Output, B::Output);
//
//     fn parse(&self, cursor: &mut C) -> Result<Self::Output, TokenError> {
//         let a = self.first.parse(cursor)?;
//         let b = self.second.parse(cursor)?;
//         Ok((a, b))
//     }
// }
//
// pub struct Or<A, B> {
//     first: A,
//     second: B,
// }
//
// impl<C, T, A, B> Parser<C, T> for Or<A, B>
// where
//     A: Parser<C, T>,
//     B: Parser<C, T, Output = A::Output>,
// {
//     type Output = A::Output;
//
//     fn parse(&self, cursor: &mut C) -> Result<Self::Output, TokenError> {
//         let checkpoint = cursor.checkpoint();
//         match self.first.parse(cursor) {
//             Ok(result) => Ok(result),
//             Err(e1) => {
//                 cursor.restore(checkpoint);
//                 match self.second.parse(cursor) {
//                     Ok(result) => Ok(result),
//                     Err(e2) => Err(e1.combine(e2)),
//                 }
//             }
//         }
//     }
// }
