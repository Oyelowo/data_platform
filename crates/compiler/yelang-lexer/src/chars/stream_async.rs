/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 27/01/2025
 */
use std::str;
use tokio::io::{self, AsyncRead, AsyncReadExt};

/// An asynchronous character stream reading from `AsyncRead`.
pub struct AsyncCharStream<R: AsyncRead + Unpin> {
    reader: R,
    buffer: String, // UTF-8 safe buffer
    position: usize,
}

impl<R> AsyncCharStream<R>
where
    R: AsyncRead + Unpin,
{
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: String::new(),
            position: 0,
        }
    }

    /// Reads the next chunk into the buffer, ensuring valid UTF-8.
    pub async fn read_next_chunk(&mut self, size: usize) -> io::Result<&str> {
        let mut byte_buf = vec![0; size];
        let bytes_read = self.reader.read(&mut byte_buf).await?;
        self.buffer.clear();
        self.buffer
            .push_str(str::from_utf8(&byte_buf[..bytes_read]).unwrap_or(""));

        self.position = 0;
        Ok(&self.buffer)
    }

    /// Peeks at the next character without advancing.
    pub async fn peek(&mut self) -> io::Result<Option<char>> {
        if self.position >= self.buffer.len() {
            self.read_next_chunk(1024).await?;
        }
        Ok(self.buffer[self.position..].chars().next())
    }

    /// Peeks ahead `n` characters.
    pub async fn peek_n(&mut self, n: usize) -> io::Result<Option<&str>> {
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
    pub async fn advance(&mut self) -> io::Result<Option<char>> {
        let mut chars = self.buffer[self.position..].char_indices();
        if let Some((idx, ch)) = chars.next() {
            self.position += idx + ch.len_utf8();
            Ok(Some(ch))
        } else {
            Ok(None)
        }
    }

    /// Consumes characters while the predicate is true.
    pub async fn consume_while<F>(&mut self, predicate: F) -> io::Result<String>
    where
        F: Fn(char) -> bool,
    {
        let mut result = String::new();
        while let Some(ch) = self.peek().await? {
            if !predicate(ch) {
                break;
            }
            result.push(self.advance().await?.unwrap());
        }
        Ok(result)
    }

    async fn fill_buffer(&mut self, min_chars: usize) -> io::Result<()> {
        while self.buffer[self.position..].chars().count() < min_chars {
            self.read_next_chunk(1024).await?;
        }
        Ok(())
    }
}
