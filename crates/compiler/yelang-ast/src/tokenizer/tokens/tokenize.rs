use super::{InterpolatedPart, TokenKind};
use crate::Interner;
use crate::{
    DateTimeLit, DurationLit, FloatLit, GeometryLit, Ident, IntegerLit, Literal, RecordIdLit,
    StrKind, StringLit, UuidLit,
};
use yelang_lexer::{self, CarriageReturn, OneOf3, Tab, TokenStreamBuilder};
use yelang_lexer::{
    Any, ByteLexed, Char, CharCursor, CharLexerError, CharLexerResult, Comment, DatetimeLexed,
    DurationLexed, Either, FloatLexed, Geometry, IdentLexed, IntLexed, RecordIdLexed, Repeat, Span,
    StringLitLexed, Token, TokenError, TokenStream, UuidLexed, Whitespace, try_parse, word::Word2,
};
use yelang_lexer::{FileId, Position};

// impl ParseTokenStream<Token> for Token {
//     fn parse(stream: &mut TokenStream<Token>) -> TokenResult<Self> {
//         let span = stream.current_span();
//         let token = stream.advance().ok_or_else(|| TokenError::UnexpectedEof {
//             expected: "any token".into(),
//             span,
//         })?;
//         Ok(token.kind().to_owned())
//     }
// }

impl TokenKind {
    /// Tokenizes the input string into a TokenStream, interning strings using the provided interner.
    pub fn tokenize(input: &str, interner: &Interner) -> Result<TokenStream<crate::tokenizer::TokenKind>, TokenError> {
        Self::tokenize_with_file_id(input, interner, FileId::default())
    }

    /// Tokenizes with an explicit file id so `Span` values across multiple files don't collide.
    pub fn tokenize_with_file_id(
        input: &str,
        interner: &Interner,
        file_id: FileId,
    ) -> Result<TokenStream<crate::tokenizer::TokenKind>, TokenError> {
        let mut cursor = CharCursor::new_with_file_id(input, file_id);
        let mut tokens = TokenStreamBuilder::<crate::tokenizer::TokenKind>::new(interner.clone());

        while let Some(token_meta) = Self::next_token(&mut cursor, interner)? {
            let (kind, span) = token_meta.into_parts();
            tokens.append(kind, span);
        }

        Ok(tokens.build())
    }

    /// Lossless tokenization for tolerant syntax.
    ///
    /// Unlike `tokenize_with_file_id`, this emits trivia tokens (`Whitespace`, `Comment`)
    /// instead of skipping them. This MUST NOT be used as input to the AST parser.
    pub fn tokenize_lossless_with_file_id(
        input: &str,
        interner: &Interner,
        file_id: FileId,
    ) -> Result<TokenStream<crate::tokenizer::TokenKind>, TokenError> {
        let mut cursor = CharCursor::new_with_file_id(input, file_id);
        let mut tokens = TokenStreamBuilder::<crate::tokenizer::TokenKind>::new(interner.clone());

        while let Some(token_meta) = Self::next_token_lossless(&mut cursor, interner)? {
            let (kind, span) = token_meta.into_parts();
            tokens.append(kind, span);
        }

        Ok(tokens.build())
    }

    /// Lossless tokenization for a substring window, seeded with the *global* start position.
    ///
    /// This is the correctness-critical building block for incremental re-lexing: it allows lexing
    /// a slice of the document while producing globally-consistent `Span` coordinates.
    pub fn tokenize_lossless_with_file_id_and_start_pos(
        input: &str,
        interner: &Interner,
        file_id: FileId,
        start_pos: Position,
    ) -> Result<TokenStream<crate::tokenizer::TokenKind>, TokenError> {
        let mut cursor = CharCursor::new_with_file_id_and_start_pos(input, file_id, start_pos);
        let mut tokens = TokenStreamBuilder::<crate::tokenizer::TokenKind>::new(interner.clone());

        while let Some(token_meta) = Self::next_token_lossless(&mut cursor, interner)? {
            let (kind, span) = token_meta.into_parts();
            tokens.append(kind, span);
        }

        Ok(tokens.build())
    }

    /// Incremental-friendly lossless retokenization: reuse the old stream prefix and re-lex the
    /// suffix starting from the beginning of the token that *covers* `edit_start_abs`.
    ///
    /// This is a conservative but correctness-first strategy:
    /// - If the edit happens inside a string/comment, we restart lexing at the start of that token.
    /// - Everything strictly before that token start is reused as-is (requires edit ranges start
    ///   at/after `edit_start_abs`, as with normal text edits).
    ///
    /// Returns a full new lossless `TokenStream` for `new_input`.
    pub fn retokenize_lossless_suffix_from_covering_token_start(
        old_stream: &TokenStream<crate::tokenizer::TokenKind>,
        new_input: &str,
        file_id: FileId,
        edit_start_abs: usize,
    ) -> Result<TokenStream<crate::tokenizer::TokenKind>, TokenError> {
        let old_tokens = old_stream.tokens.as_ref();
        let interner = &old_stream.interner;

        if old_tokens.is_empty() {
            return Self::tokenize_lossless_with_file_id(new_input, interner, file_id);
        }

        let edit_start_abs = edit_start_abs.min(new_input.len());
        let mut covering_idx: Option<usize> = None;

        // Prefer exact token start matches first.
        for (i, t) in old_tokens.iter().enumerate() {
            if t.span().start().absolute == edit_start_abs {
                covering_idx = Some(i);
                break;
            }
        }
        // Otherwise pick the token whose span covers the offset.
        if covering_idx.is_none() {
            for (i, t) in old_tokens.iter().enumerate() {
                let s = t.span().start().absolute;
                let e = t.span().end().absolute;
                if s <= edit_start_abs && edit_start_abs < e {
                    covering_idx = Some(i);
                    break;
                }
            }
        }
        // Otherwise fall back to the last token that starts before the offset.
        let covering_idx = covering_idx.unwrap_or_else(|| {
            old_tokens
                .iter()
                .rposition(|t| t.span().start().absolute <= edit_start_abs)
                .unwrap_or(0)
        });

        let start_pos = old_tokens[covering_idx].span().start();
        let start_abs = start_pos.absolute;

        // Token start positions should always be valid UTF-8 boundaries.
        debug_assert!(new_input.is_char_boundary(start_abs));
        let window_input = &new_input[start_abs..];
        let suffix = Self::tokenize_lossless_with_file_id_and_start_pos(
            window_input,
            interner,
            file_id,
            start_pos,
        )?;

        let mut out = TokenStreamBuilder::<crate::tokenizer::TokenKind>::new(interner.clone());
        for t in old_tokens.iter().take(covering_idx) {
            out.append(t.kind().clone(), t.span());
        }
        for t in suffix.tokens.iter() {
            out.append(t.kind().clone(), t.span());
        }

        Ok(out.build())
    }

