/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */

use std::io::{self, Read};
use tokio::io::{AsyncRead, AsyncReadExt};

/// A stream that reads bytes from an I/O source (e.g. files).
pub struct ByteStream<R: Read> {
    reader: R,
    buffer: Vec<u8>,
    position: usize,
}

impl<R: Read> ByteStream<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            position: 0,
        }
    }

    /// Reads the next chunk of bytes into the buffer.
    pub fn read_next_chunk(&mut self, size: usize) -> io::Result<&[u8]> {
        self.buffer.clear();
        let mut chunk = vec![0; size];
        let bytes_read = self.reader.read(&mut chunk)?;
        self.buffer.extend_from_slice(&chunk[..bytes_read]);
        self.position = 0;
        Ok(&self.buffer)
    }

    /// Peeks at the next byte without advancing.
    pub fn peek(&mut self) -> io::Result<Option<u8>> {
        if self.position >= self.buffer.len() {
            // Read next chunk if buffer is empty.
            self.read_next_chunk(1024)?;
        }
        Ok(self.buffer.get(self.position).copied())
    }

    /// Advances the cursor by one byte and returns it.
    pub fn advance(&mut self) -> io::Result<Option<u8>> {
        let byte = self.peek()?;
        if byte.is_some() {
            self.position += 1;
        }
        Ok(byte)
    }

    /// Consume bytes while the predicate is true.
    pub fn consume_while<F>(&mut self, predicate: F) -> io::Result<Vec<u8>>
    where
        F: Fn(u8) -> bool,
    {
        let mut result = Vec::new();
        while let Some(byte) = self.peek()? {
            if !predicate(byte) {
                break;
            }
            result.push(self.advance()?.unwrap());
        }
        Ok(result)
    }

    /// Seeks to a specific offset.
    pub fn seek(&mut self, offset: usize) -> io::Result<()> {
        if offset < self.buffer.len() {
            self.position = offset;
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Seek out of bounds",
            ))
        }
    }

    /// Moves the cursor back by `steps` bytes.
    pub fn backtrack(&mut self, steps: usize) -> io::Result<()> {
        if self.position >= steps {
            self.position -= steps;
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Backtrack out of bounds",
            ))
        }
    }
}

/// An asynchronous byte stream that reads from an `AsyncRead` source.
pub struct AsyncByteStream<R: AsyncRead + Unpin> {
    reader: R,
    buffer: Vec<u8>,
    position: usize,
}

impl<R: AsyncRead + Unpin> AsyncByteStream<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            position: 0,
        }
    }

    /// Reads the next chunk of bytes into the buffer.
    pub async fn read_next_chunk(&mut self, size: usize) -> io::Result<&[u8]> {
        self.buffer.clear();
        let mut chunk = vec![0; size];
        let bytes_read = self.reader.read(&mut chunk).await?;
        self.buffer.extend_from_slice(&chunk[..bytes_read]);
        self.position = 0;
        Ok(&self.buffer)
    }

    /// Peeks at the next byte without advancing.
    pub async fn peek(&mut self) -> io::Result<Option<u8>> {
        if self.position >= self.buffer.len() {
            self.read_next_chunk(1024).await?; // Read next chunk if buffer is empty.
        }
        Ok(self.buffer.get(self.position).copied())
    }

    /// Advances the cursor by one byte and returns it.
    pub async fn advance(&mut self) -> io::Result<Option<u8>> {
        let byte = self.peek().await?;
        if byte.is_some() {
            self.position += 1;
        }
        Ok(byte)
    }

    /// Consume bytes while the predicate is true.
    pub async fn consume_while<F>(&mut self, predicate: F) -> io::Result<Vec<u8>>
    where
        F: Fn(u8) -> bool,
    {
        let mut result = Vec::new();
        while let Some(byte) = self.peek().await? {
            if !predicate(byte) {
                break;
            }
            result.push(self.advance().await?.unwrap());
        }
        Ok(result)
    }

    /// Seeks to a specific offset.
    pub async fn seek(&mut self, offset: usize) -> io::Result<()> {
        if offset < self.buffer.len() {
            self.position = offset;
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Seek out of bounds",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    // use super::*;
    // use std::fs::File;
    // use std::io::BufReader;
    //
    // fn main() -> std::io::Result<()> {
    //     let file = File::open("test.bin")?;
    //     let mut stream = ByteStream::new(BufReader::new(file));
    //
    //     println!("Reading next chunk...");
    //     let data = stream.read_next_chunk(256)?;
    //     println!("Bytes read: {:?}", data);
    //
    //     println!("Peeking at the next byte: {:?}", stream.peek()?);
    //     println!("Advancing cursor...");
    //     stream.advance()?;
    //
    //     println!("Seeking to byte 10...");
    //     stream.seek(10)?;
    //
    //     println!("Backtracking 5 bytes...");
    //     stream.backtrack(5)?;
    //
    //     Ok(())
    // }
    //
    //
    //
    //
    //
    //
    // use tokio::fs::File;
    // use tokio::io::BufReader;
    // use tokio::runtime::Runtime;
    //
    // #[tokio::main]
    // async fn main2() -> std::io::Result<()> {
    //     let file = File::open("test.bin").await?;
    //     let mut stream = AsyncByteStream::new(BufReader::new(file));
    //
    //     println!("Reading next chunk...");
    //     let data = stream.read_next_chunk(256).await?;
    //     println!("Bytes read: {:?}", data);
    //
    //     println!("Peeking at the next byte: {:?}", stream.peek().await?);
    //     println!("Advancing cursor...");
    //     stream.advance().await?;
    //
    //     println!("Seeking to byte 10...");
    //     stream.seek(10).await?;
    //
    //     println!("Backtracking 5 bytes...");
    //     stream.backtrack(5).await?;
    //
    //     Ok(())
    // }
}
