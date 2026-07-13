/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::{ByteLexerError, ParseBytes};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::ops::Add;

/// A simple byte position tracking only the absolute offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BytePosition {
    /// 0-based byte offset in the input
    pub absolute: usize,
    /// 1-based line number
    pub line: u32,
    /// 1-based column number
    pub column: u32,
}

impl Default for BytePosition {
    fn default() -> Self {
        BytePosition {
            absolute: 0,
            line: 1,
            column: 1,
        }
    }
}

impl Add<usize> for BytePosition {
    type Output = BytePosition;

    fn add(self, rhs: usize) -> Self::Output {
        BytePosition {
            absolute: self.absolute + rhs,
            line: self.line,
            column: self.column + rhs as u32,
        }
    }
}

impl Display for BytePosition {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "byte-offset({})", self.absolute)
    }
}

/// A span covering a portion of the byte input.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    start: BytePosition,
    end: BytePosition,
}

impl ByteSpan {
    pub fn new(start: BytePosition, end: BytePosition) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end.absolute - self.start.absolute
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn start(&self) -> BytePosition {
        self.start
    }

    pub fn end(&self) -> BytePosition {
        self.end
    }

    pub fn is_valid(&self) -> bool {
        self.start.absolute <= self.end.absolute
    }

    /// Merge two spans into one that covers the entire region.
    pub fn merge(self, other: ByteSpan) -> ByteSpan {
        ByteSpan {
            // start: BytePosition {
            //     absolute: self.start.absolute.min(other.start.absolute),
            // },
            // end: BytePosition {
            //     absolute: self.end.absolute.max(other.end.absolute),
            // },
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl Display for ByteSpan {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// A checkpoint allows you to restore the cursor’s position.
#[derive(Debug, Clone, Copy)]
pub struct ByteCheckpoint {
    current_pos: BytePosition,
}

impl ByteCheckpoint {
    pub fn current_pos(&self) -> BytePosition {
        self.current_pos
    }
}

/// A cursor for binary data. It works on a byte slice and tracks the absolute offset.
#[derive(Debug, Clone)]
pub struct ByteCursor<'a> {
    /// The input as a byte slice.
    input: &'a [u8],
    current_pos: BytePosition,
}

impl<'a> Iterator for ByteCursor<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.advance_simple()
    }
}

// impl<'a> IntoIterator for ByteCursor<'a> {
//     type Item = u8;
//     type IntoIter = std::iter::Map<std::slice::Iter<'a, u8>, fn(&u8) -> u8>;
//
//     fn into_iter(self) -> Self::IntoIter {
//         self.input.iter().copied()
//     }
// }

const NEWLINE_BYTES: [u8; 2] = [b'\n', b'\r'];
impl<'a> ByteCursor<'a> {
    /// Create a new ByteCursor from a byte slice.
    pub fn new(input: &'a [u8]) -> Self {
        ByteCursor {
            input,
            current_pos: BytePosition::default(),
        }
    }

    /// Get the current byte offset.
    pub fn current_pos(&self) -> BytePosition {
        self.current_pos
    }

    /// Check if the cursor has reached the end of input.
    pub fn is_eof(&self) -> bool {
        self.current_pos.absolute >= self.input.len()
    }

    /// Peek at the next byte without advancing.
    pub fn peek(&self) -> Option<u8> {
        self.input.get(self.current_pos.absolute).copied()
        // self.remaining().get(0).copied()
    }

    /// Peek N bytes without advancing
    pub fn peek_n(&self, n: usize) -> Option<&'a [u8]> {
        let rem = self.remaining();
        if rem.len() >= n {
            Some(&rem[..n])
        } else {
            None
        }
    }

    /// Advance the cursor by one byte and return that byte.
    pub fn advance_simple(&mut self) -> Option<u8> {
        if self.is_eof() {
            None
        } else {
            let byte = self.input[self.current_pos.absolute];
            self.current_pos.absolute += 1;
            Some(byte)
        }
    }

    pub fn advance(&mut self) -> Option<u8> {
        if self.is_eof() {
            return None;
        }
        let b = self.input[self.current_pos.absolute];

        // Check for \r (with optional following \n)
        if b == b'\r' {
            let next = self.input.get(self.current_pos.absolute + 1).copied();
            if next == Some(b'\n') {
                self.current_pos.absolute += 2;
            } else {
                self.current_pos.absolute += 1;
            }
            self.current_pos.line += 1;
            self.current_pos.column = 1;
            return Some(b'\n'); // Normalize to \n if desired
        }

        self.current_pos.absolute += 1;
        if b == b'\n' || NEWLINE_BYTES.contains(&b) {
            self.current_pos.line += 1;
            self.current_pos.column = 1;
        } else {
            self.current_pos.column += 1;
        }
        Some(b)
    }

