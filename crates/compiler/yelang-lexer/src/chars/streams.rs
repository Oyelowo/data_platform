/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 27/01/2025
 */
use super::{CharCursor, ParseCharStream};
use std::io::{self, Read};
use std::str;

// +-----------------+
// |  Stream         |
// |  (I/O Boundary) |
// +--------+--------+
//          |
//          | Buffers chunks
//          v
// +--------+--------+
// |  Cursor         |
// |  (Parse Engine) |
// +-----------------+
/// A stream that reads characters from an I/O source (e.g., files, stdin).
pub struct CharStream<R: Read> {
    reader: R,
    buffer: String, // UTF-8 safe buffer
    position: usize,
}

impl<R: Read> CharStream<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: String::new(),
            position: 0,
        }
    }

    /// Cant handle backtracking/rewinding/restoration since it reads from a stream
    /// - Use with caution
    pub fn parse<T: ParseCharStream>(&mut self) -> io::Result<T> {
        T::parse(self)
    }

    /// Convert entire stream to cursor (for small streams)
    pub fn into_cursor(mut self) -> io::Result<CharCursor<'static>> {
        let mut buffer = String::new();
        while let Ok(chunk) = self.read_next_chunk(1024) {
            buffer.push_str(chunk);
        }
        Ok(CharCursor::new(buffer.leak())) // 'static lifetime
    }

    /// Reads the next chunk into the buffer, ensuring valid UTF-8.
    pub fn read_next_chunk(&mut self, size: usize) -> io::Result<&str> {
        let mut byte_buf = vec![0; size];
        let bytes_read = self.reader.read(&mut byte_buf)?;
        self.buffer.clear();
        self.buffer
            .push_str(str::from_utf8(&byte_buf[..bytes_read]).unwrap_or(""));

        self.position = 0;
        Ok(&self.buffer)
    }

    /// Peeks at the next character without advancing.
    pub fn peek(&mut self) -> io::Result<Option<char>> {
        if self.position >= self.buffer.len() {
            self.read_next_chunk(1024)?;
        }
        Ok(self.buffer[self.position..].chars().next())
    }

    /// Peeks ahead `n` characters.
    pub fn peek_n(&mut self, n: usize) -> io::Result<Option<&str>> {
        let mut iter = self.buffer[self.position..].char_indices();
        let mut end_idx = self.position;

        for _ in 0..n {
            if let Some((idx, _)) = iter.next() {
                end_idx = self.position + idx;
            } else {
                return Ok(None);
            }
        }
        Ok(Some(&self.buffer[self.position..end_idx]))
    }

    /// Advances the cursor by one character and returns it.
    pub fn advance(&mut self) -> io::Result<Option<char>> {
        let mut chars = self.buffer[self.position..].char_indices();
        if let Some((idx, ch)) = chars.next() {
            self.position += idx + ch.len_utf8();
            Ok(Some(ch))
        } else {
            Ok(None)
        }
    }

    /// Consumes characters while the predicate is true.
    pub fn consume_while<F>(&mut self, predicate: F) -> io::Result<String>
    where
        F: Fn(char) -> bool,
    {
        let mut result = String::new();
        while let Some(ch) = self.peek()? {
            if !predicate(ch) {
                break;
            }
            result.push(self.advance()?.unwrap());
        }
        Ok(result)
    }

    /// WARNING: This function is dangerous and may panic if the cursor is not at the end of the stream.
    /// Use with extreme caution.
    /// Only use in cursor with known data
    /// May struggle with
    /// Problematic for streams: let version = cursor.consume_exact(5, |c| c.is_ascii_digit())?; // "1.2.3"
    ///
    /// Limitation: This function is not safe to use with streams that contain multi-byte characters.
    /// - Requires precise character counting across chunk boundaries
    /// - Difficult to handle partial matches that span multiple reads
    pub fn consume_exact_dangerous(&mut self, n: usize) -> io::Result<String> {
        let mut result = String::with_capacity(n);
        while result.len() < n {
            if self.position >= self.buffer.len() {
                self.read_next_chunk(n - result.len())?;
            }
            let remaining = n - result.len();
            let available = &self.buffer[self.position..];
            let take = available.chars().take(remaining);
            result.extend(take);
            self.position += available
                .chars()
                .take(remaining)
                .map(|c| c.len_utf8())
                .sum::<usize>();
        }
        Ok(result)
    }
}
