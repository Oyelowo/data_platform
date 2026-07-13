/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerResult, ParseChars, ParseTokenStream, TokenResult, TokenStream, TokenTrait,
};

impl<T, U> ParseChars for (T, U)
where
    T: ParseChars,
    U: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<T>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        let second = match cursor.parse::<U>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((first, second))
    }
}

impl<T, U, To> ParseTokenStream<To> for (T, U)
where
    To: TokenTrait,
    T: ParseTokenStream<To>,
    U: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<T>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<U>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((first, second))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuple2<T, U>(pub T, pub U);

impl<T, U> Tuple2<T, U> {
    pub fn into_inner(self) -> (T, U) {
        (self.0, self.1)
    }
}

impl<T, U> ParseChars for Tuple2<T, U>
where
    T: ParseChars,
    U: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<T>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        let second = match cursor.parse::<U>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok(Tuple2(first, second))
    }
}

impl<T, U, To> ParseTokenStream<To> for Tuple2<T, U>
where
    To: TokenTrait,
    T: ParseTokenStream<To>,
    U: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<T>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<U>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok(Tuple2(first, second))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuple3<T, U, V>(pub T, pub U, pub V);

impl<T, U, V> ParseChars for Tuple3<T, U, V>
where
    T: ParseChars,
    U: ParseChars,
    V: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<T>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        let second = match cursor.parse::<U>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        let third = match cursor.parse::<V>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok(Tuple3(first, second, third))
    }
}

impl<T, U, V, To> ParseTokenStream<To> for Tuple3<T, U, V>
where
    To: TokenTrait,
    T: ParseTokenStream<To>,
    U: ParseTokenStream<To>,
    V: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<T>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<U>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<V>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok(Tuple3(first, second, third))
    }
}

impl<A, B, C> ParseChars for (A, B, C)
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((first, second, third))
    }
}

impl<A, B, C, To> ParseTokenStream<To> for (A, B, C)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((first, second, third))
    }
}

impl<A, B, C, D> ParseChars for (A, B, C, D)
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match cursor.parse::<D>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((first, second, third, fourth))
    }
}

impl<A, B, C, D, To> ParseTokenStream<To> for (A, B, C, D)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
    D: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match tokenstream.parse::<D>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((first, second, third, fourth))
    }
}

impl<A, B, C, D, E> ParseChars for (A, B, C, D, E)
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match cursor.parse::<D>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match cursor.parse::<E>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((first, second, third, fourth, fifth))
    }
}

impl<A, B, C, D, E, To> ParseTokenStream<To> for (A, B, C, D, E)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
    D: ParseTokenStream<To>,
    E: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match tokenstream.parse::<D>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match tokenstream.parse::<E>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((first, second, third, fourth, fifth))
    }
}

impl<A, B, C, D, E, F> ParseChars for (A, B, C, D, E, F)
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
    F: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match cursor.parse::<D>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match cursor.parse::<E>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match cursor.parse::<F>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((first, second, third, fourth, fifth, sixth))
    }
}

impl<A, B, C, D, E, F, To> ParseTokenStream<To> for (A, B, C, D, E, F)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
    D: ParseTokenStream<To>,
    E: ParseTokenStream<To>,
    F: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match tokenstream.parse::<D>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match tokenstream.parse::<E>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match tokenstream.parse::<F>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((first, second, third, fourth, fifth, sixth))
    }
}

impl<A, B, C, D, E, F, G> ParseChars for (A, B, C, D, E, F, G)
where
    A: ParseChars,
    B: ParseChars,
    C: ParseChars,
    D: ParseChars,
    E: ParseChars,
    F: ParseChars,
    G: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match cursor.parse::<D>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match cursor.parse::<E>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match cursor.parse::<F>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match cursor.parse::<G>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((first, second, third, fourth, fifth, sixth, seventh))
    }
}

impl<A, B, C, D, E, F, G, To> ParseTokenStream<To> for (A, B, C, D, E, F, G)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
    D: ParseTokenStream<To>,
    E: ParseTokenStream<To>,
    F: ParseTokenStream<To>,
    G: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match tokenstream.parse::<D>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match tokenstream.parse::<E>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match tokenstream.parse::<F>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match tokenstream.parse::<G>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((first, second, third, fourth, fifth, sixth, seventh))
    }
}

