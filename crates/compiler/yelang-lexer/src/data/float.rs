/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */
use crate::{
    Char, CharCursor, CharLexerResult, Either, OneOf2, OneOf3, OneOf4, OneOf10, ParseChars, Span,
    UIntLexed,
    word::{Word3, Word4},
};
use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatSuffix {
    F16,
    F32,
    F64,
    F128,
}

impl Display for FloatSuffix {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FloatSuffix::F16 => write!(f, "f16"),
            FloatSuffix::F32 => write!(f, "f32"),
            FloatSuffix::F64 => write!(f, "f64"),
            FloatSuffix::F128 => write!(f, "f128"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatLexed {
    span: Span,
    suffix: Option<FloatSuffix>,
}

impl FloatLexed {
    pub fn span(&self) -> Span {
        self.span
    }

    pub fn suffix(&self) -> Option<FloatSuffix> {
        self.suffix
    }
}

type Epsilon = Either<Char<'e'>, Char<'E'>>;
type Sign = Either<Char<'-'>, Char<'+'>>;
type Digit = OneOf10<
    Char<'0'>,
    Char<'1'>,
    Char<'2'>,
    Char<'3'>,
    Char<'4'>,
    Char<'5'>,
    Char<'6'>,
    Char<'7'>,
    Char<'8'>,
    Char<'9'>,
>;
// pub type IntUnsigned = (Digit, Option<Repeat<Either<Digit, Char<'_'>>>>);

type EpsilonPart = (Epsilon, Option<Sign>, UIntLexed);

// Case 1: Leading decimal (e.g., .69, .123E4)
type Leadingdecimal = (Char<'.'>, UIntLexed, Option<EpsilonPart>);

// Case 2: Scientific notation (e.g., 59e37, 5e-5, 94.17e-2, 3.14E+2, 94.17E264)
type Suffix1 = EpsilonPart;
type Suffix2 = (Char<'.'>, UIntLexed, EpsilonPart);
type ScientificNotation = (UIntLexed, Either<Suffix1, Suffix2>);

// Case 3: Trailing decimal (e.g., 54., 75.19)
type TrailingDecimal = (UIntLexed, Char<'.'>, Option<UIntLexed>);

type FloatSuffixWord = OneOf4<
    Word3<'f', '1', '6'>,
    Word3<'f', '3', '2'>,
    Word3<'f', '6', '4'>,
    Word4<'f', '1', '2', '8'>,
>;

impl ParseChars for FloatSuffix {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let word = cursor.parse::<FloatSuffixWord>()?;
        match word {
            OneOf4::_1(_) => Ok(FloatSuffix::F16),
            OneOf4::_2(_) => Ok(FloatSuffix::F32),
            OneOf4::_3(_) => Ok(FloatSuffix::F64),
            OneOf4::_4(_) => Ok(FloatSuffix::F128),
        }
    }
}

impl ParseChars for FloatLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        // .42	Leading decimal
        // 42.	Trailing decimal
        // 42.42	Standard float
        // -42.42	Negative float
        // 42e10	Scientific notation
        // -3.14e-2	Scientific with negative
        // +3.14	Positive sign
        // 3.14E+2	Uppercase exponent
        //
        let checkpoint = cursor.checkpoint();
        // let sign = cursor.parse::<Char<'-'>>().ok();
        // NOTE: We intentionally do NOT accept leading-dot floats like `.69`.
        // This avoids an ambiguity with Rust-style tuple field access (`t.0`, `t.1`, ...),
        // and is consistent with Rust, which requires `0.69`.
        type FloatType = OneOf2<
            // Case 2: Scientific notation (e.g., 59e37, 94.17e-2)
            ScientificNotation,
            // Case 3: Trailing decimal (e.g., 54., 75.19)
            TrailingDecimal,
        >;

        let (float, span) = cursor.parse_with_span::<FloatType>()?;
        let suffix = cursor.parse::<Option<FloatSuffix>>()?;

