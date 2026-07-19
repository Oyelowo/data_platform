//! Variable-length unsigned integer encoding (LEB128 / uvarint).

use std::io::{self, Read, Write};

/// Maximum number of bytes needed to encode a `u64` as a uvarint.
pub const MAX_VARINT_LEN: usize = 10;

/// Encode `value` as a uvarint into `buf`.
///
/// Returns the number of bytes written.
///
/// # Panics
///
/// Panics if `buf` is too short to hold the encoded value.
pub fn encode_uvarint(buf: &mut [u8], mut value: u64) -> usize {
    let mut i = 0;
    while value >= 0x80 {
        buf[i] = (value as u8) | 0x80;
        value >>= 7;
        i += 1;
    }
    buf[i] = value as u8;
    i + 1
}

/// Return the number of bytes needed to encode `value` as a uvarint.
pub fn encoded_uvarint_len(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

/// Decode a uvarint from the start of `buf`.
///
/// Returns the decoded value and the number of bytes consumed.
pub fn decode_uvarint(buf: &[u8]) -> Result<(u64, usize), VarintError> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut i = 0;

    loop {
        if i >= buf.len() {
            return Err(VarintError::UnexpectedEof);
        }
        if i >= MAX_VARINT_LEN {
            return Err(VarintError::Overflow);
        }
        let b = buf[i];
        let v = (b & 0x7f) as u64;
        value |= v << shift;
        i += 1;
        if b & 0x80 == 0 {
            return Ok((value, i));
        }
        shift += 7;
    }
}

/// Write a uvarint to `writer`.
pub fn write_uvarint<W: Write>(writer: &mut W, value: u64) -> io::Result<usize> {
    let mut buf = [0u8; MAX_VARINT_LEN];
    let len = encode_uvarint(&mut buf, value);
    writer.write_all(&buf[..len])?;
    Ok(len)
}

/// Read a uvarint from `reader`.
pub fn read_uvarint<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut buf = [0u8; 1];

    for _ in 0..MAX_VARINT_LEN {
        reader.read_exact(&mut buf)?;
        let b = buf[0];
        value |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }

    Err(io::Error::new(io::ErrorKind::InvalidData, "uvarint overflow"))
}

/// Errors that can occur while decoding a uvarint.
#[derive(Debug, thiserror::Error)]
pub enum VarintError {
    /// The buffer ended before the varint terminated.
    #[error("unexpected end of uvarint")]
    UnexpectedEof,

    /// The varint is too long to fit in a `u64`.
    #[error("uvarint overflow")]
    Overflow,
}
