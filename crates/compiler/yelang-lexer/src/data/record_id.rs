use crate::{
    Char, CharCursor, CharLexerResult, Either, ParseChars, Span,
    data::{IdentLexed, UIntLexed},
};
use std::fmt::{self, Display};

// A record ID is in the format `table:id`
// The `id` part can be an identifier or an unsigned integer.
type RecordIdFormat = (IdentLexed, Char<':'>, Either<IdentLexed, UIntLexed>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordIdLexed {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordIdParts<'a> {
    pub table: &'a str,
    pub key: &'a str,
    pub key_is_uint: bool,
}

// Removed interning-specific types/helpers

/// Parse a record id string into its (table, key) parts.
///
/// This is the canonical parser: it uses the same lexer machinery as the tokenizer
/// (`IdentLexed`, `UIntLexed`) rather than re-implementing identifier/number rules.
pub fn parse_record_id_parts(s: &str) -> Option<RecordIdParts<'_>> {
    let mut cursor = CharCursor::new(s);
    let parsed = cursor.parse_exact::<RecordIdFormat>().ok()?;

    // Ensure full consumption.
    if !cursor.is_eof() {
        return None;
    }

    let (table, _, key) = parsed;
    let table = cursor.str_from_span(table.span());

    let (key, key_is_uint) = match key {
        Either::Left(ident) => (cursor.str_from_span(ident.span()), false),
        Either::Right(uint) => (cursor.str_from_span(uint.span()), true),
    };

    Some(RecordIdParts {
        table,
        key,
        key_is_uint,
    })
}

impl Display for RecordIdLexed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "record_id")
    }
}

impl ParseChars for RecordIdLexed {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let (_, span) = cursor.parse_with_span::<RecordIdFormat>()?;
        Ok(RecordIdLexed { span })
    }
}
