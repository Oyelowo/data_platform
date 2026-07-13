/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerError, ParseChars, ParseTokenStream, TokenResult, TokenStream, TokenTrait,
};

#[derive(Debug)]
pub enum OneOf2<A, B> {
    _1(A),
    _2(B),
}

impl<A, B> OneOf2<A, B> {
    pub fn _1(&self) -> Option<&A> {
        match self {
            OneOf2::_1(a) => Some(a),
            _ => None,
        }
    }

    pub fn _2(&self) -> Option<&B> {
        match self {
            OneOf2::_2(b) => Some(b),
            _ => None,
        }
    }
}

impl<A, B> ParseChars for OneOf2<A, B>
where
    A: ParseChars,
    B: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf2::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf2::_2(val)),
            Err(e2) => e2,
        };

        Err(a.merge(&b))

        // let a = cursor.parse::<A>().map(OneOf3::First);
        // let b = cursor.parse::<B>().map(OneOf3::Second);
        // let c = cursor.parse::<C>().map(OneOf3::Third);
        //
        // a.or_else(|s| b.map_err(|e| s.merge(&e)))
        //     .or_else(|s| c.map_err(|e| s.merge(&e)))
    }
}

impl<A, B, T: TokenTrait> ParseTokenStream<T> for OneOf2<A, B>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        tokenstream.parse::<A>().map(OneOf2::_1).or_else(|s| {
            tokenstream
                .parse::<B>()
                .map(OneOf2::_2)
                .map_err(|e| s.merge(e))
        })
    }
}

#[derive(Debug)]
pub enum OneOf3<A, B, C> {
    _1(A),
    _2(B),
    _3(C),
}

impl<A, B, C> OneOf3<A, B, C> {
    pub fn _1(&self) -> Option<&A> {
        match self {
            OneOf3::_1(a) => Some(a),
            _ => None,
        }
    }

    pub fn _2(&self) -> Option<&B> {
        match self {
            OneOf3::_2(b) => Some(b),
            _ => None,
        }
    }

    pub fn _3(&self) -> Option<&C> {
        match self {
            OneOf3::_3(c) => Some(c),
            _ => None,
        }
    }
}

impl<A, B, C> ParseChars for OneOf3<A, B, C>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf3::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf3::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf3::_3(val)),
            Err(e3) => e3,
        };

        Err(a.merge(&b).merge(&c))

        // let a = cursor.parse::<A>().map(OneOf3::First);
        // let b = cursor.parse::<B>().map(OneOf3::Second);
        // let c = cursor.parse::<C>().map(OneOf3::Third);
        //
        // a.or_else(|s| b.map_err(|e| s.merge(&e)))
        //     .or_else(|s| c.map_err(|e| s.merge(&e)))
    }
}

impl<A, B, C, T: TokenTrait> ParseTokenStream<T> for OneOf3<A, B, C>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
    C: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        tokenstream
            .parse::<A>()
            .map(OneOf3::_1)
            .or_else(|s| {
                tokenstream
                    .parse::<B>()
                    .map(OneOf3::_2)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<C>()
                    .map(OneOf3::_3)
                    .map_err(|e| s.merge(e))
            })
    }
}

pub enum OneOf4<A, B, C, D> {
    _1(A),
    _2(B),
    _3(C),
    _4(D),
}

impl<A, B, C, D> ParseChars for OneOf4<A, B, C, D>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf4::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf4::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf4::_3(val)),
            Err(e3) => e3,
        };

        let d = match cursor.parse::<D>() {
            Ok(val) => return Ok(OneOf4::_4(val)),
            Err(e4) => e4,
        };

        Err(a.merge(&b).merge(&c).merge(&d))
    }
}

impl<A, B, C, D, T: TokenTrait> ParseTokenStream<T> for OneOf4<A, B, C, D>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
    C: ParseTokenStream<T>,
    D: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        tokenstream
            .parse::<A>()
            .map(OneOf4::_1)
            .or_else(|s| {
                tokenstream
                    .parse::<B>()
                    .map(OneOf4::_2)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<C>()
                    .map(OneOf4::_3)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<D>()
                    .map(OneOf4::_4)
                    .map_err(|e| s.merge(e))
            })
    }
}