        let float = Self {
            // span: cursor.span_since(checkpoint),
            span,
            suffix,
        };
        // Alternative imperative approach
        // let float = cursor
        //     // Case 1: Leading decimal (e.g., .69, .123E4)
        //     .parse_as_str::<LeadingDecimal>()
        //     // Case 2: Scientific notation (e.g., 59e37, 94.17e-2)
        //     .or_else(|_| cursor.parse_as_str::<ScientificNotation>())
        //     // Case 3: Trailing decimal (e.g., 54., 75.19)
        //     .or_else(|_| cursor.parse_as_str::<TrailingDecimal>())
        //     .map(|span| {
        //         let value = span.parse::<f64>().expect(
        //             "Invalid float. This should never happen and is a \
        //                 bug in the compiler itself. Please, report on github at \
        //                 github.com/oyelowo/yedb",
        //         );
        //         let signed_value = match sign {
        //             Some(_) => -value,
        //             None => value,
        //         };
        //         Self {
        //             // Unwrap is safe because the float is guaranteed to be a valid float
        //             value: signed_value,
        //             span: cursor.span_since(checkpoint),
        //         }
        //     });

        Ok(float)
    }
}

mod float_parsed_value {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Float {
        value: f64,
        span: Span,
    }

    impl Display for Float {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.value)
        }
    }

    impl Float {
        pub fn value(&self) -> f64 {
            self.value
        }

        pub fn span(&self) -> Span {
            self.span
        }

        // pub fn to_negative(&self) -> Float {
        //     Float {
        //         value: -self.value,
        //         span: self.span,
        //     }
        // }
    }

    impl ParseChars for Float {
        fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
            // .42	Leading decimal
            // 42.	Trailing decimal
            // 42.42	Standard float
            // -42.42	Negative float
            // 42e10	Scientific notation
            // -3.14e-2	Scientific with negative
            // +3.14	Positive sign
            // 3.14E+2	Uppercase exponent
            //
            let checkpoint = cursor.checkpoint();
            // let sign = cursor.parse::<Char<'-'>>().ok();
            type FloatType = OneOf2<ScientificNotation, TrailingDecimal>;

            let float = cursor.parse_as_str::<FloatType>().map(|slice| {
                let slice = slice.replace("_", "");
                let value = slice.parse::<f64>().expect(
                    "Invalid float. This should never happen and is a \
                        bug in the compiler itself. Please, report on github at \
                        github.com/oyelowo/yedb",
                );
                // let signed_value = match sign {
                //     Some(_) => -value,
                //     None => value,
                // };
                Self {
                    // Unwrap is safe because the float is guaranteed to be a valid float
                    // value: signed_value,
                    value,
                    span: cursor.span_since(checkpoint),
                }
            });
            // Alternative imperative approach
            // let float = cursor
            //     // Case 1: Leading decimal (e.g., .69, .123E4)
            //     .parse_as_str::<LeadingDecimal>()
            //     // Case 2: Scientific notation (e.g., 59e37, 94.17e-2)
            //     .or_else(|_| cursor.parse_as_str::<ScientificNotation>())
            //     // Case 3: Trailing decimal (e.g., 54., 75.19)
            //     .or_else(|_| cursor.parse_as_str::<TrailingDecimal>())
            //     .map(|span| {
            //         let value = span.parse::<f64>().expect(
            //             "Invalid float. This should never happen and is a \
            //                 bug in the compiler itself. Please, report on github at \
            //                 github.com/oyelowo/yedb",
            //         );
            //         let signed_value = match sign {
            //             Some(_) => -value,
            //             None => value,
            //         };
            //         Self {
            //             // Unwrap is safe because the float is guaranteed to be a valid float
            //             value: signed_value,
            //             span: cursor.span_since(checkpoint),
            //         }
            //     });

            float
        }
    }

    #[cfg(test)]
    mod tests {
        // #![allow(clippy::approx_constant)]
        //
        // use super::*;
        // use rstest::rstest;
        //
        // #[rstest]
        // #[case(".42", 0.42)]
        // #[case(".4_2", 0.42)]
        // #[case(".42_", 0.42)]
        // #[case("-.42", -0.42)]
        // #[case("42.", 42.0)]
        // #[case("4_2.", 42.0)]
        // #[case("42.42", 42.42)]
        // #[case("4_2.4_2", 42.42)]
        // #[case("4_2_.4_2_", 42.42)]
        // #[case(".42", 0.42)]
        // #[case("-.42", -0.42)]
        // #[case("42.", 42.0)]
        // #[case("42.42", 42.42)]
        // #[case("-42.42", -42.42)]
        // #[case("42e10", 42e10)]
        // #[case("-3.14e-2", -3.14e-2)]
        // #[case("-3.14E-2", -3.14e-2)]
        // #[case("-3.14", -3.14)]
        // #[case("3.14e+2", 3.14e2)]
        // #[case("3.14E+2", 3.14e2)]
        // #[case("3.14e2", 3.14e2)]
        // #[case("3.14E2", 3.14e2)]
        // #[case("59e37", 59e37)]
        // #[case("59E37", 59e37)]
        // #[case("94.17e-2", 94.17e-2)]
        // #[case("94.17E-2", 94.17e-2)]
        // #[case("54.", 54.0)]
        // #[case("75.19", 75.19)]
        // #[case(".69", 0.69)]
        // #[case(".123E4", 0.123e4)]
        // #[case(".123e4", 0.123e4)]
        // fn test_float_unsigned(#[case] input: &str, #[case] expected: f64) {
        //     let mut cursor = CharCursor::new(input);
        //     let float = cursor.parse::<Float>().unwrap();
        //     assert_eq!(float.value(), expected);
        //
        //     cursor.reset_dangerous();
        //     let float = cursor.parse_exact::<Float>().unwrap();
        //     assert_eq!(float.value(), expected);
        // }
        //
        // #[rstest]
        // #[case("-42.42 ", -42.42, (0, 6), 1)]
        // #[case("-42.42 34", -42.42, (0, 6), 3)]
        // #[case("42.42.42", 42.42, (0, 5), 3)]
        // #[case("42.42-5.42", 42.42, (0, 5), 5)]
        // #[case("42.42 -5.42", 42.42, (0, 5), 6)]
        // fn test_float_signed_partial(
        //     #[case] input: &str,
        //     #[case] expected: f64,
        //     #[case] sign_span: (usize, usize),
        //     #[case] span_len: usize,
        // ) {
        //     let mut cursor = CharCursor::new(input);
        //     let float = cursor.parse::<Float>().unwrap();
        //     assert_eq!(float.value(), expected);
        //     assert_eq!(float.span.start().absolute, sign_span.0);
        //     assert_eq!(cursor.remaining().len(), span_len);
        //
        //     cursor.reset_dangerous();
        //     let float = cursor.parse_exact::<Float>();
        //     assert!(float.is_err());
        // }
        //
        // #[rstest]
        // #[case("_0")]
        // #[case("+42.42")]
        // #[case("-42.42 34")]
        // #[case("_0_0_")]
        // #[case("3.14E+2 34")]
        // #[case("42.42.42")]
        // #[case("42.42-5.42")]
        // #[case("42.42 -5.42")]
        // fn test_float_unsigned_exact_fail(#[case] input: &str) {
        //     let mut cursor = CharCursor::new(input);
        //     let parsed = cursor.parse_exact::<Float>();
        //     assert!(parsed.is_err());
        // }
        //
        // #[rstest]
        // #[case("_0")]
        // #[case("_0_0_")]
        // fn test_float_unsigned_partial_fail(#[case] input: &str) {
        //     let mut cursor = CharCursor::new(input);
        //     let parsed = cursor.parse::<Float>();
        //     assert!(parsed.is_err());
        // }
    }
}
