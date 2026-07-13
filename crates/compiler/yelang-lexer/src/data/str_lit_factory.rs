/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 27/01/2025
 */
use crate::{
    Char, CharCursor, OneOf3, Repeat, Span, Verify, errors::CharLexerError, traits::ParseChars,
};
use std::marker::PhantomData;

#[derive(Debug, Clone, Default, Copy, PartialEq)]
pub struct ModifiersTag {
    raw: bool,          // r
    interpolated: bool, // i
    span: Span,
}

impl ParseChars for ModifiersTag {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let checkpoint = cursor.checkpoint();
        let mut raw = false;
        let mut interpolated = false;
        let modifier = cursor.consume_while_m_n(1, 2, |c| c == 'r' || c == 'i')?;
        let mut chars = modifier.chars();
        // Safe due to consume_while_m_n's min=1
        let first = chars.next().unwrap();
        let second = chars.next();
        if let Some(second) = second {
            if first == second {
                return Err(CharLexerError::DuplicateModifier {
                    modifier: first,
                    span: cursor.current_span(),
                });
            }
        }

        for c in modifier.chars() {
            match c {
                'r' => raw = true,
                'i' => interpolated = true,
                _ => {
                    return Err(CharLexerError::InvalidModifiers {
                        span: cursor.current_span(),
                    });
                }
            }
        }
        Ok(ModifiersTag {
            raw,
            interpolated,
            span: cursor.span_since(checkpoint),
        })
    }
}

#[derive(Debug, Clone, Default, Copy, PartialEq)]
pub struct StringTag {
    pub value: Span,
}

impl StringTag {}

impl ParseChars for StringTag {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        // Assume tag is like "geo" or "dt"
        let span = cursor.consume_while_span(|c| c.is_alphabetic());
        if span.is_empty() {
            return Err(CharLexerError::InvalidStringTag {
                span: cursor.current_span(),
            });
        }
        Ok(StringTag { value: span })
    }
}

#[derive(Debug, Clone, Default, Copy, PartialEq)]
struct DelimiterParser {
    level: u8,
}

impl ParseChars for DelimiterParser {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        // let level = cursor.consume_while(|c| c == '#').len() as u8;
        // Ok(Self { level})
        let repeated = cursor.parse::<Repeat<Char<'#'>>>()?;
        Ok(Self {
            level: repeated.count() as u8,
        })
    }
}

type _3Quotes = OneOf3<Char<'\''>, Char<'"'>, Char<'`'>>;

// - `[tag][-modifiers][delimiter]<quote>content<quote>[delimiter]`.
// e.g dt"2025-05-25", geo-r##'POINT(1 2)'##, r"raw string", i'Interpolated {name} string'
#[derive(Debug, Clone, Copy, PartialEq)]
// pub struct StringLitLexed<TDataTag: ParseChars = StringTag, TQuote: ParseChars = _3Quotes> {
pub struct StringMakerLexed<TDataTag: ParseChars, TQuote: ParseChars, TContent: ParseChars> {
    /// "geo", "dt", etc
    pub all_tags_span: Option<Span>,
    pub tag_data_marker: PhantomData<TDataTag>,
    pub tags_modifiers: Option<ModifiersTag>,
    pub quote: Span,
    pub quote_marker: PhantomData<TQuote>,
    /// Number of #s
    pub delimiter_level: u8,
    pub raw_content: Span,
    pub content_marker: PhantomData<TContent>,
    pub span: Span,
}

impl<TDataTag, TQuote, TContent> StringMakerLexed<TDataTag, TQuote, TContent>
where
    TDataTag: ParseChars,
    TQuote: ParseChars,
    TContent: ParseChars,
{
    pub fn opener(&self) -> Option<Span> {
        self.all_tags_span
    }

    pub fn quote(&self) -> &Span {
        &self.quote
    }

    pub fn raw_content(&self) -> &Span {
        &self.raw_content
    }

    pub fn modifiers(&self) -> Option<ModifiersTag> {
        self.tags_modifiers
    }

    pub fn delimiter_level(&self) -> u8 {
        self.delimiter_level
    }
}