pub enum OneOf5<A, B, C, D, E> {
    _1(A),
    _2(B),
    _3(C),
    _4(D),
    _5(E),
}

impl<A, B, C, D, E> ParseChars for OneOf5<A, B, C, D, E>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf5::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf5::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf5::_3(val)),
            Err(e3) => e3,
        };

        let d = match cursor.parse::<D>() {
            Ok(val) => return Ok(OneOf5::_4(val)),
            Err(e4) => e4,
        };

        let e = match cursor.parse::<E>() {
            Ok(val) => return Ok(OneOf5::_5(val)),
            Err(e5) => e5,
        };

        Err(a.merge(&b).merge(&c).merge(&d).merge(&e))
    }
}

impl<A, B, C, D, E, T: TokenTrait> ParseTokenStream<T> for OneOf5<A, B, C, D, E>
where
    T: TokenTrait,
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
    C: ParseTokenStream<T>,
    D: ParseTokenStream<T>,
    E: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        tokenstream
            .parse::<A>()
            .map(OneOf5::_1)
            .or_else(|s| {
                tokenstream
                    .parse::<B>()
                    .map(OneOf5::_2)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<C>()
                    .map(OneOf5::_3)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<D>()
                    .map(OneOf5::_4)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<E>()
                    .map(OneOf5::_5)
                    .map_err(|e| s.merge(e))
            })
    }
}

pub enum OneOf6<A, B, C, D, E, F> {
    _1(A),
    _2(B),
    _3(C),
    _4(D),
    _5(E),
    _6(F),
}

impl<A, B, C, D, E, F> ParseChars for OneOf6<A, B, C, D, E, F>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
    F: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf6::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf6::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf6::_3(val)),
            Err(e3) => e3,
        };

        let d = match cursor.parse::<D>() {
            Ok(val) => return Ok(OneOf6::_4(val)),
            Err(e4) => e4,
        };

        let e = match cursor.parse::<E>() {
            Ok(val) => return Ok(OneOf6::_5(val)),
            Err(e5) => e5,
        };

        let f = match cursor.parse::<F>() {
            Ok(val) => return Ok(OneOf6::_6(val)),
            Err(e6) => e6,
        };

        Err(a.merge(&b).merge(&c).merge(&d).merge(&e).merge(&f))
    }
}

#[derive(Debug)]
pub enum OneOf7<A, B, C, D, E, F, G> {
    _1(A),
    _2(B),
    _3(C),
    _4(D),
    _5(E),
    _6(F),
    _7(G),
}

impl<A, B, C, D, E, F, G> ParseChars for OneOf7<A, B, C, D, E, F, G>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
    F: ParseChars,
    G: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf7::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf7::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf7::_3(val)),
            Err(e3) => e3,
        };

        let d = match cursor.parse::<D>() {
            Ok(val) => return Ok(OneOf7::_4(val)),
            Err(e4) => e4,
        };

        let e = match cursor.parse::<E>() {
            Ok(val) => return Ok(OneOf7::_5(val)),
            Err(e5) => e5,
        };

        let f = match cursor.parse::<F>() {
            Ok(val) => return Ok(OneOf7::_6(val)),
            Err(e6) => e6,
        };

        let g = match cursor.parse::<G>() {
            Ok(val) => return Ok(OneOf7::_7(val)),
            Err(e7) => e7,
        };

        Err(a
            .merge(&b)
            .merge(&c)
            .merge(&d)
            .merge(&e)
            .merge(&f)
            .merge(&g))
    }
}

#[derive(Debug)]
pub enum OneOf8<A, B, C, D, E, F, G, H> {
    _1(A),
    _2(B),
    _3(C),
    _4(D),
    _5(E),
    _6(F),
    _7(G),
    _8(H),
}

