/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 27/01/2025
 */
use crate::chars::whitespace::Whitespace;
use crate::data::comments::Comment;
use crate::helper_types::either::Either;
use crate::{
    Char, CharCursor, OneOf3, OneOf4, Repeat, Span, Verify, errors::CharLexerError,
    traits::ParseChars,
};
use std::fmt::Display;

#[derive(Debug, Clone, PartialEq)]
pub enum InterpolationPart {
    Literal(Span),
    Expression(Span),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolatedStringPartKind {
    Literal,
    Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpolatedStringPartOffsets {
    pub kind: InterpolatedStringPartKind,
    pub start: usize,
    pub end: usize,
}

/// Scans an interpolated string *token text* (e.g. `i"Hello ${name}!"`) and returns
/// best-effort ranges for literal and expression parts.
///
/// Offsets are byte offsets into the provided `token_text`.
///
/// Notes:
/// - Ranges are based on the lexer's string-literal parser (`StringLitLexed`) so they
///   exclude the tag/modifier and outer quotes by default.
/// - Expression part ranges include the interpolation markers (`{...}` or `${...}`)
///   and the closing `}`.
/// - On malformed input, this returns a single literal part spanning the string content.
pub fn scan_interpolated_string_parts_in_token_text(
    token_text: &str,
) -> Option<Vec<InterpolatedStringPartOffsets>> {
    let mut token_cursor = CharCursor::new(token_text);
    let string_lit = token_cursor.parse_exact::<StringLitLexed>().ok()?;

    let mods = string_lit.modifiers.unwrap_or_default();
    if !mods.interpolated {
        return None;
    }
    let is_raw = mods.raw;

    let content_span = string_lit.raw_content;
    let content = content_span.as_slice(&token_cursor);
    let content_start = content_span.start().absolute as usize;

    let mut cursor = CharCursor::new(content);
    let mut parts: Vec<InterpolatedStringPartOffsets> = Vec::new();

    let mut lit_start: usize = 0;

    while !cursor.is_eof() {
        // Handle traditional escapes FIRST (only for non-raw strings).
        // This matches the tokenizer's behavior: `\{` should not start an interpolation.
        if !is_raw && cursor.peek() == Some('\\') {
            cursor.advance();
            if cursor.peek().is_some() {
                cursor.advance();
            }
            continue;
        }

        // Brace escapes are recognized in both raw and non-raw strings.
        // They prevent `{{` from starting an interpolation hole.
        if cursor.consume("{{").is_ok() {
            continue;
        }
        if cursor.consume("}}").is_ok() {
            continue;
        }

        // Expression start: support both `{expr}` and `${expr}`.
        let checkpoint = cursor.checkpoint();
        let expr_start_rel = cursor.position().absolute as usize;

        let marker_len = if cursor.consume("${").is_ok() {
            2
        } else if cursor.consume_char('{').is_ok() {
            1
        } else {
            cursor.restore(checkpoint);
            0
        };

        if marker_len > 0 {
            // Flush preceding literal.
            if lit_start < expr_start_rel {
                parts.push(InterpolatedStringPartOffsets {
                    kind: InterpolatedStringPartKind::Literal,
                    start: content_start + lit_start,
                    end: content_start + expr_start_rel,
                });
            }

            // Scan balanced expression (cursor is positioned after the marker).
            let Ok(()) = consume_balanced_expression_in_interpolated_string(&mut cursor) else {
                // Malformed: treat the whole content as a single literal for stability.
                return Some(vec![InterpolatedStringPartOffsets {
                    kind: InterpolatedStringPartKind::Literal,
                    start: content_start,
                    end: content_start + content.len(),
                }]);
            };

            // Expect and consume closing brace.
            if cursor.consume_char('}').is_err() {
                return Some(vec![InterpolatedStringPartOffsets {
                    kind: InterpolatedStringPartKind::Literal,
                    start: content_start,
                    end: content_start + content.len(),
                }]);
            }

            let expr_end_rel = cursor.position().absolute as usize;
            parts.push(InterpolatedStringPartOffsets {
                kind: InterpolatedStringPartKind::Expr,
                start: content_start + expr_start_rel,
                end: content_start + expr_end_rel,
            });

            lit_start = expr_end_rel;
            continue;
        }

        cursor.advance();
    }

    if lit_start < content.len() {
        parts.push(InterpolatedStringPartOffsets {
            kind: InterpolatedStringPartKind::Literal,
            start: content_start + lit_start,
            end: content_start + content.len(),
        });
    }

    if parts.is_empty() {
        parts.push(InterpolatedStringPartOffsets {
            kind: InterpolatedStringPartKind::Literal,
            start: content_start,
            end: content_start + content.len(),
        });
    }

    Some(parts)
}

fn consume_balanced_expression_in_interpolated_string(
    cursor: &mut CharCursor,
) -> Result<(), CharLexerError> {
    let _start = cursor.checkpoint();
    let mut brace_depth: i32 = 1;

    while brace_depth > 0 {
        if cursor.is_eof() {
            return Err(CharLexerError::UnexpectedEof {
                expected: "}".to_string(),
                span: cursor.current_span(),
            });
        }

        // Skip whitespace + comments like the main tokenizer.
        cursor.parse::<Repeat<Either<Comment, Whitespace>>>().ok();

        match cursor.peek() {
            Some('{') => {
                cursor.advance();
                brace_depth += 1;
            }
            Some('}') => {
                brace_depth -= 1;
                if brace_depth == 0 {
                    break;
                }
                cursor.advance();
            }
            Some('"') | Some('\'') | Some('`') => {
                // Skip string literals inside the expression to avoid counting braces within them.
                let checkpoint = cursor.checkpoint();
                if cursor.parse::<StringLitLexed>().is_err() {
                    cursor.restore(checkpoint);
                    cursor.advance();
                }
            }
            _ => {
                cursor.advance();
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Default, Copy, PartialEq)]
pub struct Modifiers {
    raw: bool,          // r
    interpolated: bool, // i
}

impl Modifiers {
    pub fn is_raw(&self) -> bool {
        self.raw
    }

    pub fn is_interpolated(&self) -> bool {
        self.interpolated
    }
}

impl ParseChars for Modifiers {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        type R = Char<'r'>;
        type I = Char<'i'>;
        type Mod = OneOf4<(R, I), (I, R), R, I>;
        let modifier = cursor.parse::<Mod>()?;
        let (raw, interpolated) = match modifier {
            OneOf4::_1(_) | OneOf4::_2(_) => (true, true),
            OneOf4::_3(_) => (true, false),
            OneOf4::_4(_) => (false, true),
        };
        // let mut raw = false;
        // let mut interpolated = false;
        //
        // let modifier = cursor.consume_while_m_n(1, 2, |c| c == 'r' || c == 'i')?;
        // let mut chars = modifier.chars();
        // // Safe due to consume_while_m_n's min=1
        // let first = chars.next().unwrap();
        // let second = chars.next();
        // if let Some(second) = second {
        //     if first == second {
        //         return Err(CharLexerError::DuplicateModifier {
        //             modifier: first,
        //             span: cursor.current_span(),
        //         });
        //     }
        // }
        //
        // for c in modifier.chars() {
        //     match c {
        //         'r' => raw = true,
        //         'i' => interpolated = true,
        //         _ => {
        //             return Err(CharLexerError::InvalidModifiers {
        //                 span: cursor.current_span(),
        //             });
        //         }
        //     }
        // }
        Ok(Modifiers { raw, interpolated })
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

// - `[tag][-modifiers][delimiter]<quote>content<quote>[delimiter]`.
#[derive(Debug, Clone, PartialEq)]
pub struct StringLitLexed {
    /// "geo", "dt", etc
    pub tag: Option<StringTag>,
    pub modifiers: Option<Modifiers>,
    /// Number of #s
    pub delimiter_level: u8,
    pub raw_content: Span,
    pub span: Span,
    pub quote_char: char,
}

impl StringLitLexed {
    pub fn tag(&self) -> Option<Span> {
        self.tag.map(|t| t.value)
    }

    pub fn modifiers(&self) -> Option<Modifiers> {
        self.modifiers
    }

    pub fn delimiter_level(&self) -> u8 {
        self.delimiter_level
    }
}

impl ParseChars for StringLitLexed {
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let start = cursor.checkpoint();
        // let tag = cursor.parse::<Option<StringTag>>()?;
        //
        // // 2. Parse optional modifiers. A hyphen is required only if a tag is present.
        // let modifier: Option<Modifiers> = if tag.is_some() {
        //     if cursor.consume("-").is_ok() {
        //         cursor.parse::<Option<Modifiers>>()?
        //     } else {
        //         None
        //     }
        // } else {
        //     cursor.parse::<Option<Modifiers>>()?
        // };

        type QuoteOrHash = OneOf4<Char<'\''>, Char<'"'>, Char<'`'>, Char<'#'>>;
        type TagAndModifier = (StringTag, Char<'-'>, Modifiers);
        type ModsOnly = (Modifiers, Verify<QuoteOrHash>);
        type TagOnly = (StringTag, Verify<QuoteOrHash>);

        let all = cursor.parse::<Option<OneOf3<TagAndModifier, ModsOnly, TagOnly>>>()?;
        let (tag, modifier) = match all {
            Some(OneOf3::_1((tag, _, modifier))) => (Some(tag), Some(modifier)),
            Some(OneOf3::_2(mods)) => (None, Some(mods.0)),
            Some(OneOf3::_3((tag, _))) => (Some(tag), None),
            None => (None, None),
        };

        let delimeter = cursor.parse::<DelimiterParser>().unwrap_or_default();

        // Capture the opening quote character
        let open_quote = cursor.parse::<OneOf3<Char<'\''>, Char<'"'>, Char<'`'>>>()?;
        let quote_char = match open_quote {
            OneOf3::_1(_) => '\'',
            OneOf3::_2(_) => '"',
            OneOf3::_3(_) => '`',
        };

        if modifier.is_some_and(|m| !m.raw) && delimeter.level > 0 {
            return Err(CharLexerError::InvalidStringDelimeterWithModifier {
                span: cursor.current_span(),
            });
        }

        let closing_delimiter = format!("{}{}", quote_char, "#".repeat(delimeter.level as usize));
        let (_content, content_span) = cursor.until_b4_str(&closing_delimiter)?;
        cursor.consume(&closing_delimiter)?;
        if cursor.verify("#").is_ok() {
            return Err(CharLexerError::UnMatchedStringDelimeter {
                span: cursor.current_span(),
                opening: delimeter.level as usize,
            });
        }

        Ok(StringLitLexed {
            tag,
            modifiers: modifier,
            delimiter_level: delimeter.level,
            raw_content: content_span,
            span: cursor.span_since(start),
            quote_char,
        })
    }
}

impl Display for StringLitLexed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.quote_char, self.raw_content, self.quote_char
        )
    }
}

#[cfg(test)]
mod string_lit_test {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::simple("geo'content'", "geo", "", 0, "content", '\'')]
    #[case::with_modifiers("geo-i'content'", "geo", "i", 0, "content", '\'')]
    #[case::with_raw("geo-r'content'", "geo", "r", 0, "content", '\'')]
    #[case::with_raw("r'content'", "", "r", 0, "content", '\'')]
    #[case::with_raw("i'content'", "", "i", 0, "content", '\'')]
    #[case::with_raw("dt'content'", "dt", "", 0, "content", '\'')]
    #[case::with_raw("dt#'cont'ent'#", "dt", "", 1, "cont'ent", '\'')]
    #[case::with_raw("dt#'cont''ent'#", "dt", "", 1, "cont''ent", '\'')]
    #[case::with_raw("dt#'cont'''ent'#", "dt", "", 1, "cont'''ent", '\'')]
    #[case::with_raw("dt##'cont'''ent'##", "dt", "", 2, "cont'''ent", '\'')]
    #[case::with_raw("dt##'cont''#'ent'##", "dt", "", 2, "cont''#'ent", '\'')]
    #[case::with_raw("dt###'cont''##'ent'###", "dt", "", 3, "cont''##'ent", '\'')]
    #[case::with_delimiter("geo##'content'##", "geo", "", 2, "content", '\'')]
    #[case::with_modifiers_and_delimiter("geo-ir##'content'##", "geo", "ir", 2, "content", '\'')]
    #[case::with_modifiers_and_delimiter("geo-ri##'content'##", "geo", "ri", 2, "content", '\'')]
    #[case::with_modifiers_and_delimiter("geo-ir###'content'###", "geo", "ir", 3, "content", '\'')]
    #[case::with_modifiers_and_delimiter("dt-ir###'content'###", "dt", "ir", 3, "content", '\'')]
    #[case::with_modifiers_and_delimiter("dt-ir####'content'####", "dt", "ir", 4, "content", '\'')]
    #[case::double_quote("geo\"content\"", "geo", "", 0, "content", '"')]
    #[case::backtick("geo`content`", "geo", "", 0, "content", '`')]
    #[case::double_quote_modifiers("dt-i\"content\"", "dt", "i", 0, "content", '"')]
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
            .parse_exact::<StringLitLexed>()
            .map_err(|e| e.to_string())
            .unwrap();
        let raw_content = string_lit.raw_content.as_slice(&cursor);

