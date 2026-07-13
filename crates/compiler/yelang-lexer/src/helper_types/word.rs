/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 04/02/2025
 */

// TODO: Replace this tuple
use crate::{
    CharCursor, CharLexerResult, ParseChars, ParseTokenStream, TokenError, TokenResult,
    TokenStream, TokenTrait,
};

macro_rules! gen_word {
    ($name:ident, $($param:ident),+ $(,)?) => {
        pub struct $name< $( const $param: char ),+ >;

        impl<TMeta, $( const $param: char ),+> ParseTokenStream<TMeta> for $name< $( $param ),+ >
        where
            TMeta: TokenTrait,
        {
            fn parse(tokenstream: &mut TokenStream<TMeta>) -> TokenResult<Self> {
                let mut word = String::new();
                $(
                    word.push($param);
                )+

                match tokenstream.peek() {
                    Some(t) => {
        // We should be able to do byte-by-byte comparison here
                        let tokenstr = t.kind().to_string();
                        if word == tokenstr {
                            tokenstream.advance()
                        } else {
                            return Err(TokenError::UnexpectedToken {
                                expected: word,
                                found: tokenstr,
                                span: t.span(),
                            });
                        }
                    },
                    None => {
                        return Err(TokenError::UnexpectedEof {
                            expected: word,
                            span: tokenstream.span(),
                        });
                    }
                };
                Ok($name::< $( $param ),+ >)
            }
        }


        impl<$( const $param: char ),+> ParseChars for $name< $( $param ),+ > {
            fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
                let mut word = String::new();
                $(
                    word.push($param);
                )+

                cursor.consume(&word)?;
                Ok($name::< $( $param ),+ >)
            }
        }
    };
}

gen_word!(Word1, _1);
gen_word!(Word2, _1, _2);
gen_word!(Word3, _1, _2, _3);
gen_word!(Word4, _1, _2, _3, _4);
gen_word!(Word5, _1, _2, _3, _4, _5);
gen_word!(Word6, _1, _2, _3, _4, _5, _6);
gen_word!(Word7, _1, _2, _3, _4, _5, _6, _7);
gen_word!(Word8, _1, _2, _3, _4, _5, _6, _7, _8);
gen_word!(Word9, _1, _2, _3, _4, _5, _6, _7, _8, _9);
gen_word!(Word10, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10);
gen_word!(Word11, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11);
gen_word!(Word12, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12);
gen_word!(
    Word13, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13
);
gen_word!(
    Word14, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14
);
gen_word!(
    Word15, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15
);
gen_word!(
    Word16, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16
);
gen_word!(
    Word17, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17
);
gen_word!(
    Word18, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18
);
gen_word!(
    Word19, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19
);
gen_word!(
    Word20, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20
);
gen_word!(
    Word21, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21
);
gen_word!(
    Word22, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22
);
gen_word!(
    Word23, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23
);
gen_word!(
    Word24, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24
);
gen_word!(
    Word25, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25
);
gen_word!(
    Word26, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26
);
gen_word!(
    Word27, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27
);
gen_word!(
    Word28, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27, _28
);
gen_word!(
    Word29, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27, _28, _29
);
gen_word!(
    Word30, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27, _28, _29, _30
);
gen_word!(
    Word31, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27, _28, _29, _30, _31
);
gen_word!(
    Word32, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27, _28, _29, _30, _31, _32
);
gen_word!(
    Word33, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27, _28, _29, _30, _31, _32, _33
);
gen_word!(
    Word34, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19,
    _20, _21, _22, _23, _24, _25, _26, _27, _28, _29, _30, _31, _32, _33, _34
);

// pub struct CharsAsWord<T1, T2> {
//     t1: T1,
//     t2: T2,
// }
//
// impl<T1, T2> CharsAsWord<T1, T2> {
//     pub fn new(t1: T1, t2: T2) -> Self {
//         Self { t1, t2 }
//     }
// }
//
// impl<const C1: char, const C2: char, TMeta> ParseTokenStream<TMeta, TMeta>
//     for CharsAsWord<Char<C1>, Char<C2>>
// where
//     TMeta: TokenTrait,
// {
//     fn parse(tokenstream: &mut TokenStream<TMeta>) -> TokenResult<TMeta> {
//         let word = format!("{C1}{C2}");
//         let res = match tokenstream.peek() {
//             Some(t) => {
//                 let tokenstr = t.kind().to_string();
//                 if word == tokenstr {
//                     tokenstream.advance()
//                 } else {
//                     return Err(TokenError::UnexpectedToken {
//                         expected: word,
//                         found: tokenstr,
//                         span: t.span(),
//                     });
//                 }
//             }
//             None => {
//                 return Err(TokenError::UnexpectedEof {
//                     expected: word,
//                     span: tokenstream.span(),
//                 })
//             }
//         };
//
//         Ok(res.expect("This is a bug. Please, report at github.com/oyelowo/yedb. This should not error since we have validated.").clone())
//     }
// }
//
// pub struct Word<const _1: char, const _2: char>;
//
// impl<const _1: char, const _2: char, TMeta> ParseTokenStream<TMeta> for Word<_1, _2>
// where
//     TMeta: TokenTrait,
// {
//     fn parse(tokenstream: &mut TokenStream<TMeta>) -> TokenResult<Self> {
//         let word = format!("{}{}", _1, _2);
//         let res = match tokenstream.peek() {
//             Some(t) => {
//                 let tokenstr = t.kind().to_string();
//                 if word == tokenstr {
//                     tokenstream.advance()
//                 } else {
//                     return Err(TokenError::UnexpectedToken {
//                         expected: word,
//                         found: tokenstr,
//                         span: t.span(),
//                     });
//                 }
//             }
//             None => {
//                 return Err(TokenError::UnexpectedEof {
//                     expected: word,
//                     span: tokenstream.span(),
//                 })
//             }
//         };
//
//         // Ok(Word::new(Char::<C>::new(), Char::<S>::new()))
//         // Ok(res.expect("This is a bug. Please, report at github.com/oyelowo/yedb. This should not error since we have validated.").clone())
//         Ok(Word::<_1, _2>)
//     }
// }
//
