/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */

use crate::chars::CharLexerError;

use super::types::{Checkpoint, FileId, Position, Span};

#[derive(Debug, Clone)]
pub struct CharCursor<'a> {
    /// Input as a string slice (no allocation)
    pub(super) input: &'a str,
    /// Global position of the start of `input` within the original source.
    ///
    /// When lexing a substring window, `current_pos.absolute` tracks the *global* byte offset,
    /// while indexing into `input` uses `current_pos.absolute - base_pos.absolute`.
    pub(super) base_pos: Position,
    pub(super) current_pos: Position,
    pub(super) file_id: FileId,
}

const NEWLINES: [char; 7] = [
    '\n',       // Line Feed (LF)
    '\r',       // Carriage Return (CR)
    '\x0C',     // Form Feed
    '\x0B',     // Vertical Tab
    '\u{0085}', // Next Line (NEL)
    '\u{2028}', // Line Separator
    '\u{2029}', // Paragraph Separator
];

impl<'a> CharCursor<'a> {
    pub fn new(input: &'a str) -> Self {
        CharCursor {
            input,
            base_pos: Position {
                line: 1,
                column: 1,
                absolute: 0,
            },
            current_pos: Position {
                line: 1,
                column: 1,
                absolute: 0,
            },
            file_id: FileId::default(),
        }
    }

    pub fn new_with_file_id(input: &'a str, file_id: FileId) -> Self {
        CharCursor {
            input,
            base_pos: Position {
                line: 1,
                column: 1,
                absolute: 0,
            },
            current_pos: Position {
                line: 1,
                column: 1,
                absolute: 0,
            },
            file_id,
        }
    }

    pub fn new_with_file_id_and_start_pos(
        input: &'a str,
        file_id: FileId,
        start_pos: Position,
    ) -> Self {
        CharCursor {
            input,
            base_pos: start_pos,
            current_pos: start_pos,
            file_id,
        }
    }

    fn local_absolute(&self) -> usize {
        debug_assert!(
            self.current_pos.absolute >= self.base_pos.absolute,
            "current_pos must be >= base_pos"
        );
        self.current_pos.absolute - self.base_pos.absolute
    }

    fn to_local_absolute(&self, global_abs: usize) -> usize {
        debug_assert!(
            global_abs >= self.base_pos.absolute,
            "attempted to index before base_pos"
        );
        global_abs - self.base_pos.absolute
    }

    pub fn file_id(&self) -> FileId {
        self.file_id
    }