    fn advance_position_by_text(mut pos: Position, text: &str) -> Position {
        // Mirror `CharCursor::advance` behavior to preserve line/column correctness.
        const NEWLINES: [char; 7] = [
            '\n',       // Line Feed (LF)
            '\r',       // Carriage Return (CR)
            '\x0C',     // Form Feed
            '\x0B',     // Vertical Tab
            '\u{0085}', // Next Line (NEL)
            '\u{2028}', // Line Separator
            '\u{2029}', // Paragraph Separator
        ];

        let mut it = text.chars().peekable();
        while let Some(ch) = it.next() {
            if ch == '\r' {
                // Merge CRLF like the cursor.
                if matches!(it.peek(), Some('\n')) {
                    let _ = it.next();
                    pos.absolute += 2;
                } else {
                    pos.absolute += 1;
                }
                pos.line = pos.line.saturating_add(1);
                pos.column = 1;
                continue;
            }

            pos.absolute += ch.len_utf8();

            match ch {
                '\n' => {
                    pos.line = pos.line.saturating_add(1);
                    pos.column = 1;
                }
                '\t' => {
                    pos.column = pos.column.saturating_add(4);
                }
                _ if NEWLINES.contains(&ch) => {
                    pos.line = pos.line.saturating_add(1);
                    pos.column = 1;
                }
                _ => {
                    pos.column = pos.column.saturating_add(1);
                }
            }
        }

        pos
    }

