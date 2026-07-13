/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */

use crate::chars::{CharLexerError, ParseChars};

use super::core::CharCursor;
use super::types::{Checkpoint, Span};

impl<'a> CharCursor<'a> {
    /// Parses elements of type T until encountering terminator U
    /// Returns parsed items and span covering all parsed content excluding the terminator
    pub fn until_b4<T, U>(&mut self) -> Result<(Vec<T>, Span), CharLexerError>
    where
        T: ParseChars,
        U: ParseChars,
    {
        let start = self.checkpoint();
        let mut content_checkpoint = start;
        let mut items = Vec::new();

        loop {
            let checkpoint = self.checkpoint();
            match U::parse(self) {
                Ok(_) => {
                    let span = Span::new_with_file_id(
                        start.current_pos(),
                        content_checkpoint.current_pos(),
                        self.file_id,
                    );
                    return Ok((items, span));
                }
                Err(_) => {
                    self.restore(checkpoint);
                    match T::parse(self) {
                        Ok(item) => {
                            content_checkpoint = self.checkpoint();
                            items.push(item)
                        }
                        Err(e) => {
                            // advance to prevent infinite loop, if no progress
                            if self.position().absolute == checkpoint.current_pos().absolute {
                                self.advance();
                            }
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    pub fn until_b4_str(&mut self, terminator: &str) -> Result<(&'a str, Span), CharLexerError> {
        let start = self.checkpoint();

        loop {
            if self.verify(terminator).is_ok() {
                let span = self.span_since(start);
                return Ok((span.as_slice(self), span));
            }

            if self.advance().is_none() {
                return Err(CharLexerError::UnterminatedString {
                    span: Span::new_with_file_id(self.position(), self.position(), self.file_id),
                });
            }
        }
    }

    pub fn parse_until_inclusive<T, U>(&mut self) -> Result<(Vec<T>, Span), CharLexerError>
    where
        T: ParseChars,
        U: ParseChars,
    {
        todo!()
    }

    pub fn _dont_use_parse_until_to_be_deleted<T, U>(
        &mut self,
    ) -> Result<(Vec<T>, Span), CharLexerError>
    where
        T: ParseChars,
        U: ParseChars,
    {
        let start = self.checkpoint();
        let mut items = Vec::new();

        loop {
            let checkpoint = self.checkpoint();
            match T::parse(self) {
                Ok(item) => items.push(item),
                Err(e1) => match U::parse(self) {
                    Ok(_) => {
                        let span = self.span_since(start);
                        return Ok((items, span));
                    }
                    Err(e2) => {
                        // advance to prevent infinite loop, if no progress
                        if self.position().absolute == checkpoint.current_pos().absolute {
                            self.advance();
                        }
                        return Err(e1.merge(&e2));
                    }
                },
            }
        }
    }

    /// Specialized version for parsing individual characters until terminator
    pub fn parse_chars_until<U>(&mut self) -> Result<(String, Span), CharLexerError>
    where
        U: ParseChars,
    {
        let start = self.checkpoint();
        let mut buffer = String::new();

        while !self.is_eof() {
            let checkpoint = self.checkpoint();
            match U::parse(self) {
                Ok(_) => {
                    let span = self.span_since(start);
                    return Ok((buffer, span));
                }
                Err(_) => {
                    self.restore(checkpoint);
                    if let Some(c) = self.advance() {
                        buffer.push(c);
                    }
                }
            }
        }

        Err(CharLexerError::UnexpectedEof {
            expected: format!("terminator {}", std::any::type_name::<U>()),
            span: self.span_since(start),
        })
    }

    /// Consume characters while predicate is true
    pub fn consume_while<F>(&mut self, predicate: F) -> &'a str
    where
        F: FnMut(char) -> bool,
    {
        self.consume_while_base(predicate, None, None, None)
            .unwrap_or_default()
    }

    pub fn consume_while_span<F>(&mut self, predicate: F) -> Span
    where
        F: FnMut(char) -> bool,
    {
        let start = self.checkpoint();
        self.consume_while(predicate);
        self.span_since(start)
    }

    /// Consume between min and max characters (inclusive)
    pub fn consume_while_m_n<F>(
        &mut self,
        min_chars: usize,
        max_chars: usize,
        predicate: F,
    ) -> Result<&'a str, CharLexerError>
    where
        F: FnMut(char) -> bool,
    {
        self.consume_while_base(predicate, Some(min_chars), Some(max_chars), None)
    }

    /// Consume at least N characters
    pub fn consume_while_m<F>(
        &mut self,
        min_chars: usize,
        predicate: F,
    ) -> Result<&'a str, CharLexerError>
    where
        F: FnMut(char) -> bool,
    {
        self.consume_while_base(predicate, Some(min_chars), None, None)
    }

    pub fn consume_while_m_span<F>(
        &mut self,
        min_chars: usize,
        predicate: F,
    ) -> Result<Span, CharLexerError>
    where
        F: FnMut(char) -> bool,
    {
        let start = self.checkpoint();
        self.consume_while_m(min_chars, predicate)?;
        Ok(self.span_since(start))
    }

    /// Consume characters with byte length limit
    pub fn consume_while_with_byte_limit<F>(&mut self, max_bytes: usize, predicate: F) -> &'a str
    where
        F: FnMut(char) -> bool,
    {
        self.consume_while_base(predicate, None, None, Some(max_bytes))
            .unwrap_or_default()
    }

    /// Consume exactly N characters
    pub fn consume_exact<F>(
        &mut self,
        count: usize,
        predicate: F,
    ) -> Result<&'a str, CharLexerError>
    where
        F: FnMut(char) -> bool,
    {
        self.consume_while_base(predicate, Some(count), Some(count), None)
    }

    fn consume_while_base<F>(
        &mut self,
        mut predicate: F,
        min_chars: Option<usize>,
        max_chars: Option<usize>,
        max_bytes: Option<usize>,
    ) -> Result<&'a str, CharLexerError>
    where
        F: FnMut(char) -> bool,
    {
        let start = self.checkpoint();
        let mut char_count = 0;
        let mut bytes_consumed = 0;

        while let Some(c) = self.peek() {
            let byte_len = c.len_utf8();
            if max_bytes.is_some_and(|max| bytes_consumed + byte_len > max) {
                break;
            }

            if max_chars.is_some_and(|max| char_count >= max) {
                break;
            }

            if !predicate(c) {
                break;
            }

            char_count += 1;
            bytes_consumed += byte_len;
            self.advance();
        }

        if let Some(min) = min_chars {
            if char_count < min {
                self.restore(start);
                return Err(CharLexerError::InvalidLength {
                    found: char_count,
                    min,
                    max: max_chars.unwrap_or(char_count),
                    span: self.span_since(start),
                });
            }
        }

        Ok(self.slice(start.current_pos(), self.position()))
    }

    /// Checks string but does not consume or advance
    pub fn verify(&mut self, expected: &str) -> Result<Span, CharLexerError> {
        let start = self.checkpoint();
        let result = match self.consume(expected) {
            Ok(_) => {
                let span = self.span_since(start);
                Ok(span)
            }
            Err(e) => Err(e),
        };
        self.restore(start);
        result
    }

    /// Same as consume_exact but does not consume characters
    pub fn verify_exact(
        &mut self,
        count: usize,
        predicate: impl Fn(char) -> bool,
    ) -> Result<Span, CharLexerError> {
        let start = self.checkpoint();
        self.consume_exact(count, predicate)?;
        let span = self.span_since(start);
        self.restore(start);
        Ok(span)
    }

    /// Same as consume_while but does not consume characters
    pub fn verify_while(
        &mut self,
        predicate: impl Fn(char) -> bool,
    ) -> Result<Span, CharLexerError> {
        let start = self.checkpoint();
        self.consume_while(predicate);
        let span = self.span_since(start);
        self.restore(start);
        Ok(span)
    }

    /// Same as consume_while_m but does not consume characters
    pub fn verify_while_m(
        &mut self,
        min_chars: usize,
        predicate: impl Fn(char) -> bool,
    ) -> Result<Span, CharLexerError> {
        let start = self.checkpoint();
        self.consume_while_m(min_chars, predicate)?;
        let span = self.span_since(start);
        self.restore(start);
        Ok(span)
    }

    /// Same as consume_while_m_n but does not consume characters
    pub fn verify_while_m_n(
        &mut self,
        min_chars: usize,
        max_chars: usize,
        predicate: impl Fn(char) -> bool,
    ) -> Result<Span, CharLexerError> {
        let start = self.checkpoint();
        self.consume_while_m_n(min_chars, max_chars, predicate)?;
        let span = self.span_since(start);
        self.restore(start);
        Ok(span)
    }

    /// Same as consume_while_with_byte_limit but does not consume characters
    pub fn verify_while_with_byte_limit(
        &mut self,
        max_bytes: usize,
        predicate: impl Fn(char) -> bool,
    ) -> Result<Span, CharLexerError> {
        let start = self.checkpoint();
        self.consume_while_with_byte_limit(max_bytes, predicate);
        let span = self.span_since(start);
        self.restore(start);
        Ok(span)
    }

    pub fn repeat_until<T, U>(&mut self) -> Result<Vec<T>, CharLexerError>
    where
        T: ParseChars,
        U: ParseChars,
    {
        let reps = self.repeat_until_span::<T, U>()?;
        Ok(reps.0)
    }

    pub fn repeat_until_span<T, U>(&mut self) -> Result<(Vec<T>, Span), CharLexerError>
    where
        T: ParseChars,
        U: ParseChars,
    {
        let start = self.checkpoint();
        let mut items = Vec::new();
        loop {
            let checkpoint = self.checkpoint();
            match (T::parse(self), U::parse(self)) {
                (Ok(item), _) => items.push(item),
                (_, Ok(_terminator)) => {
                    let span = self.span_since(start);
                    return Ok((items, span));
                }
                (Err(e1), Err(e2)) => {
                    if self.position().absolute == checkpoint.current_pos().absolute {
                        self.advance();
                    }
                    return Err(e1.merge(&e2));
                }
            }
        }
    }

    pub fn repeat_until2<T: ParseChars, U: ParseChars>(
        &mut self,
    ) -> Result<Vec<T>, CharLexerError> {
        let mut items = Vec::new();
        loop {
            let checkpoint = self.checkpoint();
            match T::parse(self) {
                Ok(item) => items.push(item),
                Err(e1) => {
                    match U::parse(self) {
                        Ok(_) => break Ok(items),
                        Err(e2) => {
                            // If no progress made, advance by 1 char to prevent infinite loop
                            if self.position().absolute == checkpoint.current_pos().absolute {
                                self.advance();
                            }
                            return Err(e1.merge(&e2));
                        }
                    }
                }
            }
        }
    }

    pub fn recover_until<F>(&mut self, predicate: F) -> &'a str
    where
        F: Fn(char) -> bool,
    {
        let start = self.position().absolute;
        while let Some(c) = self.peek() {
            if predicate(c) {
                break;
            }
            self.advance();
        }
        &self.input[start..self.position().absolute]
    }

    /// Expect a specific character, returning span-aware error
    pub fn consume_char(&mut self, expected: char) -> Result<Span, CharLexerError> {
        let checkpoint = self.checkpoint();
        match self.peek() {
            Some(c) if c == expected => {
                self.advance();
                let span = self.span_since(checkpoint);
                Ok(span)
            }
            Some(c) => {
                let end = checkpoint.current_pos + c.len_utf8();
                self.restore(checkpoint);
                Err(CharLexerError::UnexpectedChar {
                    expected: expected.to_string(),
                    found: c,
                    span: Span::new_with_file_id(checkpoint.current_pos, end, self.file_id),
                })
            }
            None => {
                self.restore(checkpoint);
                Err(CharLexerError::UnexpectedEof {
                    expected: expected.to_string(),
                    span: Span::new_with_file_id(
                        checkpoint.current_pos,
                        self.current_pos,
                        self.file_id,
                    ),
                })
            }
        }
    }

    pub fn consume(&mut self, expected: &str) -> Result<Span, CharLexerError> {
        if expected.is_empty() {
            return Err(CharLexerError::EmptyExpectedString {
                span: self.current_span(),
            });
        }
        let checkpoint = self.checkpoint();
        for c in expected.chars() {
            if let Err(e) = self.consume_char(c) {
                self.restore(checkpoint);
                return Err(e);
            }
        }

        let span = self.span_since(checkpoint);
        Ok(span)
    }

    pub fn consume_case_insensitive(&mut self, expected: &str) -> Result<Span, CharLexerError> {
        let checkpoint = self.checkpoint();
        for c in expected.chars() {
            let c = c.to_ascii_lowercase();
            let next = self.peek().map(|c| c.to_ascii_lowercase());
            if next != Some(c) {
                self.restore(checkpoint);
                return Err(CharLexerError::UnexpectedChar {
                    expected: expected.to_string(),
                    found: self.peek().unwrap_or_default(),
                    span: self.current_span(),
                });
            }
            self.advance();
        }
        // Check that we're not in the middle of an identifier
        // Allow EOF or any non-alphabetic, non-digit, non-underscore character
        let next_char = self.peek();
        if let Some(c) = next_char {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.restore(checkpoint);
                return Err(CharLexerError::UnexpectedChar {
                    expected: "keyword boundary".to_string(),
                    found: c,
                    span: self.current_span(),
                });
            }
        }

        let span = self.span_since(checkpoint);
        Ok(span)
    }

    pub fn consume_not(&mut self, expected: &str) -> Result<Span, CharLexerError> {
        let checkpoint = self.checkpoint();
        let next = self.consume(expected);

        match next {
            Ok(_) => {
                self.restore(checkpoint);
                Err(CharLexerError::UnexpectedChar {
                    expected: format!("anything but {}", expected),
                    found: self.peek().unwrap_or_default(),
                    span: self.current_span(),
                })
            }
            Err(_) => {
                self.advance();
                Ok(self.span_since(checkpoint))
            }
        }
    }

    pub fn consume_one_of<const T: usize>(
        &mut self,
        choices: [char; T],
    ) -> Result<Span, CharLexerError> {
        for c in choices {
            if let Ok(c) = self.consume_char(c) {
                return Ok(c);
            };
        }

        let found = self.peek().unwrap_or_default();
        Err(CharLexerError::UnexpectedChar {
            expected: format!("one of {:?}", choices),
            found,
            span: self.current_span(),
        })
    }

    pub fn consume_any(&mut self) -> Option<char> {
        self.peek().inspect(|_| {
            self.advance();
        })
    }

    /// whitespace characters (space, tab, newline)
    pub fn require_whitespace(&mut self) -> Result<(), CharLexerError> {
        let start = self.checkpoint();
        let consumed = self.consume_while(char::is_whitespace);
        if consumed.is_empty() {
            Err(CharLexerError::ExpectedWhitespace {
                span: self.span_since(start),
            })
        } else {
            Ok(())
        }
    }

    /// Consume 0+ whitespace characters (space, tab, newline)
    pub fn skip_whitespace(&mut self) {
        self.consume_while(char::is_whitespace);
    }

    /// horizontal space (space, tab)
    pub fn require_horizontal(&mut self) -> Result<(), CharLexerError> {
        let start = self.checkpoint();
        let consumed = self.consume_while(|c| matches!(c, ' ' | '\t'));
        if consumed.is_empty() {
            Err(CharLexerError::ExpectedHorizontalSpace {
                span: self.span_since(start),
            })
        } else {
            Ok(())
        }
    }
    /// Consume 0+ horizontal space (space, tab)
    pub fn skip_horizontal(&mut self) {
        self.consume_while(|c| matches!(c, ' ' | '\t'));
    }

    /// vertical space (newlines)
    pub fn require_vertical(&mut self) -> Result<(), CharLexerError> {
        let start = self.checkpoint();
        let consumed = self.consume_while(|c| matches!(c, '\n' | '\r'));
        if consumed.is_empty() {
            Err(CharLexerError::ExpectedVerticalSpace {
                span: self.span_since(start),
            })
        } else {
            Ok(())
        }
    }

    /// Consume 0+ vertical space (newlines)
    pub fn skip_vertical(&mut self) {
        self.consume_while(|c| matches!(c, '\n' | '\r'));
    }

    /// Require exactly one space character
    pub fn require_space(&mut self) -> Result<(), CharLexerError> {
        let start = self.checkpoint();
        match self.peek() {
            Some(' ') => {
                self.advance();
                Ok(())
            }
            _ => Err(CharLexerError::ExpectedSingleSpace {
                span: self.span_since(start),
            }),
        }
    }

    /// Skip all whitespace and comments (implement comment skipping if needed)
    pub fn skip_whitespace_and_comments(&mut self) -> Result<(), CharLexerError> {
        loop {
            self.skip_whitespace();
            if self.consume("//").is_ok() {
                self.consume_while(|c| c != '\n');
            } else if self.consume("/*").is_ok() {
                let start = self.position();
                while self.consume("*/").is_err() {
                    if self.advance().is_none() {
                        return Err(CharLexerError::UnterminatedComment {
                            span: Span::new_with_file_id(start, self.position(), self.file_id),
                        });
                    }
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Skip horizontal space but preserve newlines
    pub fn skip_indentation(&mut self) {
        self.consume_while(|c| matches!(c, ' ' | '\t'));
    }

    /// Skip horizontal space and require at least one
    pub fn require_indentation(&mut self) -> Result<(), CharLexerError> {
        let start = self.checkpoint();
        self.consume_while(|c| matches!(c, ' ' | '\t'));
        if start.current_pos.absolute == self.current_pos.absolute {
            Err(CharLexerError::ExpectedHorizontalSpace {
                span: self.span_since(start),
            })
        } else {
            Ok(())
        }
    }

    /// Skip any combination of horizontal space and line continuations
    pub fn skip_continuation(&mut self) -> Result<(), CharLexerError> {
        loop {
            self.skip_horizontal();
            if self.consume_line_continuation()? {
                continue;
            }
            break;
        }
        Ok(())
    }

    /// Handle line continuation characters (e.g., backslash + newline)
    fn consume_line_continuation(&mut self) -> Result<bool, CharLexerError> {
        let checkpoint = self.checkpoint();
        if self.peek() == Some('\\') {
            self.advance();
            self.skip_vertical();
            Ok(true)
        } else {
            self.restore(checkpoint);
            Ok(false)
        }
    }
}