    /// Advance by N bytes (or error if EOF)
    pub fn advance_by(&mut self, n: usize) -> Result<&[u8], ByteLexerError> {
        if self.current_pos.absolute + n > self.input.len() {
            return Err(ByteLexerError::UnexpectedEof {
                expected: format!("{} more bytes", n),
                span: ByteSpan::new(self.current_pos(), self.current_pos()),
            });
        }
        let start = self.current_pos.absolute;
        self.current_pos.absolute += n;
        Ok(&self.input[start..self.current_pos.absolute])
    }

    /// Create a checkpoint to later restore the cursor.
    pub fn checkpoint(&self) -> ByteCheckpoint {
        ByteCheckpoint {
            current_pos: self.current_pos,
        }
    }

    /// Restore the cursor to a previous checkpoint.
    pub fn restore(&mut self, checkpoint: ByteCheckpoint) {
        assert!(checkpoint.current_pos.absolute <= self.input.len());
        self.current_pos = checkpoint.current_pos;
    }

    /// Remaining bytes
    pub fn remaining(&self) -> &'a [u8] {
        &self.input[self.current_pos.absolute..]
    }

    /// Return a slice of the input from the start to the end positions.
    pub fn slice(&self, start: BytePosition, end: BytePosition) -> &'a [u8] {
        &self.input[start.absolute..end.absolute]
    }

    /// Return a slice from a starting position to the end of input.
    pub fn slice_from(&self, start: BytePosition) -> &'a [u8] {
        &self.input[start.absolute..]
    }

    /// Return a span covering the bytes from the given checkpoint to the current position.
    pub fn span_since(&self, checkpoint: ByteCheckpoint) -> ByteSpan {
        ByteSpan::new(checkpoint.current_pos(), self.current_pos)
    }

    pub fn current_span(&self) -> ByteSpan {
        ByteSpan::new(self.current_pos, self.current_pos)
    }

    /// Consume the expected byte sequence.
    pub fn consume(&mut self, expected: &[u8]) -> Result<ByteSpan, ByteLexerError> {
        if expected.is_empty() {
            return Err(ByteLexerError::EmptyExpectedString {
                span: self.current_span(),
            });
        }
        let checkpoint = self.checkpoint();
        for &b in expected {
            self.consume_byte(b)?;
            // if let Err(e) = self.consume_byte(b) {
            //     self.restore(checkpoint);
            //     return Err(e);
            // }
        }
        Ok(self.span_since(checkpoint))
    }

    /// Consume a specific byte.
    pub fn consume_byte(&mut self, expected: u8) -> Result<ByteSpan, ByteLexerError> {
        let checkpoint = self.checkpoint();
        match self.peek() {
            Some(b) if b == expected => {
                self.advance();
                Ok(self.span_since(checkpoint))
            }
            Some(b) => {
                let end = checkpoint.current_pos + 1;
                self.restore(checkpoint);
                Err(ByteLexerError::UnexpectedByte {
                    expected: format!("byte {}", expected),
                    found: b,
                    span: ByteSpan::new(checkpoint.current_pos, end),
                })
            }
            None => Err(ByteLexerError::UnexpectedEof {
                expected: format!("byte {}", expected),
                span: ByteSpan::new(checkpoint.current_pos, self.current_pos),
            }),
        }
    }

    /// Consume bytes while a predicate holds.
    ///
    /// For example, to consume digits you could call:
    /// `cursor.consume_while(|b| (b'0'..=b'9').contains(&b))`
    pub fn consume_while<F>(&mut self, mut predicate: F) -> &'a [u8]
    where
        F: FnMut(u8) -> bool,
    {
        let checkpoint = self.checkpoint();
        while let Some(b) = self.peek() {
            if !predicate(b) {
                break;
            }
            self.advance();
        }
        self.slice(checkpoint.current_pos(), self.current_pos())
    }

    /// Consume *at least* `min_bytes`, and optionally up to `max_bytes`, while `predicate` is true.
    pub fn consume_while_m_n<F>(
        &mut self,
        min_bytes: usize,
        max_bytes: Option<usize>,
        predicate: F,
    ) -> Result<&'a [u8], ByteLexerError>
    where
        F: Fn(u8) -> bool,
    {
        let start = self.checkpoint();
        let mut count = 0;
        while let Some(&b) = self.remaining().first() {
            if let Some(m) = max_bytes {
                if count >= m {
                    break;
                }
            }
            if !predicate(b) {
                break;
            }
            self.current_pos.absolute += 1;
            count += 1;
        }
        if count < min_bytes {
            self.restore(start);
            return Err(ByteLexerError::InvalidLength {
                found: count,
                min: min_bytes,
                max: max_bytes.unwrap_or(count),
                span: self.span_since(start),
            });
        }
        Ok(self.slice(start.current_pos(), self.current_pos()))
    }

    /// A helper for “verify” logic that does not advance the cursor
    pub fn verify<F>(&mut self, predicate: F) -> Result<ByteSpan, ByteLexerError>
    where
        F: Fn(u8) -> bool,
    {
        let checkpoint = self.checkpoint();
        let consumed = self.consume_while(predicate);
        let span = self.span_since(checkpoint);
        self.restore(checkpoint);
        if consumed.is_empty() {
            return Err(ByteLexerError::InvalidLength {
                found: 0,
                min: 1,
                max: 1,
                span,
            });
        }
        Ok(span)
    }

    /// Parse an instance of `T: ParseBytes<'a>`.
    /// If parsing fails, revert to the checkpoint.
    pub fn parse<T: ParseBytes<'a>>(&mut self) -> Result<T, ByteLexerError> {
        let checkpoint = self.checkpoint();
        match T::parse(self) {
            Ok(t) => Ok(t),
            Err(e) => {
                self.restore(checkpoint);
                Err(e)
            }
        }
    }

    /// Parse with span tracking.
    pub fn parse_with_span<T: ParseBytes<'a>>(&mut self) -> Result<(T, ByteSpan), ByteLexerError> {
        let checkpoint = self.checkpoint();
        let value = T::parse(self)?;
        Ok((value, self.span_since(checkpoint)))
    }

    /// Parse exactly and then check that we are at the end of input.
    pub fn parse_exact<T: ParseBytes<'a>>(&mut self) -> Result<T, ByteLexerError> {
        let res = T::parse(self);
        if !self.is_eof() {
            return Err(ByteLexerError::UnexpectedByte {
                expected: "end of input".to_string(),
                found: self.peek().unwrap_or(0),
                span: ByteSpan::new(self.current_pos, self.current_pos),
            });
        }
        res
    }

    /// Example: parse until `terminator` is encountered, returning all consumed bytes.
    /// This is analogous to `advance_until` for strings, but for bytes.
    pub fn advance_until(&mut self, terminator: &[u8]) -> Result<&'a [u8], ByteLexerError> {
        let start_offset = self.current_pos().absolute;
        let term_len = terminator.len();

        while !self.remaining().starts_with(terminator) {
            if self.advance().is_none() {
                return Err(ByteLexerError::UnexpectedEof {
                    expected: format!("terminator {:?}", terminator),
                    span: ByteSpan::new(self.current_pos(), self.current_pos()),
                });
            }
        }

        let end_offset = self.current_pos().absolute;
        self.advance_by(term_len)?;
        Ok(&self.input[start_offset..end_offset])
    }

    /// until_b4: Repeatedly parse T until encountering U (which is not consumed).
    /// Returns (Vec<T>, ByteSpan) for the range of items.
    pub fn until_b4<T, U>(&mut self) -> Result<(Vec<T>, ByteSpan), ByteLexerError>
    where
        T: ParseBytes<'a>,
        U: ParseBytes<'a>,
    {
        let start = self.checkpoint();
        let mut content_checkpoint = start;

        let mut items = Vec::new();

        loop {
            let checkpoint = self.checkpoint();
            match U::parse(self) {
                Ok(_) => {
                    let span = ByteSpan::new(start.current_pos(), content_checkpoint.current_pos());
                    return Ok((items, span));
                }
                Err(_) => {
                    self.restore(checkpoint);
                    match T::parse(self) {
                        Ok(item) => {
                            content_checkpoint = self.checkpoint();
                            items.push(item);
                        }
                        Err(e) => {
                            // If no progress, advance 1 byte to avoid infinite loop
                            if self.current_pos().absolute == checkpoint.current_pos().absolute {
                                self.advance();
                            }
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    /// Skip whitespace bytes (space, tab, newline).
    /// Returns the number of bytes skipped.
    pub fn skip_whitespace(&mut self) -> usize {
        let start = self.current_pos.absolute;
        // Here, whitespace is defined as space, tab, newline, and carriage return.
        // self.consume_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
        self.consume_while(|b| b.is_ascii_whitespace());
        self.current_pos.absolute - start
    }

    /// Execute a parser function with span tracking.
    pub fn parse_with<F, T>(&mut self, mut parser: F) -> Result<(T, ByteSpan), ByteLexerError>
    where
        F: FnMut(&mut Self) -> Result<T, ByteLexerError>,
    {
        let checkpoint = self.checkpoint();
        let value = parser(self)?;
        let span = self.span_since(checkpoint);
        Ok((value, span))
    }

    pub fn reset_dangerous(&mut self) {
        self.current_pos = BytePosition::default();
    }
}