    fn span_slice<'a>(input: &'a str, span: Span) -> Option<&'a str> {
        let start = span.start().absolute;
        let end = span.end().absolute;
        input.get(start..end)
    }

    /// Incremental lossless retokenization with a bounded re-lex window.
    ///
    /// Strategy:
    /// - Reuse the old token prefix up to the covering token start.
    /// - Re-lex forward from that safe start, but stop once we re-synchronize with an unchanged
    ///   suffix signature from the old stream.
    /// - After resync, reuse the old suffix token *kinds* and recompute their spans by scanning
    ///   their (unchanged) text.
    ///
    /// This preserves global-correct `Span` (absolute/line/column) even if the edit inserts or
    /// removes newlines/tabs.
    ///
    /// Falls back to `retokenize_lossless_suffix_from_covering_token_start` if resync fails.
    pub fn retokenize_lossless_bounded_window_from_covering_token_start(
        old_stream: &TokenStream<crate::tokenizer::TokenKind>,
        old_input: &str,
        new_input: &str,
        file_id: FileId,
        edit_start_abs: usize,
        edit_end_abs: usize,
    ) -> Result<TokenStream<crate::tokenizer::TokenKind>, TokenError> {
        let old_tokens = old_stream.tokens.as_ref();
        let interner = &old_stream.interner;

        if old_tokens.is_empty() {
            return Self::tokenize_lossless_with_file_id(new_input, interner, file_id);
        }

        // Basic safety: the old spans are in old_input coordinates.
        let edit_start_abs = edit_start_abs.min(old_input.len());
        let edit_end_abs = edit_end_abs.min(old_input.len());

        // Find covering token index (same logic as the suffix-to-EOF retokenizer).
        let mut covering_idx: Option<usize> = None;
        for (i, t) in old_tokens.iter().enumerate() {
            if t.span().start().absolute == edit_start_abs {
                covering_idx = Some(i);
                break;
            }
        }
        if covering_idx.is_none() {
            for (i, t) in old_tokens.iter().enumerate() {
                let s = t.span().start().absolute;
                let e = t.span().end().absolute;
                if s <= edit_start_abs && edit_start_abs < e {
                    covering_idx = Some(i);
                    break;
                }
            }
        }
        let covering_idx = covering_idx.unwrap_or_else(|| {
            old_tokens
                .iter()
                .rposition(|t| t.span().start().absolute <= edit_start_abs)
                .unwrap_or(0)
        });

        let start_pos = old_tokens[covering_idx].span().start();
        let start_abs = start_pos.absolute;

        // Attempt to resync at the first token that starts at/after the old edit end.
        let mut anchor_idx = old_tokens
            .iter()
            .position(|t| t.span().start().absolute >= edit_end_abs)
            .unwrap_or(old_tokens.len());
        if anchor_idx < covering_idx {
            anchor_idx = covering_idx;
        }
        if anchor_idx >= old_tokens.len() {
            // Nothing to resync with; just do the simple suffix-to-EOF retokenization.
            return Self::retokenize_lossless_suffix_from_covering_token_start(
                old_stream,
                new_input,
                file_id,
                edit_start_abs,
            );
        }

        let signature_len: usize = 12;
        let min_signature_len: usize = 6;
        let sig_available = old_tokens.len() - anchor_idx;
        let sig_len = signature_len.min(sig_available);
        if sig_len < min_signature_len {
            return Self::retokenize_lossless_suffix_from_covering_token_start(
                old_stream,
                new_input,
                file_id,
                edit_start_abs,
            );
        }

        // Precompute old signature slices.
        let mut old_sig: Vec<(TokenKind, String)> = Vec::with_capacity(sig_len);
        for t in &old_tokens[anchor_idx..anchor_idx + sig_len] {
            let s = match Self::span_slice(old_input, t.span()) {
                Some(s) => s,
                None => {
                    return Self::retokenize_lossless_suffix_from_covering_token_start(
                        old_stream,
                        new_input,
                        file_id,
                        edit_start_abs,
                    );
                }
            };
            old_sig.push((t.kind().clone(), s.to_string()));
        }

        // Re-lex forward from the safe start, but search for the signature.
        // Hard caps keep this correctness-first and avoid runaway work.
        let max_window_tokens: usize = 4096;
        let max_window_bytes: usize = 128 * 1024;

        if !new_input.is_char_boundary(start_abs) {
            return Self::retokenize_lossless_suffix_from_covering_token_start(
                old_stream,
                new_input,
                file_id,
                edit_start_abs,
            );
        }

        let mut cursor =
            CharCursor::new_with_file_id_and_start_pos(&new_input[start_abs..], file_id, start_pos);
        let mut window_tokens: Vec<(TokenKind, Span)> = Vec::new();
        let mut match_at_end: Option<usize> = None;

        while window_tokens.len() < max_window_tokens {
            let consumed = cursor.current_pos().absolute.saturating_sub(start_abs);
            if consumed > max_window_bytes {
                break;
            }

            let Some(tok) = Self::next_token_lossless(&mut cursor, interner)? else {
                break;
            };
            let (kind, span) = tok.into_parts();
            window_tokens.push((kind, span));

            if window_tokens.len() >= sig_len {
                let candidate = window_tokens.len() - sig_len;
                let mut ok = true;
                for j in 0..sig_len {
                    let (new_kind, new_span) = &window_tokens[candidate + j];
                    let (old_kind, old_text) = &old_sig[j];
                    if new_kind != old_kind {
                        ok = false;
                        break;
                    }
                    let new_text = match Self::span_slice(new_input, *new_span) {
                        Some(s) => s,
                        None => {
                            ok = false;
                            break;
                        }
                    };
                    if new_text != old_text {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    match_at_end = Some(candidate);
                    break;
                }
            }
        }

        let Some(resync_start) = match_at_end else {
            return Self::retokenize_lossless_suffix_from_covering_token_start(
                old_stream,
                new_input,
                file_id,
                edit_start_abs,
            );
        };

        // Build output: old prefix + window prefix + matched signature (from window).
        let mut out = TokenStreamBuilder::new(interner.clone());
        for t in old_tokens.iter().take(covering_idx) {
            out.append(t.kind().clone(), t.span());
        }
        for (kind, span) in window_tokens.iter().take(resync_start) {
            out.append(kind.clone(), *span);
        }
        for (kind, span) in window_tokens.iter().skip(resync_start) {
            out.append(kind.clone(), *span);
        }

        // Reuse the remaining old suffix token kinds, but recompute their spans by scanning text.
        let mut cur_pos = window_tokens
            .last()
            .map(|(_, s)| s.end())
            .unwrap_or(start_pos);

        let suffix_start = anchor_idx + sig_len;
        for t in old_tokens.iter().skip(suffix_start) {
            let old_span = t.span();
            let Some(text) = Self::span_slice(old_input, old_span) else {
                return Self::retokenize_lossless_suffix_from_covering_token_start(
                    old_stream,
                    new_input,
                    file_id,
                    edit_start_abs,
                );
            };

            // Validate the unchanged suffix text still matches at the computed position.
            let end_abs = cur_pos.absolute.saturating_add(text.len());
            if !new_input.is_char_boundary(cur_pos.absolute) || !new_input.is_char_boundary(end_abs)
            {
                return Self::retokenize_lossless_suffix_from_covering_token_start(
                    old_stream,
                    new_input,
                    file_id,
                    edit_start_abs,
                );
            }
            if new_input.get(cur_pos.absolute..end_abs) != Some(text) {
                return Self::retokenize_lossless_suffix_from_covering_token_start(
                    old_stream,
                    new_input,
                    file_id,
                    edit_start_abs,
                );
            }

            let end_pos = Self::advance_position_by_text(cur_pos, text);
            let new_span = Span::new_with_file_id(cur_pos, end_pos, file_id);
            out.append(t.kind().clone(), new_span);
            cur_pos = end_pos;
        }

        // Ensure we ended exactly at EOF; otherwise, bail to the safe path.
        if cur_pos.absolute != new_input.len() {
            return Self::retokenize_lossless_suffix_from_covering_token_start(
                old_stream,
                new_input,
                file_id,
                edit_start_abs,
            );
        }

        Ok(out.build())
    }

    pub fn tokenize_lossless(
        input: &str,
        interner: &Interner,
    ) -> Result<TokenStream<crate::tokenizer::TokenKind>, TokenError> {
        Self::tokenize_lossless_with_file_id(input, interner, FileId::default())
    }

    /// Parses the next token from the cursor, interning strings using the provided interner.
    fn next_token(
        cursor: &mut CharCursor,
        interner: &Interner,
    ) -> CharLexerResult<Option<Token<crate::tokenizer::TokenKind>>> {
        type Space = OneOf3<Whitespace, Tab, CarriageReturn>;
        cursor.parse::<Repeat<Either<Comment, Space>>>().ok();

        if cursor.peek().is_none() {
            return Ok(None);
        }
        // type _3Quotes = OneOf3<Char<'\''>, Char<'"'>, Char<'`'>>;
        type _3Quotes = Char<'\''>;

        let start_pos = cursor.checkpoint();

        // ### CRITICAL FIX: Lifetime-or-String disambiguation (rustc's lifetime_or_char approach) ###
        // Handle single quote disambiguation FIRST before anything else
        // When we see ', peek ahead to determine if it's a lifetime or string
        if cursor.peek() == Some('\'') {
            let checkpoint = cursor.checkpoint();
            cursor.parse::<Char<'\''>>().ok();

            // Try to parse an identifier after the quote
            if let Ok(ident) = cursor.parse::<IdentLexed>() {
                // Check if there's a closing quote
                if cursor.peek() == Some('\'') {
                    // Has closing quote → It's a string literal
                    // Restore and let StringLitLexed handle it below
                    cursor.restore(checkpoint);
                } else {
                    // No closing quote → It's a lifetime/label!
                    let name = interner.intern(cursor.str_from_span(ident.span()));
                    let span = cursor.span_since(checkpoint);
                    return Ok(Some(Token::<crate::tokenizer::TokenKind>::new(TokenKind::Lifetime(name), span)));
                }
            } else {
                // Not an identifier after ' → restore and try string parsing below
                cursor.restore(checkpoint);
            }
        }

        // ### CRITICAL FIX: Try string literals BEFORE identifiers ###
        // String modifiers like r"...", i"...", r'...' should be parsed as strings,
        // not as identifier 'r' or 'i' followed by a separate string.
        // StringLitLexed already handles all modifier patterns (r, i, ri, ir) so we just
        // need to try it before identifier parsing.
        if let Ok(s) = cursor.parse::<StringLitLexed>() {
            let tag_str = s.tag().map(|span| cursor.str_from_span(span));
            let content_slice = cursor.str_from_span(s.raw_content);
            let is_interpolated = s.modifiers().is_some_and(|m| m.is_interpolated());
            let is_raw = s.modifiers().is_some_and(|m| m.is_raw());

            let kind = match (tag_str, is_interpolated) {
                (Some("dt"), false) => {
                    let mut content_cursor = CharCursor::new(content_slice);
                    if content_cursor.parse_exact::<DatetimeLexed>().is_err() {
                        return Err(CharLexerError::UnexpectedStr {
                            expected: "valid datetime".to_string(),
                            found: content_slice.to_string(),
                            span: s.span,
                        });
                    }
                    TokenKind::Lit(Literal::DateTime(DateTimeLit {
                        value: interner.intern(content_slice),
                        format: None,
                    }))
                }
                (Some("du"), false) => {
                    let mut content_cursor = CharCursor::new(content_slice);
                    if content_cursor.parse_exact::<DurationLexed>().is_err() {
                        return Err(CharLexerError::UnexpectedStr {
                            expected: "valid duration".to_string(),
                            found: content_slice.to_string(),
                            span: s.span,
                        });
                    }
                    TokenKind::Lit(Literal::Duration(DurationLit {
                        value: interner.intern(content_slice),
                    }))
                }
                (Some("geo"), false) => {
                    let mut content_cursor = CharCursor::new(content_slice);
                    if content_cursor.parse_exact::<Geometry>().is_err() {
                        return Err(CharLexerError::UnexpectedStr {
                            expected: "valid geometry".to_string(),
                            found: content_slice.to_string(),
                            span: s.span,
                        });
                    }
                    TokenKind::Lit(Literal::Geometry(GeometryLit {
                        value: interner.intern(content_slice),
                    }))
                }
                (Some("uuid"), false) => {
                    let mut content_cursor = CharCursor::new(content_slice);
                    if content_cursor.parse_exact::<UuidLexed>().is_err() {
                        return Err(CharLexerError::UnexpectedStr {
                            expected: "valid uuid".to_string(),
                            found: content_slice.to_string(),
                            span: s.span,
                        });
                    }
                    TokenKind::Lit(Literal::Uuid(UuidLit {
                        value: interner.intern(content_slice),
                    }))
                }
                (Some("id"), false) => {
                    let mut content_cursor = CharCursor::new(content_slice);
                    if content_cursor.parse_exact::<RecordIdLexed>().is_err() {
                        return Err(CharLexerError::UnexpectedStr {
                            expected: "valid record id".to_string(),
                            found: content_slice.to_string(),
                            span: s.span,
                        });
                    }
                    TokenKind::Lit(Literal::RecordId(RecordIdLit {
                        value: interner.intern(content_slice),
                    }))
                }
                (None, false) => {
                    // Regular non-interpolated string - process escapes if not raw
                    let processed_content = if is_raw {
                        interner.intern(content_slice)
                    } else {
                        let res = Self::process_string_escapes(content_slice, s.raw_content)
                            .map_err(|_| CharLexerError::UnexpectedStr {
                                expected: "valid string literal".to_string(),
                                found: content_slice.to_string(),
                                span: s.raw_content,
                            })?;
                        interner.intern(&res)
                    };
                    TokenKind::Lit(Literal::Str(StringLit {
                        value: processed_content,
                        kind: if is_raw {
                            StrKind::Raw {
                                hash_count: s.delimiter_level,
                            }
                        } else {
                            StrKind::Normal
                        },
                    }))
                }
                (None, true) => {
                    // Handle interpolated string
                    let parts = Self::parse_interpolation_parts_directly(
                        content_slice,
                        s.span,
                        interner,
                        is_raw,
                    )
                    .map_err(|_| CharLexerError::UnexpectedStr {
                        expected: "valid interpolated string".to_string(),
                        found: content_slice.to_string(),
                        span: s.span,
                    })?;
                    TokenKind::InterpolatedString {
                        parts,
                        kind: if is_raw {
                            StrKind::Raw {
                                hash_count: s.delimiter_level,
                            }
                        } else {
                            StrKind::Normal
                        },
                    }
                }
                (_, _) => TokenKind::Unknown,
            };

            return Ok(Some(Token::<crate::tokenizer::TokenKind>::new(kind, s.span)));
        }

        // Now try identifier parsing
        if let Ok(ident) = cursor.parse::<IdentLexed>() {
            let span = ident.span();
            let text = span.as_slice(cursor);

            // 2. Check if it's a keyword or special token
            let kind = match text {
                // Underscore wildcard (rustc: _ is a wildcard pattern, not a binding)
                "_" => TokenKind::Underscore,

                // SQL Keywords (Case-insensitive)
                "select" => TokenKind::Select,
                "from" => TokenKind::From_,
                "where" => TokenKind::Where,
                "group" => TokenKind::Group,
                "by" => TokenKind::By,
                "order" => TokenKind::Order,
                "into" => TokenKind::Into,
                "create" => {
                    // Handle "create index" lookahead
                    let cp = cursor.checkpoint();
                    cursor.consume_while_m(1, |c| c.is_whitespace()).ok();
                    if cursor.consume_case_insensitive("index").is_ok() {
                        TokenKind::CreateIndex
                    } else {
                        cursor.restore(cp);
                        TokenKind::Create
                    }
                }
                "update" => TokenKind::Update,
                "set" => TokenKind::Set,
                "insert" => TokenKind::Insert,
                "delete" => TokenKind::Delete,
                "link" => TokenKind::Link,
                "unlink" => TokenKind::Unlink,
                "upsert" => TokenKind::Upsert,
                "begin" => {
                    // Handle "begin transaction"
                    let cp = cursor.checkpoint();
                    if cursor.consume_while_m(1, |c| c.is_whitespace()).is_ok()
                        && cursor.consume_case_insensitive("transaction").is_ok()
                    {
                        TokenKind::BeginTransaction
                    } else {
                        cursor.restore(cp);
                        // Treat 'begin' as ident if not transaction
                        let symbol = interner.intern(text);
                        TokenKind::Ident(Ident::new(symbol, span))
                    }
                }
                "commit" => {
                    let cp = cursor.checkpoint();
                    if cursor.consume_while_m(1, |c| c.is_whitespace()).is_ok()
                        && cursor.consume_case_insensitive("transaction").is_ok()
                    {
                        TokenKind::CommitTransaction
                    } else {
                        cursor.restore(cp);
                        let symbol = interner.intern(text);
                        TokenKind::Ident(Ident::new(symbol, span))
                    }
                }
                "cancel" => {
                    let cp = cursor.checkpoint();
                    if cursor.consume_while_m(1, |c| c.is_whitespace()).is_ok()
                        && cursor.consume_case_insensitive("transaction").is_ok()
                    {
                        TokenKind::CancelTransaction
                    } else {
                        cursor.restore(cp);
                        let symbol = interner.intern(text);
                        TokenKind::Ident(Ident::new(symbol, span))
                    }
                }
                "for" => TokenKind::For,
                "links" => TokenKind::Links,
                "and" => TokenKind::And,
                "not" => TokenKind::Not,
                "xor" => TokenKind::Xor,
                "is" => TokenKind::Is,
                "in" => TokenKind::In,
                "on" => TokenKind::On,
                "asc" => TokenKind::Asc,
                "desc" => TokenKind::Desc,
                "start" => TokenKind::Start,
                "limit" => TokenKind::Limit,

                // Rust-like Keywords (Case-sensitive usually, but let's match your existing logic)
                "struct" => TokenKind::Struct,
                "enum" => TokenKind::Enum,
                "trait" => TokenKind::Trait,
                "fn" => TokenKind::Fn,
                "let" => TokenKind::Let,
                "const" => TokenKind::Const,
                "static" => TokenKind::Static,
                "impl" => TokenKind::Impl,
                "dyn" => TokenKind::Dyn,
                "pub" => TokenKind::Pub,
                "use" => TokenKind::Use,
                "mod" => TokenKind::Mod,
                "type" => TokenKind::TypeToken,
                "default" => TokenKind::DefaultKw,
                "typeof" => TokenKind::TypeOf,
                "ReturnType" => TokenKind::ReturnType,
                "Parameters" => TokenKind::Parameters,
                "Pick" => TokenKind::Pick,
                "Omit" => TokenKind::Omit,
                "return" => TokenKind::Return,
                "break" => TokenKind::Break,
                "continue" => TokenKind::Continue,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "match" => TokenKind::Match,
                "loop" => TokenKind::Loop,
                "while" => TokenKind::While,
                "async" => TokenKind::Async,
                "await" => TokenKind::Await,
                "gen" => TokenKind::Gen,
                "yield" => TokenKind::Yield,
                "as" => TokenKind::As,
                "mut" => TokenKind::Mut,
                "crate" => TokenKind::Crate,
                "self" => TokenKind::SelfKw,
                "Self" => TokenKind::SelfType,
                "super" => TokenKind::Super,
                "pkg" => TokenKind::Pkg,
                "or" => TokenKind::Or, // Logical OR
                "true" => TokenKind::Lit(Literal::Bool(true)),
                "false" => TokenKind::Lit(Literal::Bool(false)),
                "null" => TokenKind::Null,

                // Not a keyword -> Identifier
                _ => {
                    let symbol = interner.intern(text);
                    TokenKind::Ident(Ident::new(symbol, span))
                }
            };

            return Ok(Some(Token::<crate::tokenizer::TokenKind>::new(kind, span)));
        }

        // Note: Lifetime/label disambiguation is handled at the top of next_token,
        // before trying to parse strings or identifiers

        let token_kind = try_parse!(
            cursor
                .consume("true")
                .map(|_| TokenKind::Lit(Literal::Bool(true))),
            cursor
                .consume("false")
                .map(|_| TokenKind::Lit(Literal::Bool(false))),
            // Byte string literal: b"..."
            cursor
                .parse::<ByteLexed>()
                .map(|b| TokenKind::Lit(Literal::Bytes(b.value().clone()))),
            // Note: StringLitLexed is now parsed earlier, before identifier parsing,
            // to handle string modifiers like r"..." and i"..." correctly
            cursor
                .parse::<(FloatLexed, yelang_lexer::not::PeekNot<Char<'.'>>)>()
                .map(|(f, _)| TokenKind::Lit(Literal::Float(FloatLit {
                    value: interner.intern(f.span().as_slice(cursor)),
                    suffix: f.suffix(),
                }))),
            cursor
                .parse::<IntLexed>()
                .map(|int| TokenKind::Lit(Literal::Int(IntegerLit {
                    value: interner.intern(int.span().as_slice(cursor)),
                    suffix: int.suffix(),
                }))),
            cursor.parse::<Comment>().map(TokenKind::Comment),
            // Symbols
            cursor.consume("..=").map(|_| TokenKind::DotDotEq),
            cursor.consume("...").map(|_| TokenKind::DotDotDot),
            // Shift-assign must come before < / <= and > / >= tokenization.
            cursor.consume("<<=").map(|_| TokenKind::ShiftLeftEqual),
            cursor.consume(">>=").map(|_| TokenKind::ShiftRightEqual),
            cursor
                .parse::<Word2<'=', '='>>()
                .map(|_| TokenKind::EqualEqual),
            cursor.parse::<Word2<'&', '&'>>().map(|_| TokenKind::And),
            // Note: || is NOT tokenized as a single token - it becomes two Pipe tokens
            // The parser handles disambiguation: closure params vs logical OR
            cursor.consume("<->").map(|_| TokenKind::ArrowBoth),
            cursor.consume("->").map(|_| TokenKind::ArrowRight),
            cursor.consume("..").map(|_| TokenKind::DotDot),
            cursor.consume("<-").map(|_| TokenKind::ArrowLeft),
            cursor.consume("<=").map(|_| TokenKind::LessThanEqual),
            cursor
                .parse::<Word2<'>', '='>>()
                .map(|_| TokenKind::GreaterThanEqual),
            cursor.consume("=>").map(|_| TokenKind::ArrowRight2Lines),
            cursor.parse::<Char<'='>>().map(|_| TokenKind::Equal),
            cursor.consume("{").map(|_| TokenKind::OpenBrace),
            cursor.consume("?").map(|_| TokenKind::QuestionMark),
            cursor.parse::<Char<'}'>>().map(|_| TokenKind::CloseBrace),
            cursor.consume_char('[').map(|_| TokenKind::OpenBracket),
            cursor.consume_char('$').map(|_| TokenKind::Dollar),
            cursor.consume_char(']').map(|_| TokenKind::CloseBracket),
            cursor.consume_char('(').map(|_| TokenKind::OpenParen),
            cursor.consume_char(')').map(|_| TokenKind::CloseParen),
            // Compound assignment operators must be checked BEFORE their single-char versions
            cursor.consume("+=").map(|_| TokenKind::PlusEqual),
            cursor.parse::<Char<'+'>>().map(|_| TokenKind::Plus),
            cursor.consume("-=").map(|_| TokenKind::MinusEqual),
            cursor.consume_char('-').map(|_| TokenKind::Minus),
            cursor.consume("*=").map(|_| TokenKind::StarEqual),
            cursor.consume_char('*').map(|_| TokenKind::Star),
            cursor.consume("/=").map(|_| TokenKind::SlashEqual),
            cursor.parse::<Char<'/'>>().map(|_| TokenKind::Slash),
            cursor.consume("%=").map(|_| TokenKind::PercentEqual),
            cursor.parse::<Char<'%'>>().map(|_| TokenKind::Percent),
            cursor.parse::<Char<'>'>>().map(|_| TokenKind::GreaterThan),
            cursor.parse::<Char<'<'>>().map(|_| TokenKind::LessThan),
            cursor.parse::<Char<'.'>>().map(|_| TokenKind::Dot),
            cursor.parse::<Char<','>>().map(|_| TokenKind::Comma),
            cursor.consume("::").map(|_| TokenKind::ColonColon),
            cursor.parse::<Char<':'>>().map(|_| TokenKind::Colon),
            cursor.parse::<Char<';'>>().map(|_| TokenKind::Semicolon),
            cursor.consume("!=").map(|_| TokenKind::BangEqual),
            cursor.consume_char('!').map(|_| TokenKind::Bang),
            cursor.consume_char('@').map(|_| TokenKind::At),
            cursor.consume("&=").map(|_| TokenKind::AmpersandEqual),
            cursor.consume_char('&').map(|_| TokenKind::Ampersand),
            cursor.consume("^=").map(|_| TokenKind::CaretEqual),
            cursor.consume_char('^').map(|_| TokenKind::Caret),
            cursor.consume("|=").map(|_| TokenKind::PipeEqual),
            cursor.parse::<Char<'|'>>().map(|_| TokenKind::Pipe),
            cursor.parse::<Any>().map(|_| TokenKind::Unknown),
        )?;

        Ok(Some(Token::<crate::tokenizer::TokenKind>::new(token_kind, cursor.span_since(start_pos))))
    }

    fn parse_interpolation_parts_directly(
        content: &str,
        outer_span: Span,
        interner: &Interner,
        is_raw: bool,
    ) -> Result<Vec<InterpolatedPart>, TokenError> {
        let mut parts = Vec::new();
        let mut current_literal = String::new();
        let mut cursor = CharCursor::new(content);

        while !cursor.is_eof() {
            // Handle traditional escapes FIRST (only for non-raw strings)
            if !is_raw && cursor.peek() == Some('\\') {
                cursor.advance(); // Consume the backslash

                if let Some(escaped_char) = cursor.advance() {
                    let decoded =
                        Self::process_escape_sequence(escaped_char, cursor.current_span(), true)?;
                    current_literal.push(decoded);
                    continue;
                } else {
                    return Err(TokenError::SyntaxError {
                        message: "Unterminated escape sequence".to_string(),
                        span: cursor.current_span(),
                        source: None,
                    });
                }
            }

            // Then handle brace escapes (for both raw and non-raw)
            if cursor.consume("{{").is_ok() {
                current_literal.push('{');
                continue;
            }
            if cursor.consume("}}").is_ok() {
                current_literal.push('}');
                continue;
            }

            // Then handle expression start.
            // Support both `{expr}` and `${expr}` forms.
            let checkpoint = cursor.checkpoint();
            let expr_start_pos = cursor.position().absolute;

            let marker_len = if cursor.consume("${").is_ok() {
                2
            } else if cursor.consume_char('{').is_ok() {
                1
            } else {
                cursor.restore(checkpoint);
                0
            };

            if marker_len > 0 {
                // We found an expression start
                if !current_literal.is_empty() {
                    parts.push(InterpolatedPart::Literal(interner.intern(&current_literal)));
                    current_literal.clear();
                }

                // Parse balanced expression content
                let expr_content =
                    Self::parse_balanced_expression_directly(&mut cursor, outer_span)?;
                let mut expr_tokens = Self::tokenize_expression_directly(&expr_content, interner)?;

                // Adjust spans for expression tokens.
                // `expr_start_pos` is the position at the start marker (`{` or `$`), and
                // `marker_len` accounts for the consumed prefix (`{` or `${`).
                let expr_start_abs = outer_span.start().absolute + expr_start_pos + marker_len;
                for token in &mut expr_tokens {
                    let old_span = token.span();
                    let new_start = yelang_lexer::Position {
                        absolute: old_span.start().absolute + expr_start_abs,
                        line: old_span.start().line,
                        column: old_span.start().column,
                    };
                    let new_end = yelang_lexer::Position {
                        absolute: old_span.end().absolute + expr_start_abs,
                        line: old_span.end().line,
                        column: old_span.end().column,
                    };
                    let new_span = Span::new(new_start, new_end);
                    let kind = token.kind().clone();
                    *token = Token::<crate::tokenizer::TokenKind>::new(kind, new_span);
                }

                parts.push(InterpolatedPart::Expression(expr_tokens.into()));

                // Expect and consume closing brace
                cursor
                    .consume_char('}')
                    .map_err(|_| TokenError::SyntaxError {
                        message: "Unmatched opening brace in interpolated string".to_string(),
                        span: outer_span,
                        source: None,
                    })?;
            } else {
                // Accumulate literal character
                if let Some(ch) = cursor.advance() {
                    current_literal.push(ch);
                }
            }
        }

        // Add final literal part
        if !current_literal.is_empty() {
            parts.push(InterpolatedPart::Literal(interner.intern(&current_literal)));
        }

        Ok(parts)
    }

    fn process_string_escapes(content: &str, span: Span) -> Result<String, TokenError> {
        let mut result = String::new();
        let mut chars = content.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(escaped_char) = chars.next() {
                    let decoded = Self::process_escape_sequence(escaped_char, span, false)?;
                    result.push(decoded);
                } else {
                    return Err(TokenError::SyntaxError {
                        message: "Unterminated escape sequence".to_string(),
                        span,
                        source: None,
                    });
                }
            } else {
                result.push(ch);
            }
        }

        Ok(result)
    }

    /// Process a single escape sequence, returning the decoded character
    fn process_escape_sequence(
        escaped_char: char,
        span: Span,
        allow_braces: bool,
    ) -> Result<char, TokenError> {
        match escaped_char {
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            '\\' => Ok('\\'),
            '"' => Ok('"'),
            '\'' => Ok('\''),
            '`' => Ok('`'),
            '{' if allow_braces => Ok('{'),
            '}' if allow_braces => Ok('}'),
            _ => Err(TokenError::SyntaxError {
                message: format!("Invalid escape sequence: \\{}", escaped_char),
                span,
                source: None,
            }),
        }
    }

    fn parse_balanced_expression_directly(
        cursor: &mut CharCursor,
        outer_span: Span,
    ) -> Result<String, TokenError> {
        let start = cursor.checkpoint();
        let mut brace_depth = 1;

        while brace_depth > 0 {
            if cursor.is_eof() {
                return Err(TokenError::SyntaxError {
                    message: "Unterminated expression in interpolated string".to_string(),
                    span: outer_span,
                    source: None,
                });
            }

            // Skip whitespace and comments like in main tokenizer
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
                    // Use existing string literal parser
                    let checkpoint = cursor.checkpoint();
                    if let Ok(_string_lit) = cursor.parse::<StringLitLexed>() {
                        // String consumed - continue
                    } else {
                        cursor.restore(checkpoint);
                        cursor.advance(); // Not a valid string, just advance
                    }
                }
                _ => {
                    cursor.advance();
                }
            }
        }

        let expr_span = cursor.span_since(start);
        Ok(expr_span.as_slice(cursor).to_string())
    }

    fn tokenize_expression_directly(
        expr_content: &str,
        interner: &Interner,
    ) -> Result<Vec<Token<crate::tokenizer::TokenKind>>, TokenError> {
        let mut expr_cursor = CharCursor::new(expr_content);
        let mut tokens = Vec::new();

        // Use Repeat<Either<Comment, Whitespace>> exactly like in main tokenizer
        while let Some(token) = Self::next_token(&mut expr_cursor, interner)? {
            tokens.push(token);
            // Skip whitespace/comments between tokens
            expr_cursor
                .parse::<Repeat<Either<Comment, Whitespace>>>()
                .ok();
        }

        Ok(tokens)
    }

    fn next_token_lossless(
        cursor: &mut CharCursor,
        interner: &Interner,
    ) -> CharLexerResult<Option<Token<crate::tokenizer::TokenKind>>> {
        if cursor.peek().is_none() {
            return Ok(None);
        }

        // Whitespace trivia.
        if cursor.peek().is_some_and(|c| c.is_whitespace()) {
            let cp = cursor.checkpoint();
            cursor.parse::<Whitespace>()?;
            let span = cursor.span_since(cp);
            let text = cursor.str_from_span(span);
            let sym = interner.intern(text);
            return Ok(Some(Token::<crate::tokenizer::TokenKind>::new(TokenKind::Whitespace(sym), span)));
        }

        // Comment trivia.
        if cursor.peek() == Some('/') || cursor.peek() == Some('-') {
            let cp = cursor.checkpoint();
            match cursor.parse::<Comment>() {
                Ok(comment) => {
                    let span = comment.span();
                    return Ok(Some(Token::<crate::tokenizer::TokenKind>::new(TokenKind::Comment(comment), span)));
                }
                Err(_) => {
                    cursor.restore(cp);
                }
            }
        }

        // Delegate to the normal tokenizer for all non-trivia tokens.
        Self::next_token(cursor, interner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;

    #[test]
    fn lossless_tokenize_window_start_pos_matches_full_suffix() {
        let interner = Interner::new();
        let file_id = FileId::new(42);

        let input = "\
fn main() {\n\
    // comment\n\
    let x = 1 + 2;\n\
    recv::<Vec<Option<Result<u8, E>>>>(x)\n\
}\n\
";

        let offset = input.find("recv").expect("expected 'recv' in input");
        // Compute the global start position for this offset using the same cursor logic.
        let start_pos = {
            let mut cursor = CharCursor::new_with_file_id(input, file_id);
            while cursor.position().absolute < offset {
                cursor.advance();
            }
            assert_eq!(cursor.position().absolute, offset);
            cursor.position()
        };

        let full = TokenKind::tokenize_lossless_with_file_id(input, &interner, file_id).unwrap();
        let window_input = &input[offset..];
        let window = TokenKind::tokenize_lossless_with_file_id_and_start_pos(
            window_input,
            &interner,
            file_id,
            start_pos,
        )
        .unwrap();

        let full_start_idx = full
            .tokens
            .iter()
            .position(|t| t.span().start().absolute == offset)
            .expect("expected a token starting at the window offset");

        assert_eq!(
            window.tokens.len(),
            full.tokens.len() - full_start_idx,
            "window tokenization should match full suffix length"
        );

        for (i, w) in window.tokens.iter().enumerate() {
            let f = &full.tokens[full_start_idx + i];
            assert_eq!(w.kind(), f.kind(), "token kind mismatch at {i}");
            assert_eq!(w.span(), f.span(), "token span mismatch at {i}");
            assert_eq!(w.span().file_id(), file_id);
        }
    }

    #[test]
    fn lossless_retokenize_suffix_matches_full_after_string_edit() {
        let interner = Interner::new();
        let file_id = FileId::new(77);

        let old_input = "let s = \"hello world\"; let x = 1;";
        let edit_start = old_input
            .find("world")
            .expect("expected substring in old input");
        let new_input = old_input.replacen("world", "wurld", 1);

        let old_stream =
            TokenKind::tokenize_lossless_with_file_id(old_input, &interner, file_id).unwrap();
        let incremental = TokenKind::retokenize_lossless_suffix_from_covering_token_start(
            &old_stream,
            &new_input,
            file_id,
            edit_start,
        )
        .unwrap();
        let full =
            TokenKind::tokenize_lossless_with_file_id(&new_input, &interner, file_id).unwrap();

        assert_eq!(incremental.tokens.len(), full.tokens.len());
        for (i, (a, b)) in incremental
            .tokens
            .iter()
            .zip(full.tokens.iter())
            .enumerate()
        {
            assert_eq!(a.kind(), b.kind(), "kind mismatch at {i}");
            assert_eq!(a.span(), b.span(), "span mismatch at {i}");
        }
    }

    #[test]
    fn lossless_retokenize_suffix_matches_full_after_multiline_comment_edit() {
        let interner = Interner::new();
        let file_id = FileId::new(78);

        let old_input = "let a = 1; /* hello world */ let b = 2;";
        let edit_start = old_input
            .find("world")
            .expect("expected substring in old input");
        let new_input = old_input.replacen("world", "wurld", 1);

        let old_stream =
            TokenKind::tokenize_lossless_with_file_id(old_input, &interner, file_id).unwrap();
        let incremental = TokenKind::retokenize_lossless_suffix_from_covering_token_start(
            &old_stream,
            &new_input,
            file_id,
            edit_start,
        )
        .unwrap();
        let full =
            TokenKind::tokenize_lossless_with_file_id(&new_input, &interner, file_id).unwrap();

        assert_eq!(incremental.tokens.len(), full.tokens.len());
        for (i, (a, b)) in incremental
            .tokens
            .iter()
            .zip(full.tokens.iter())
            .enumerate()
        {
            assert_eq!(a.kind(), b.kind(), "kind mismatch at {i}");
            assert_eq!(a.span(), b.span(), "span mismatch at {i}");
        }
    }

    #[test]
    fn lossless_retokenize_bounded_window_matches_full_after_newline_insertion() {
        let interner = Interner::new();
        let file_id = FileId::new(79);

        let old_input = r#"fn main() {
    	let s = "hello";
    	let x = 1;
    }
    "#;
        // Insert a newline inside the string literal (single token), shifting line/column for the suffix.
        let edit_start = old_input.find("hello").unwrap();
        let edit_end = edit_start + "hello".len();
        let new_input = old_input.replacen("hello", "he\nllo", 1);

        let old_stream =
            TokenKind::tokenize_lossless_with_file_id(old_input, &interner, file_id).unwrap();
        let incremental = TokenKind::retokenize_lossless_bounded_window_from_covering_token_start(
            &old_stream,
            old_input,
            &new_input,
            file_id,
            edit_start,
            edit_end,
        )
        .unwrap();
        let full =
            TokenKind::tokenize_lossless_with_file_id(&new_input, &interner, file_id).unwrap();

        assert_eq!(incremental.tokens.len(), full.tokens.len());
        for (i, (a, b)) in incremental
            .tokens
            .iter()
            .zip(full.tokens.iter())
            .enumerate()
        {
            assert_eq!(a.kind(), b.kind(), "kind mismatch at {i}");
            assert_eq!(a.span(), b.span(), "span mismatch at {i}");
        }
    }

    #[test]
    fn lossless_retokenize_bounded_window_matches_full_after_comment_edit() {
        let interner = Interner::new();
        let file_id = FileId::new(80);

        let old_input = r#"let a = 1; /* hello
    world */ let b = 2;
    "#;
        let edit_start = old_input.find("world").unwrap();
        let edit_end = edit_start + "world".len();
        let new_input = old_input.replacen("world", "w0rld", 1);

        let old_stream =
            TokenKind::tokenize_lossless_with_file_id(old_input, &interner, file_id).unwrap();
        let incremental = TokenKind::retokenize_lossless_bounded_window_from_covering_token_start(
            &old_stream,
            old_input,
            &new_input,
            file_id,
            edit_start,
            edit_end,
        )
        .unwrap();
        let full =
            TokenKind::tokenize_lossless_with_file_id(&new_input, &interner, file_id).unwrap();

        assert_eq!(incremental.tokens.len(), full.tokens.len());
        for (i, (a, b)) in incremental
            .tokens
            .iter()
            .zip(full.tokens.iter())
            .enumerate()
        {
            assert_eq!(a.kind(), b.kind(), "kind mismatch at {i}");
            assert_eq!(a.span(), b.span(), "span mismatch at {i}");
        }
    }
}