impl<TDataTag, TQuote, TContent> ParseChars for StringMakerLexed<TDataTag, TQuote, TContent>
where
    TDataTag: ParseChars,
    TQuote: ParseChars,
    TContent: ParseChars + std::fmt::Debug,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let start = cursor.checkpoint();
        //         // 1. Try to parse a tag.
        // let tag: Option<TTag> = cursor.optional::<TTag>()?;

        // // 2. Parse optional modifiers. A hyphen is required only if a tag is present.
        // let modifiers: Option<Modifiers> = if tag.is_some() {
        //     if cursor.consume("-").is_ok() {
        //         cursor.parse::<Option<Modifiers>>()?
        //     } else {
        //         None
        //     }
        // } else {
        //     cursor.parse::<Option<Modifiers>>()?
        // };

        type AllTags<T> = (T, Char<'-'>, ModifiersTag);
        let (all_tags, all_tags_span) = cursor
            .parse_with_span::<Option<OneOf3<AllTags<TDataTag>, ModifiersTag, TDataTag>>>()?;
        let (ty_data_tag, ty_modifier) = match all_tags {
            Some(OneOf3::_1((tag_data, _, tag_modifier))) => {
                // Tags cannot have interpolated modifier i.e templated strings
                if tag_modifier.interpolated {
                    return Err(CharLexerError::InvalidModifiers {
                        span: cursor.current_span(),
                    });
                }
                (Some(tag_data), Some(tag_modifier))
            }
            Some(OneOf3::_2(tag_modifier)) => (None, Some(tag_modifier)),
            Some(OneOf3::_3(tag_data)) => (Some(tag_data), None),
            None => (None, None),
        };

        let delimeter = cursor.parse::<DelimiterParser>().unwrap_or_default();

        // Capture the opening quote character
        let (quote_char, quote_span) = cursor.parse_with_span_as_str::<TQuote>()?;
        // let open_quote = cursor.parse::<OneOf3<Char<'\''>, Char<'"'>, Char<'`'>>>()?;
        // let quote_char = match open_quote {
        //     OneOf3::_1(_) => '\'',
        //     OneOf3::_2(_) => '"',
        //     OneOf3::_3(_) => '`',
        // };
        // if ty_data_tag.is_some() {
        //     // let x = cursor.parse_with_span_as_str::<Verify<TContent>>();
        //     let x = cursor.parse_exact::<Verify<TContent>>();
        //     panic!("mama: {x:#?}");
        // }

        let closing_delimiter = format!("{}{}", quote_char, "#".repeat(delimeter.level as usize));
        let (content, content_span) = cursor.until_b4_str(&closing_delimiter)?;
        if ty_data_tag.is_some() {
            let mut curs = CharCursor::new(content);
            curs.parse::<Verify<TContent>>()?;
            // curs.parse_exact::<TContent>()?;
        }

        cursor.consume(&closing_delimiter)?;
        if cursor.verify("#").is_ok() {
            return Err(CharLexerError::UnMatchedStringDelimeter {
                span: cursor.current_span(),
                opening: delimeter.level as usize,
            });
        }

        Ok(StringMakerLexed {
            all_tags_span: ty_data_tag.map(|_| all_tags_span),
            tag_data_marker: PhantomData,
            tags_modifiers: ty_modifier,
            quote: quote_span,
            delimiter_level: delimeter.level,
            raw_content: content_span,
            content_marker: PhantomData,
            span: cursor.span_since(start),
            quote_marker: PhantomData,
        })
    }
}