        let tag_value = string_lit.tag.unwrap_or_default().value.as_slice(&cursor);
        assert_eq!(tag_value, tag);
        assert_eq!(raw_content, content);
        assert_eq!(string_lit.delimiter_level, delimiter);
        assert_eq!(string_lit.quote_char, quote_char);
        assert_eq!(
            string_lit.modifiers.unwrap_or_default().interpolated,
            modifiers.contains('i')
        );
        assert_eq!(
            string_lit.modifiers.unwrap_or_default().raw,
            modifiers.contains('r')
        );

        cursor.reset_dangerous();
        let string_lit = cursor
            .parse::<StringLitLexed>()
            .map_err(|e| e.to_string())
            .unwrap();
        let raw_content = string_lit.raw_content.as_slice(&cursor);
        let tag_value = string_lit.tag.unwrap_or_default().value.as_slice(&cursor);

        assert_eq!(tag_value, tag);
        assert_eq!(raw_content, content);
        assert_eq!(string_lit.delimiter_level, delimiter);
        assert_eq!(string_lit.quote_char, quote_char);
        assert_eq!(
            string_lit.modifiers.unwrap_or_default().interpolated,
            modifiers.contains('i')
        );
        assert_eq!(
            string_lit.modifiers.unwrap_or_default().raw,
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
            .parse::<StringLitLexed>()
            .map_err(|e| e.to_string())
            .unwrap();
        let raw_content = string_lit.raw_content.as_slice(&cursor);
        let tag_value = string_lit.tag.unwrap_or_default().value.as_slice(&cursor);
        assert_eq!(tag_value, tag);

        assert_eq!(raw_content, content);
        assert_eq!(string_lit.delimiter_level, delimiter);
        assert_eq!(
            string_lit.modifiers.unwrap_or_default().interpolated,
            modifiers.contains('i')
        );
        assert_eq!(
            string_lit.modifiers.unwrap_or_default().raw,
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
        let string_lit = cursor.parse::<StringLitLexed>();
        assert!(string_lit.is_err());
    }
}

#[cfg(test)]
mod escape_tests {
    use crate::Interner;
    use crate::{InterpolatedPart, Literal, StrKind, TokenKind};

