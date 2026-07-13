/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 28/01/2025
 */
use crate::{
    Char, CharCursor, CharLexerError, CharLexerResult, Either, OneOf6, OneOf10, ParseChars, Repeat,
    Span,
    word::{Word2, Word3, Word4, Word5},
};
use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntSuffix {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

impl Display for IntSuffix {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            IntSuffix::I8 => write!(f, "i8"),
            IntSuffix::I16 => write!(f, "i16"),
            IntSuffix::I32 => write!(f, "i32"),
            IntSuffix::I64 => write!(f, "i64"),
            IntSuffix::I128 => write!(f, "i128"),
            IntSuffix::Isize => write!(f, "isize"),
            IntSuffix::U8 => write!(f, "u8"),
            IntSuffix::U16 => write!(f, "u16"),
            IntSuffix::U32 => write!(f, "u32"),
            IntSuffix::U64 => write!(f, "u64"),
            IntSuffix::U128 => write!(f, "u128"),
            IntSuffix::Usize => write!(f, "usize"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radix {
    Binary,
    Octal,
    Decimal,
    Hexadecimal,
}

pub type Epsilon = Either<Char<'e'>, Char<'E'>>;
pub type TDigit = OneOf10<
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

// struct IntUnsigned(IntUnsigned);
// impl MM {}
#[derive(Debug, Clone, Copy)]
pub struct UIntLexed {
    span: Span,
    suffix: Option<Span>,
}

impl UIntLexed {
    pub fn span(&self) -> Span {
        self.span
    }
}

pub type Sign = Either<Char<'-'>, Char<'+'>>;
#[derive(Debug, Clone, Copy)]
pub struct IntLexed {
    span: Span,
    suffix: Option<IntSuffix>,
    radix: Radix,
}

impl IntLexed {
    pub fn span(&self) -> Span {
        self.span
    }

    pub fn suffix(&self) -> Option<IntSuffix> {
        self.suffix
    }

    pub fn radix(&self) -> Radix {
        self.radix
    }
}

impl ParseChars for UIntLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        // let (_nums, span) =
        //     cursor.parse_with_span::<SeparatedList<NumberNoSeparation, T!["_"]>>()?;
        // let value = span.as_slice(cursor).replace("_", "").parse().unwrap();
        // if span.is_empty() {
        //     return Err(LexerError::ExpectedNumber { span });
        // }
        pub type IntUnsignedx = (TDigit, Option<Repeat<Either<TDigit, Char<'_'>>>>);
        let (_res, span) = cursor.parse_with_span_as_str::<IntUnsignedx>()?;
        if span.is_empty() {
            return Err(CharLexerError::ExpectedNumber { span });
        }

        let (suff_str, suff_span) = cursor.parse_with_span::<Option<IntSuffix>>()?;
        // cursor.verify_exact(1, |c| c != '.')?;

        Ok(UIntLexed {
            span,
            suffix: suff_str.and(Some(suff_span)),
        })
    }
}

type UIntSuff = OneOf6<
    Word2<'i', '8'>,
    Word3<'i', '1', '6'>,
    Word3<'i', '3', '2'>,
    Word3<'i', '6', '4'>,
    Word4<'i', '1', '2', '8'>,
    Word5<'i', 's', 'i', 'z', 'e'>,
>;

type IIntSuff = OneOf6<
    Word2<'u', '8'>,
    Word3<'u', '1', '6'>,
    Word3<'u', '3', '2'>,
    Word3<'u', '6', '4'>,
    Word4<'u', '1', '2', '8'>,
    Word5<'u', 's', 'i', 'z', 'e'>,
>;

impl ParseChars for IntSuffix {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let word = cursor.parse::<Either<IIntSuff, UIntSuff>>()?;
        match word {
            Either::Left(unsigned) => match unsigned {
                OneOf6::_1(_) => Ok(IntSuffix::U8),
                OneOf6::_2(_) => Ok(IntSuffix::U16),
                OneOf6::_3(_) => Ok(IntSuffix::U32),
                OneOf6::_4(_) => Ok(IntSuffix::U64),
                OneOf6::_5(_) => Ok(IntSuffix::U128),
                OneOf6::_6(_) => Ok(IntSuffix::Usize),
            },
            Either::Right(signed) => match signed {
                OneOf6::_1(_) => Ok(IntSuffix::I8),
                OneOf6::_2(_) => Ok(IntSuffix::I16),
                OneOf6::_3(_) => Ok(IntSuffix::I32),
                OneOf6::_4(_) => Ok(IntSuffix::I64),
                OneOf6::_5(_) => Ok(IntSuffix::I128),
                OneOf6::_6(_) => Ok(IntSuffix::Isize),
            },
        }
    }
}

impl ParseChars for IntLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let start = cursor.checkpoint();

        // An integer cannot start with an underscore. This check ensures it's not an identifier.
        if cursor.peek() == Some('_') {
            return Err(CharLexerError::InvalidIdent {
                found: "Integer cannot start with an underscore".to_string(),
                span: cursor.current_span(),
            });
        }

        let _sign = cursor.parse::<Option<Char<'-'>>>()?;

        // Determine the radix and the valid digit predicate
        let (radix, is_valid_digit) = if cursor.consume("0x").is_ok() {
            (Radix::Hexadecimal, is_hex_digit as fn(char) -> bool)
        } else if cursor.consume("0b").is_ok() {
            (Radix::Binary, is_binary_digit as fn(char) -> bool)
        } else if cursor.consume("0o").is_ok() {
            (Radix::Octal, is_octal_digit as fn(char) -> bool)
        } else {
            (Radix::Decimal, is_decimal_digit as fn(char) -> bool)
        };

        // For all radices, first character must be a valid digit
        cursor.verify_exact(1, is_valid_digit)?;

        // Now consume the rest of the digits and underscores.
        let num_body = cursor.consume_while(|c| is_valid_digit(c) || c == '_');

        if num_body.ends_with('_') {
            return Err(CharLexerError::InvalidIdent {
                found: "Trailing underscore in number".to_string(),
                span: cursor.span_since(start),
            });
        }

        let suffix = cursor.parse::<Option<IntSuffix>>()?;

        Ok(IntLexed {
            span: cursor.span_since(start),
            suffix,
            radix,
        })
    }
}
fn is_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