#[cfg(test)]
mod string_lit_test {
    use super::*;
    use crate::word::{Word2, Word3, Word4};
    use crate::{Any, OneOf4};
    use rstest::rstest;
    type DataTag = OneOf3<Word2<'d', 't'>, Word3<'g', 'e', 'o'>, Word3<'t', 'a', 'g'>>;
    type QuoteChar = OneOf4<Char<'\''>, Char<'"'>, Char<'`'>, Char<'"'>>; // Added double quote as separate type

    type StrTest = StringMakerLexed<DataTag, QuoteChar, Any>;
    #[rstest]
    #[case::simple("geo'content'", "geo", "", 0, "content", '\'')]
    // interpolation not allowed anymore in data tag strings
    // #[case::with_modifiers("geo-i'content'", "geo", "i", 0, "content", '\'')]
    #[case::with_raw("geo-r'content'", "geo-r", "r", 0, "content", '\'')]
    #[case::with_raw("r'content'", "", "r", 0, "content", '\'')]
    #[case::with_raw("i'content'", "", "i", 0, "content", '\'')]
    #[case::with_raw("dt'content'", "dt", "", 0, "content", '\'')]
    #[case::with_raw("dt#'cont'ent'#", "dt", "", 1, "cont'ent", '\'')]
    #[case::with_raw("dt#'cont''ent'#", "dt", "", 1, "cont''ent", '\'')]
    #[case::with_raw("dt#'cont'''ent'#", "dt", "", 1, "cont'''ent", '\'')]
    #[case::with_raw("dt##'cont'''ent'##", "dt", "", 2, "cont'''ent", '\'')]
    #[case::with_raw("dt##'cont''#'ent'##", "dt", "", 2, "cont''#'ent", '\'')]
    #[case::with_raw("dt###'cont''##'ent'###", "dt", "", 3, "cont''##'ent", '\'')]
    #[case::with_modifiers_and_delimiter("dt-r###'content'###", "dt-r", "r", 3, "content", '\'')]
    #[case::with_delimiter("geo##'content'##", "geo", "", 2, "content", '\'')]
    #[case::with_modifiers_and_delimiter("geo-r##'content'##", "geo-r", "r", 2, "content", '\'')]
    // interpolation not allowed anymore in data tag strings
    // #[case::with_modifiers_and_delimiter("geo-ri##'content'##", "geo", "ri", 2, "content", '\'')]
    // #[case::with_modifiers_and_delimiter("geo-ir###'content'###", "geo", "ir", 3, "content", '\'')]
    // #[case::with_modifiers_and_delimiter("dt-ir####'content'####", "dt", "ir", 4, "content", '\'')]
    #[case::double_quote("geo\"content\"", "geo", "", 0, "content", '"')]
    #[case::backtick("geo`content`", "geo", "", 0, "content", '`')]
    #[case::double_quote_modifiers("dt-r\"content\"", "dt-r", "r", 0, "content", '"')]
    #[case::backtick_delimiter("tag##`content`##", "tag", "", 2, "content", '`')]
    fn test_string_lit(
        #[case] input: &str,
        #[case] tag: &str,
        #[case] modifiers: &str,
        #[case] delimiter: u8,
        #[case] content: &str,
        #[case] quote_char: char,
    ) {
        let mut cursor = CharCursor::new(input);

        let string_lit = cursor
            .parse_exact::<StringMakerLexed<DataTag, QuoteChar, Any>>()
            .map_err(|e| e.to_string())
            .unwrap();
        let raw_content = string_lit.raw_content.as_slice(&cursor);
        let tag_value = string_lit
            .all_tags_span
            .unwrap_or_default()
            .as_slice(&cursor);
        let quote_value = string_lit.quote.as_slice(&cursor).chars().next().unwrap();
        assert_eq!(tag_value, tag);
        assert_eq!(raw_content, content);
        assert_eq!(string_lit.delimiter_level, delimiter);
        assert_eq!(quote_value, quote_char);
        assert_eq!(
            string_lit.tags_modifiers.unwrap_or_default().interpolated,
            modifiers.contains('i')
        );
        assert_eq!(
            string_lit.tags_modifiers.unwrap_or_default().raw,
            modifiers.contains('r')
        );

        cursor.reset_dangerous();
        let string_lit = cursor
            .parse::<StrTest>()
            .map_err(|e| e.to_string())
            .unwrap();
        let raw_content = string_lit.raw_content.as_slice(&cursor);
        let tag_value = string_lit
            .all_tags_span
            .unwrap_or_default()
            .as_slice(&cursor);
        let quote_value = string_lit.quote.as_slice(&cursor).chars().next().unwrap();

        assert_eq!(tag_value, tag);
        assert_eq!(raw_content, content);
        assert_eq!(string_lit.delimiter_level, delimiter);
        assert_eq!(quote_value, quote_char);
        assert_eq!(
            string_lit.tags_modifiers.unwrap_or_default().interpolated,
            modifiers.contains('i')
        );
        assert_eq!(
            string_lit.tags_modifiers.unwrap_or_default().raw,
            modifiers.contains('r')
        );
    }