    #[test]
    fn test_escape_handling() {
        let mut interner = Interner::new();

        // Test regular string with escapes
        let input1 = r#""Line1\nLine2""#;
        let tokens1 = TokenKind::tokenize(input1, &mut interner).unwrap();
        eprintln!("Tokens produced: {} tokens", tokens1.tokens.len());
        if !tokens1.tokens.is_empty() {
            eprintln!("First token: {:?}", tokens1.tokens[0].kind());
        }
        let str_lit = &tokens1.tokens[0];
        if let TokenKind::Lit(literal) = str_lit.kind() {
            if let Literal::Str(stri) = literal {
                let processed = interner.resolve(&stri.value);
                assert_eq!(processed, "Line1\nLine2");
            } else {
                panic!("Expected string literal");
            }
        } else {
            panic!("Expected literal token");
        }

        // Test raw string (should not process escapes)
        let input2 = r#"r"Line1\nLine2""#;
        let tokens2 = TokenKind::tokenize(input2, &mut interner).unwrap();
        let str_lit = &tokens2.tokens[0];
        if let TokenKind::Lit(literal) = str_lit.kind() {
            if let Literal::Str(stri) = literal {
                let processed = interner.resolve(&stri.value);
                assert_eq!(processed, r"Line1\nLine2");
            } else {
                panic!("Expected string literal");
            }
        } else {
            panic!("Expected literal token");
        }

        // Test interpolated string with escapes
        let input3 = r#"i"Line1\nLine2 {var}""#;
        let tokens3 = TokenKind::tokenize(input3, &mut interner).unwrap();
        let interpolated_lit = &tokens3.tokens[0];
        if let TokenKind::InterpolatedString {
            parts,
            kind: StrKind::Normal,
        } = interpolated_lit.kind()
        {
            // Check first part (literal with escapes)
            if let InterpolatedPart::Literal(symbol) = &parts[0] {
                let processed = interner.resolve(symbol);
                assert_eq!(processed, "Line1\nLine2 ");
            } else {
                panic!("Expected literal part");
            }
            // Check second part (expression)
            if let InterpolatedPart::Expression(_) = &parts[1] {
                // Expression part exists
            } else {
                panic!("Expected expression part");
            }
        } else {
            panic!("Expected interpolated string");
        }

        // Test raw interpolated string (should not process escapes)
        let input4 = r#"ri"Line1\nLine2 {var}""#;
        let tokens4 = TokenKind::tokenize(input4, &mut interner).unwrap();
        let interpolated_lit = &tokens4.tokens[0];
        if let TokenKind::InterpolatedString {
            parts,
            kind: StrKind::Raw { hash_count: 0 },
        } = interpolated_lit.kind()
        {
            // Check first part (literal without escape processing)
            if let InterpolatedPart::Literal(symbol) = &parts[0] {
                let processed = interner.resolve(symbol);
                assert_eq!(processed, r"Line1\nLine2 ");
            } else {
                panic!("Expected literal part");
            }
        } else {
            panic!("Expected raw interpolated string");
        }

        // Test `${expr}` interpolation marker is NOT included in literal parts.
        let input5 = r#"i"Hello ${var}!""#;
        let tokens5 = TokenKind::tokenize(input5, &mut interner).unwrap();
        let interpolated_lit = &tokens5.tokens[0];
        if let TokenKind::InterpolatedString {
            parts,
            kind: StrKind::Normal,
        } = interpolated_lit.kind()
        {
            assert!(matches!(
                parts.get(1),
                Some(InterpolatedPart::Expression(_))
            ));

            let InterpolatedPart::Literal(symbol0) = &parts[0] else {
                panic!("Expected first part to be literal");
            };
            assert_eq!(interner.resolve(symbol0), "Hello ");

            let InterpolatedPart::Literal(symbol2) = &parts[2] else {
                panic!("Expected third part to be literal");
            };
            assert_eq!(interner.resolve(symbol2), "!");
        } else {
            panic!("Expected interpolated string");
        }

        // Regression: a literal '$' immediately before `{expr}` stays literal.
        let input6 = r#"i"Price $ {var}""#;
        let tokens6 = TokenKind::tokenize(input6, &mut interner).unwrap();
        let interpolated_lit = &tokens6.tokens[0];
        if let TokenKind::InterpolatedString {
            parts,
            kind: StrKind::Normal,
        } = interpolated_lit.kind()
        {
            let InterpolatedPart::Literal(symbol0) = &parts[0] else {
                panic!("Expected first part to be literal");
            };
            assert_eq!(interner.resolve(symbol0), "Price $ ");
        } else {
            panic!("Expected interpolated string");
        }
    }
}

#[cfg(test)]
mod interpolated_scan_tests {
    use super::*;
    use std::ops::Range;

