use crate::{
    Char, CharCursor, CharLexerError, CharLexerResult, Either, OneOf7, ParseChars, RepeatMin, Span,
    UIntLexed,
    word::{Word1, Word2},
};
use std::fmt::{self, Display};

// e.g., 1w, 2d, 3h, 4m, 5s, 6ms, 7ns
type DurationUnit = OneOf7<
    Word1<'w'>,
    Word1<'d'>,
    Word1<'h'>,
    Word1<'m'>,
    Word1<'s'>,
    Word2<'m', 's'>,
    Word2<'n', 's'>,
>;

type DurationComponent = (UIntLexed, DurationUnit);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DurationLexed {
    pub span: Span,
}

impl Display for DurationLexed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "duration")
    }
}

impl ParseChars for DurationLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        // A duration is one or more components, e.g., 1w2d3h
        cursor.parse::<RepeatMin<1, DurationComponent>>()?;
        Ok(DurationLexed {
            span: cursor.span_since(checkpoint),
        })
    }
}