    #[rstest]
    #[case::with_raw("dt'cont'ent'", "dt", "", 0, "cont")]
    #[case::with_raw("dt'cont#ent'", "dt", "", 0, "cont#ent")]
    #[case::with_raw("dt'cont##ent'", "dt", "", 0, "cont##ent")]
    #[case::with_raw("dt'cont''##'ent'", "dt", "", 0, "cont")]
    #[case::with_raw("dt##'cont''##'ent'##", "dt", "", 2, "cont'")]
    #[case::with_raw("dt###'cont''##'ent'###", "dt", "", 3, "cont''##'ent")]
    fn test_string_lit_partial(
        #[case] input: &str,
        #[case] tag: &str,
        #[case] modifiers: &str,
        #[case] delimiter: u8,
        #[case] content: &str,
    ) {
        let mut cursor = CharCursor::new(input);
        let string_lit = cursor
            .parse::<StrTest>()
            .map_err(|e| e.to_string())
            .unwrap();
        let raw_content = string_lit.raw_content.as_slice(&cursor);
        let tag_value = string_lit
            .all_tags_span
            .unwrap_or_default()
            .as_slice(&cursor);
        assert_eq!(tag_value, tag);

        assert_eq!(raw_content, content);
        assert_eq!(string_lit.delimiter_level, delimiter);
        assert_eq!(
            string_lit.tags_modifiers.unwrap_or_default().interpolated,
            modifiers.contains('i')
        );
        assert_eq!(
            string_lit.tags_modifiers.unwrap_or_default().raw,
            modifiers.contains('r')
        );
    }

    #[rstest]
    #[case::invalid_tag("1'content'")]
    #[case::invalid_tag("i#'content'##")]
    #[case::invalid_tag("1a'content'")]
    #[case::invalid_tag("1a-ir'content'")]
    #[case::invalid_tag("geo-irx'content'")]
    #[case::invalid_tag("geo-'content'")]
    #[case::invalid_tag("geo-a'content'")]
    #[case::invalid_tag("geo-a#'content'#")]
    #[case::invalid_tag("-a#'content'#")]
    #[case::invalid_tag("-i#'content'#")]
    #[case::invalid_tag("-r#'content'#")]
    #[case::invalid_tag("-ir#'content'#")]
    #[case::invalid_tag("-ri#'content'#")]
    #[case::invalid_tag("-a'content'")]
    #[case::invalid_tag("-i'content'")]
    #[case::invalid_tag("-r'content'")]
    #[case::invalid_tag("-ir'content'")]
    #[case::invalid_tag("-ri'content'")]
    #[case::invalid_tag("geo*ir#'content'#")]
    fn test_string_lit_invalid_tag(#[case] input: &str) {
        let mut cursor = CharCursor::new(input);
        let string_lit = cursor.parse::<StrTest>();
        assert!(string_lit.is_err());
    }
}
