use crate::{
    Char, CharCursor, CharLexerError, CharLexerResult, Either, OneOf2, OneOf3, OneOf6, OneOf10,
    ParseChars, RepeatExact, RepeatMinMax, Span, TDigit,
};
use std::fmt::{self, Display};

// let uuid = uuid'3a843a-8642-4227-ab55-83c70ce8a6d6';

// A hex digit (0-9, a-f, A-F)
type HexDigit = Either<
    TDigit,
    Either<
        OneOf6<Char<'a'>, Char<'b'>, Char<'c'>, Char<'d'>, Char<'e'>, Char<'f'>>,
        OneOf6<Char<'A'>, Char<'B'>, Char<'C'>, Char<'D'>, Char<'E'>, Char<'F'>>,
    >,
>;

// Standard UUID format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
type UuidFormat = (
    RepeatExact<8, HexDigit>,
    Char<'-'>,
    RepeatExact<4, HexDigit>,
    Char<'-'>,
    RepeatExact<4, HexDigit>,
    Char<'-'>,
    RepeatExact<4, HexDigit>,
    Char<'-'>,
    RepeatExact<12, HexDigit>,
);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UuidLexed {
    pub span: Span,
}

impl Display for UuidLexed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "uuid")
    }
}

impl ParseChars for UuidLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let (_, span) = cursor.parse_with_span::<UuidFormat>()?;
        Ok(UuidLexed { span })
    }
}