fn is_binary_digit(c: char) -> bool {
    c == '0' || c == '1'
}

fn is_octal_digit(c: char) -> bool {
    ('0'..='7').contains(&c)
}

fn is_decimal_digit(c: char) -> bool {
    c.is_ascii_digit()
}

mod int_parsed_value {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub struct UInt {
        value: u64,
    }

    impl Display for UInt {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.value)
        }
    }

    impl Eq for UInt {}

    impl PartialEq for UInt {
        fn eq(&self, other: &Self) -> bool {
            self.value == other.value
        }
    }

    impl UInt {
        pub fn value(&self) -> u64 {
            self.value
        }

        pub fn to_negative(&self) -> Int {
            let mut signed = self.to_signed();
            signed.0 = -signed.0;
            signed
        }

        pub fn to_signed(&self) -> Int {
            Int(self.value as i64)
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub struct Int(i64);

    impl Int {
        pub fn new(value: i64) -> Self {
            Self(value)
        }

        pub fn value(&self) -> i64 {
            self.0
        }
    }

    impl Display for Int {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl Eq for Int {}

    impl PartialEq for Int {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }

    // #[derive(Debug, Clone, Copy)]
    // pub struct NumberNoSeparation;
    //
    // impl<'a> ParseChars<'a> for NumberNoSeparation {
    //     fn parse(cursor: &mut Cursor<'a>) -> CharLexerResult<Self> {
    //         let span = cursor.consume_while_m_span(1, |c| c.is_ascii_digit())?;
    //         if span.is_empty() {
    //             return Err(LexerError::ExpectedNumber { span });
    //         }
    //         Ok(NumberNoSeparation)
    //     }
    // }

    impl ParseChars for UInt {
        fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
            // let (_nums, span) =
            //     cursor.parse_with_span::<SeparatedList<NumberNoSeparation, T!["_"]>>()?;
            // let value = span.as_slice(cursor).replace("_", "").parse().unwrap();
            // if span.is_empty() {
            //     return Err(LexerError::ExpectedNumber { span });
            // }
            pub type IntUnsignedx = (TDigit, Option<Repeat<Either<TDigit, Char<'_'>>>>);
            let (res, span) = cursor.parse_with_span_as_str::<IntUnsignedx>()?;
            // cursor.verify_exact(1, |c| c != '.')?;

            let value = res.replace("_", "").parse().unwrap();
            if span.is_empty() {
                return Err(CharLexerError::ExpectedNumber { span });
            }

            Ok(UInt { value })
        }
    }

    impl ParseChars for Int {
        fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
            let (sign, int) = cursor.parse::<(Option<Char<'-'>>, UInt)>()?;
            let sign = sign.map(|_| -1).unwrap_or(1);
            Ok(Int(int.value as i64 * sign))
        }
    }

    #[cfg(test)]
    mod tests {
        #![allow(clippy::inconsistent_digit_grouping)]
        #![allow(clippy::zero_prefixed_literal)]

        use super::*;
        use rstest::rstest;

        #[rstest]
        #[case("0", 0)]
        #[case("01", 0001)]
        #[case("1", 1)]
        #[case("10", 10)]
        #[case("100", 100)]
        #[case("100_34_45", 1_003_445)]
        #[case("1003445", 1_003_445)]
        #[case("100_34_45", 1003445)]
        #[case("100_34_45", 10034_45)]
        #[case("1003445", 100344_5)]
        #[case("10034_45", 10034_45)]
        fn test_int_unsigned(#[case] input: &str, #[case] expected: u64) {
            let mut cursor = CharCursor::new(input);
            let int = cursor.parse::<UInt>().unwrap();
            assert_eq!(int.value(), expected);

            cursor.reset_dangerous();
            let int = cursor.parse_exact::<UInt>().unwrap();
            assert_eq!(int.value(), expected);
        }

        #[rstest]
        #[case("_0")]
        #[case("_0_0_")]
        #[case("rtr")]
        #[case("x0")]
        fn test_int_unsigned_fail(#[case] input: &str) {
            let mut cursor = CharCursor::new(input);
            let int = cursor.parse::<UInt>();
            assert!(int.is_err());
        }

        #[rstest]
        #[case("0", 0)]
        #[case("-2", -2)]
        #[case("2", 2)]
        #[case("100", 100)]
        #[case("-100", -100)]
        #[case("100_34_45", 100_34_45)]
        #[case("-1003445", -100_34_45)]
        fn test_int_signed(#[case] input: &str, #[case] expected: i64) {
            let mut cursor = CharCursor::new(input);
            let int = cursor.parse::<Int>().unwrap();
            assert_eq!(int.0, expected);
        }

        #[rstest]
        #[case("_0")]
        #[case("_0_0_")]
        #[case("rtr")]
        #[case("x0")]
        fn test_int_signed_fail(#[case] input: &str) {
            let mut cursor = CharCursor::new(input);
            let int = cursor.parse::<Int>();
            assert!(int.is_err());
        }
    }
}