impl<A, B, C, D, E, F, G, H> ParseChars for (A, B, C, D, E, F, G, H)
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
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match cursor.parse::<D>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match cursor.parse::<E>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match cursor.parse::<F>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match cursor.parse::<G>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let eight = match cursor.parse::<H>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((first, second, third, fourth, fifth, sixth, seventh, eight))
    }
}

impl<A, B, C, D, E, F, G, H, To> ParseTokenStream<To> for (A, B, C, D, E, F, G, H)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
    D: ParseTokenStream<To>,
    E: ParseTokenStream<To>,
    F: ParseTokenStream<To>,
    G: ParseTokenStream<To>,
    H: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match tokenstream.parse::<D>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match tokenstream.parse::<E>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match tokenstream.parse::<F>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match tokenstream.parse::<G>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let eight = match tokenstream.parse::<H>() {
            Ok(e) => e,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((first, second, third, fourth, fifth, sixth, seventh, eight))
    }
}

impl<A, B, C, D, E, F, G, H, I> ParseChars for (A, B, C, D, E, F, G, H, I)
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
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match cursor.parse::<D>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match cursor.parse::<E>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match cursor.parse::<F>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match cursor.parse::<G>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let eight = match cursor.parse::<H>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let ninth = match cursor.parse::<I>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((
            first, second, third, fourth, fifth, sixth, seventh, eight, ninth,
        ))
    }
}

impl<A, B, C, D, E, F, G, H, I, To> ParseTokenStream<To> for (A, B, C, D, E, F, G, H, I)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
    D: ParseTokenStream<To>,
    E: ParseTokenStream<To>,
    F: ParseTokenStream<To>,
    G: ParseTokenStream<To>,
    H: ParseTokenStream<To>,
    I: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match tokenstream.parse::<D>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match tokenstream.parse::<E>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match tokenstream.parse::<F>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match tokenstream.parse::<G>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let eight = match tokenstream.parse::<H>() {
            Ok(e) => e,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let ninth = match tokenstream.parse::<I>() {
            Ok(n) => n,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((
            first, second, third, fourth, fifth, sixth, seventh, eight, ninth,
        ))
    }
}

impl<A, B, C, D, E, F, G, H, I, J> ParseChars for (A, B, C, D, E, F, G, H, I, J)
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
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();

        let first = match cursor.parse::<A>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match cursor.parse::<B>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match cursor.parse::<C>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match cursor.parse::<D>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match cursor.parse::<E>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match cursor.parse::<F>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match cursor.parse::<G>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let eighth = match cursor.parse::<H>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let ninth = match cursor.parse::<I>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };
        let tenth = match cursor.parse::<J>() {
            Ok(v) => v,
            Err(e) => {
                cursor.restore(checkpoint);
                return Err(e);
            }
        };

        Ok((
            first, second, third, fourth, fifth, sixth, seventh, eighth, ninth, tenth,
        ))
    }
}

impl<A, B, C, D, E, F, G, H, I, J, To> ParseTokenStream<To> for (A, B, C, D, E, F, G, H, I, J)
where
    To: TokenTrait,
    A: ParseTokenStream<To>,
    B: ParseTokenStream<To>,
    C: ParseTokenStream<To>,
    D: ParseTokenStream<To>,
    E: ParseTokenStream<To>,
    F: ParseTokenStream<To>,
    G: ParseTokenStream<To>,
    H: ParseTokenStream<To>,
    I: ParseTokenStream<To>,
    J: ParseTokenStream<To>,
{
    fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
        let checkpoint = tokenstream.checkpoint();
        let first = match tokenstream.parse::<A>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let second = match tokenstream.parse::<B>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let third = match tokenstream.parse::<C>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fourth = match tokenstream.parse::<D>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let fifth = match tokenstream.parse::<E>() {
            Ok(f) => f,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let sixth = match tokenstream.parse::<F>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let seventh = match tokenstream.parse::<G>() {
            Ok(s) => s,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let eighth = match tokenstream.parse::<H>() {
            Ok(e) => e,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let ninth = match tokenstream.parse::<I>() {
            Ok(n) => n,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        let tenth = match tokenstream.parse::<J>() {
            Ok(t) => t,
            Err(e) => {
                tokenstream.restore(checkpoint);
                return Err(e);
            }
        };
        Ok((
            first, second, third, fourth, fifth, sixth, seventh, eighth, ninth, tenth,
        ))
    }
}