impl<A, B, C, D, E, F, G, H> ParseChars for OneOf8<A, B, C, D, E, F, G, H>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
    F: ParseChars,
    G: ParseChars,
    H: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf8::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf8::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf8::_3(val)),
            Err(e3) => e3,
        };

        let d = match cursor.parse::<D>() {
            Ok(val) => return Ok(OneOf8::_4(val)),
            Err(e4) => e4,
        };

        let e = match cursor.parse::<E>() {
            Ok(val) => return Ok(OneOf8::_5(val)),
            Err(e5) => e5,
        };

        let f = match cursor.parse::<F>() {
            Ok(val) => return Ok(OneOf8::_6(val)),
            Err(e6) => e6,
        };

        let g = match cursor.parse::<G>() {
            Ok(val) => return Ok(OneOf8::_7(val)),
            Err(e7) => e7,
        };

        let h = match cursor.parse::<H>() {
            Ok(val) => return Ok(OneOf8::_8(val)),
            Err(e8) => e8,
        };

        Err(a
            .merge(&b)
            .merge(&c)
            .merge(&d)
            .merge(&e)
            .merge(&f)
            .merge(&g)
            .merge(&h))
    }
}

impl<A, B, C, D, E, F, G, H, T: TokenTrait> ParseTokenStream<T> for OneOf8<A, B, C, D, E, F, G, H>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
    C: ParseTokenStream<T>,
    D: ParseTokenStream<T>,
    E: ParseTokenStream<T>,
    F: ParseTokenStream<T>,
    G: ParseTokenStream<T>,
    H: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        tokenstream
            .parse::<A>()
            .map(OneOf8::_1)
            .or_else(|s| {
                tokenstream
                    .parse::<B>()
                    .map(OneOf8::_2)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<C>()
                    .map(OneOf8::_3)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<D>()
                    .map(OneOf8::_4)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<E>()
                    .map(OneOf8::_5)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<F>()
                    .map(OneOf8::_6)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<G>()
                    .map(OneOf8::_7)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<H>()
                    .map(OneOf8::_8)
                    .map_err(|e| s.merge(e))
            })
    }
}

#[derive(Debug)]
pub enum OneOf9<A, B, C, D, E, F, G, H, I> {
    _1(A),
    _2(B),
    _3(C),
    _4(D),
    _5(E),
    _6(F),
    _7(G),
    _8(H),
    _9(I),
}

impl<A, B, C, D, E, F, G, H, I> ParseChars for OneOf9<A, B, C, D, E, F, G, H, I>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
    F: ParseChars,
    G: ParseChars,
    H: ParseChars,
    I: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf9::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf9::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf9::_3(val)),
            Err(e3) => e3,
        };

        let d = match cursor.parse::<D>() {
            Ok(val) => return Ok(OneOf9::_4(val)),
            Err(e4) => e4,
        };

        let e = match cursor.parse::<E>() {
            Ok(val) => return Ok(OneOf9::_5(val)),
            Err(e5) => e5,
        };

        let f = match cursor.parse::<F>() {
            Ok(val) => return Ok(OneOf9::_6(val)),
            Err(e6) => e6,
        };

        let g = match cursor.parse::<G>() {
            Ok(val) => return Ok(OneOf9::_7(val)),
            Err(e7) => e7,
        };

        let h = match cursor.parse::<H>() {
            Ok(val) => return Ok(OneOf9::_8(val)),
            Err(e8) => e8,
        };

        let i = match cursor.parse::<I>() {
            Ok(val) => return Ok(OneOf9::_9(val)),
            Err(e9) => e9,
        };

        Err(a
            .merge(&b)
            .merge(&c)
            .merge(&d)
            .merge(&e)
            .merge(&f)
            .merge(&g)
            .merge(&h)
            .merge(&i))
    }
}

impl<A, B, C, D, E, F, G, H, I, T: TokenTrait> ParseTokenStream<T>
    for OneOf9<A, B, C, D, E, F, G, H, I>