    pub fn str_from_span(&self, span: Span) -> &'a str {
        self.slice(span.start, span.end)
    }

    pub fn current_span(&self) -> Span {
        Span::new_with_file_id(self.current_pos, self.current_pos, self.file_id)
    }

    pub fn current_pos(&self) -> Position {
        self.current_pos
    }

    pub fn reset_dangerous(&mut self) {
        self.base_pos = Position::default();
        self.current_pos = Position::default();
    }

    pub fn is_eof(&self) -> bool {
        self.local_absolute() >= self.input.len()
    }

    pub fn remaining(&self) -> &'a str {
        &self.input[self.local_absolute()..]
    }

    pub fn input(&self) -> &'a str {
        self.input
    }

    /// Peek the next character without advancing
    pub fn peek(&self) -> Option<char> {
        self.input[self.local_absolute()..].chars().next()
    }

    pub fn peek_n_bytes(&self, n: usize) -> Option<&'a str> {
        let remaining = self.remaining();
        if remaining.len() >= n {
            Some(&remaining[..n])
        } else {
            None
        }
    }

    pub fn peek_n_char(&self, n: usize) -> Option<&'a str> {
        let remaining = self.remaining();
        let mut count = 0;
        let mut byte_len = 0;

        for c in remaining.chars() {
            if count == n {
                break;
            }
            byte_len += c.len_utf8();
            count += 1;
        }

        if count == n {
            Some(&remaining[..byte_len])
        } else {
            None
        }
    }

    /// Look ahead N characters without advancing
    pub fn lookahead(&self, n: usize) -> Option<char> {
        self.input[self.local_absolute()..].chars().nth(n)
    }

    /// Advance the cursor by one character
    /// Enhanced advance that merges \r\n line endings
    pub fn advance(&mut self) -> Option<char> {
        let remaining = &self.input[self.local_absolute()..];
        let mut chars = remaining.chars();
        let first = chars.next()?;

        // Handle \r and \r\n sequences first
        if first == '\r' {
            let next_char = chars.next();
            self.current_pos.absolute += if next_char == Some('\n') { 2 } else { 1 };
            self.current_pos.line += 1;
            self.current_pos.column = 1;
            return Some('\n');
        }

        let c_len = first.len_utf8();
        self.current_pos.absolute += c_len;

        match first {
            '\n' => {
                self.current_pos.line += 1;
                self.current_pos.column = 1;
            }
            '\t' => {
                self.current_pos.column += 4;
            }
            _ if NEWLINES.contains(&first) => {
                self.current_pos.line += 1;
                self.current_pos.column = 1;
            }
            _ => {
                self.current_pos.column += 1;
            }
        }

        Some(first)
    }

    pub fn advance_by(&mut self, n: usize) -> Result<&str, CharLexerError> {
        let checkpoint = self.checkpoint();
        for _ in 0..n {
            if self.is_eof() {
                return Err(CharLexerError::UnexpectedEof {
                    expected: "more characters".to_string(),
                    span: Span::new_with_file_id(self.current_pos, self.current_pos, self.file_id),
                });
            }
            self.advance();
        }
        Ok(self.slice(checkpoint.current_pos(), self.current_pos))
    }

    /// Advance until sequence is found, returns consumed string
    pub fn advance_until(&mut self, sequence: &str) -> Result<&'a str, CharLexerError> {
        let start = self.position().absolute;
        let seq_len = sequence.len();

        while !self.remaining().starts_with(sequence) {
            if self.advance().is_none() {
                return Err(CharLexerError::UnterminatedString {
                    span: Span::new_with_file_id(self.position(), self.position(), self.file_id),
                });
            }
        }

        let end = self.position().absolute;
        self.advance_by(seq_len)?;
        let start_local = self.to_local_absolute(start);
        let end_local = self.to_local_absolute(end);
        Ok(&self.input()[start_local..end_local])
    }

    /// Get current position information
    pub fn position(&self) -> Position {
        self.current_pos
    }

    /// Create a restoration checkpoint
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            current_pos: self.current_pos,
        }
    }

    /// Restore cursor to previous checkpoint
    pub fn restore(&mut self, checkpoint: Checkpoint) {
        let local = self.to_local_absolute(checkpoint.current_pos.absolute);
        assert!(local <= self.input.len());
        self.current_pos = checkpoint.current_pos;
    }

    pub fn slice(&self, start: Position, end: Position) -> &'a str {
        let start_local = self.to_local_absolute(start.absolute);
        let end_local = self.to_local_absolute(end.absolute);
        &self.input[start_local..end_local]
    }

    pub fn slice_with_span(&self, span: Span) -> &'a str {
        self.slice(span.start, span.end)
    }

    pub fn slice_from(&self, start: Position) -> &'a str {
        let start_local = self.to_local_absolute(start.absolute);
        &self.input[start_local..]
    }

    pub fn span_since(&self, checkpoint: Checkpoint) -> Span {
        Span::new_with_file_id(checkpoint.current_pos(), self.current_pos(), self.file_id)
    }

    pub fn consume_space0(&mut self) -> usize {
        let count = self.consume_while(char::is_whitespace);
        count.len()
    }
}

impl Span {
    pub fn from_cursor(cursor: &CharCursor<'_>) -> Self {
        Span {
            start: cursor.current_pos,
            end: cursor.current_pos,
            file_id: cursor.file_id,
        }
    }

    pub fn as_slice<'a>(&self, cursor: &CharCursor<'a>) -> &'a str {
        cursor.slice(self.start, self.end)
    }
}