    fn slice(text: &str, r: Range<usize>) -> &str {
        text.get(r).unwrap_or("")
    }

    #[test]
    fn scan_parts_excludes_prefix_and_quotes_and_includes_markers() {
        let tok = r#"i"Hello ${var}!""#;
        let parts = scan_interpolated_string_parts_in_token_text(tok).unwrap();

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].kind, InterpolatedStringPartKind::Literal);
        assert_eq!(slice(tok, parts[0].start..parts[0].end), "Hello ");

        assert_eq!(parts[1].kind, InterpolatedStringPartKind::Expr);
        assert_eq!(slice(tok, parts[1].start..parts[1].end), "${var}");

        assert_eq!(parts[2].kind, InterpolatedStringPartKind::Literal);
        assert_eq!(slice(tok, parts[2].start..parts[2].end), "!");
    }

    #[test]
    fn scan_parts_supports_brace_only_marker() {
        let tok = r#"i"Hi {name}""#;
        let parts = scan_interpolated_string_parts_in_token_text(tok).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(slice(tok, parts[0].start..parts[0].end), "Hi ");
        assert_eq!(slice(tok, parts[1].start..parts[1].end), "{name}");
    }

    #[test]
    fn scan_parts_respects_brace_escapes() {
        let tok = r#"i"{{not_expr}} {x}""#;
        let parts = scan_interpolated_string_parts_in_token_text(tok).unwrap();
        // First part should include the escaped braces and text.
        assert!(parts.len() >= 2);
        assert_eq!(parts[0].kind, InterpolatedStringPartKind::Literal);
        assert!(slice(tok, parts[0].start..parts[0].end).starts_with("{{not_expr}} "));
    }
}
