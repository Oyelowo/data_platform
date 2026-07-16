//! Framed postcard serialization over byte streams.

use std::io::{self, Read, Write};

use thiserror::Error;

use super::message::{Request, Response};

#[derive(Debug, Error)]
pub enum SerializeError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("postcard error: {0}")]
    Postcard(#[from] postcard::Error),
}

/// Read a framed postcard message.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, SerializeError> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > 64 * 1024 * 1024 {
        return Err(SerializeError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        )));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

/// Write a framed postcard message.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<(), SerializeError> {
    let len = u32::try_from(payload.len()).map_err(|_| {
        SerializeError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "payload too large",
        ))
    })?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

/// Read a `Request`.
pub fn read_request<R: Read>(reader: &mut R) -> Result<Request, SerializeError> {
    let buf = read_frame(reader)?;
    Ok(postcard::from_bytes(&buf)?)
}

/// Write a `Request`.
pub fn write_request<W: Write>(writer: &mut W, request: &Request) -> Result<(), SerializeError> {
    let buf = postcard::to_allocvec(request)?;
    write_frame(writer, &buf)
}

/// Read a `Response`.
pub fn read_response<R: Read>(reader: &mut R) -> Result<Response, SerializeError> {
    let buf = read_frame(reader)?;
    Ok(postcard::from_bytes(&buf)?)
}

/// Write a `Response`.
pub fn write_response<W: Write>(writer: &mut W, response: &Response) -> Result<(), SerializeError> {
    let buf = postcard::to_allocvec(response)?;
    write_frame(writer, &buf)
}

pub type MessageReader<R> = fn(&mut R) -> Result<Request, SerializeError>;
pub type MessageWriter<W> = fn(&mut W, &Response) -> Result<(), SerializeError>;