where
    A: ParseTokenStream<T>,
    B: ParseTokenStream<T>,
    C: ParseTokenStream<T>,
    D: ParseTokenStream<T>,
    E: ParseTokenStream<T>,
    F: ParseTokenStream<T>,
    G: ParseTokenStream<T>,
    H: ParseTokenStream<T>,
    I: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        tokenstream
            .parse::<A>()
            .map(OneOf9::_1)
            .or_else(|s| {
                tokenstream
                    .parse::<B>()
                    .map(OneOf9::_2)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<C>()
                    .map(OneOf9::_3)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<D>()
                    .map(OneOf9::_4)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<E>()
                    .map(OneOf9::_5)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<F>()
                    .map(OneOf9::_6)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<G>()
                    .map(OneOf9::_7)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<H>()
                    .map(OneOf9::_8)
                    .map_err(|e| s.merge(e))
            })
            .or_else(|s| {
                tokenstream
                    .parse::<I>()
                    .map(OneOf9::_9)
                    .map_err(|e| s.merge(e))
            })
    }
}

pub enum OneOf10<A, B, C, D, E, F, G, H, I, J> {
    _1(A),
    _2(B),
    _3(C),
    _4(D),
    _5(E),
    _6(F),
    _7(G),
    _8(H),
    _9(I),
    _10(J),
}

impl<A, B, C, D, E, F, G, H, I, J> ParseChars for OneOf10<A, B, C, D, E, F, G, H, I, J>
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
    F: ParseChars,
    G: ParseChars,
    H: ParseChars,
    I: ParseChars,
    J: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let a = match cursor.parse::<A>() {
            Ok(val) => return Ok(OneOf10::_1(val)),
            Err(e1) => e1,
        };

        let b = match cursor.parse::<B>() {
            Ok(val) => return Ok(OneOf10::_2(val)),
            Err(e2) => e2,
        };

        let c = match cursor.parse::<C>() {
            Ok(val) => return Ok(OneOf10::_3(val)),
            Err(e3) => e3,
        };

        let d = match cursor.parse::<D>() {
            Ok(val) => return Ok(OneOf10::_4(val)),
            Err(e4) => e4,
        };

        let e = match cursor.parse::<E>() {
            Ok(val) => return Ok(OneOf10::_5(val)),
            Err(e5) => e5,
        };

        let f = match cursor.parse::<F>() {
            Ok(val) => return Ok(OneOf10::_6(val)),
            Err(e6) => e6,
        };

        let g = match cursor.parse::<G>() {
            Ok(val) => return Ok(OneOf10::_7(val)),
            Err(e7) => e7,
        };

        let h = match cursor.parse::<H>() {
            Ok(val) => return Ok(OneOf10::_8(val)),
            Err(e8) => e8,
        };

        let i = match cursor.parse::<I>() {
            Ok(val) => return Ok(OneOf10::_9(val)),
            Err(e9) => e9,
        };

        let j = match cursor.parse::<J>() {
            Ok(val) => return Ok(OneOf10::_10(val)),
            Err(e10) => e10,
        };

        Err(a
            .merge(&b)
            .merge(&c)
            .merge(&d)
            .merge(&e)
            .merge(&f)
            .merge(&g)
            .merge(&h)
            .merge(&i)
            .merge(&j))

        // let a = cursor.parse::<A>().map(OneOf10::First);
        // let b = cursor.parse::<B>().map(OneOf10::Second);
        //        let c = cursor.parse::<C>().map(OneOf10::Third);
        //      let d = cursor.parse::<D>().map(OneOf10::Fourth);
        //      let e = cursor.parse::<E>().map(OneOf10::Fifth);
        //
        //      a.or_else(|s| b.map_err(|e| s.merge(e)))
        //      .or_else(|s| c.map_err(|e| s.merge(e)))
        //
        //      .or_else(|s| d.map_err(|e| s.merge(e)))
        //      .or_else(|s| e.map_err(|e| s.merge(e)))
    }
}
