use crate::{
    Char, CharCursor, CharLexerError, CharLexerResult, Either, OneOf10, ParseChars, Repeat,
    RepeatExact, Span,
};
use std::fmt::{self, Display};

// Helper for parsing digits
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

// YYYY
type Year = RepeatExact<4, Digit>;
// MM
type Month = RepeatExact<2, Digit>;
// DD
type Day = RepeatExact<2, Digit>;
// HH
type Hour = RepeatExact<2, Digit>;
// MM
type Minute = RepeatExact<2, Digit>;
// SS
type Second = RepeatExact<2, Digit>;
// .ffffff...
type FractionalSecond = (Char<'.'>, Repeat<Digit>);

// YYYY-MM-DD
type DatePart = (Year, Char<'-'>, Month, Char<'-'>, Day);

// HH:MM:SS[.ffffff]
type TimePart = (
    Hour,
    Char<':'>,
    Minute,
    Char<':'>,
    Second,
    Option<FractionalSecond>,
);

// Z or +HH:MM or -HH:MM
type TimezonePart = Either<Char<'Z'>, (Either<Char<'+'>, Char<'-'>>, Hour, Char<':'>, Minute)>;

// Full datetime: YYYY-MM-DDTHH:MM:SS.ffffffZ
type FullDateTime = (
    DatePart,
    Either<Char<'T'>, Char<' '>>,
    TimePart,
    Option<TimezonePart>,
);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatetimeLexed {
    pub span: Span,
}

impl Display for DatetimeLexed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "datetime")
    }
}

impl ParseChars for DatetimeLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        // To avoid ambiguity with integers or floats, we parse the full, structured format.
        let (_, span) = cursor.parse_with_span::<FullDateTime>()?;
        Ok(DatetimeLexed { span })
    }
}
