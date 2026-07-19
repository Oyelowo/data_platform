//! Little-endian integer read/write helpers.

use bytes::Buf;

/// Read a little-endian `u16` from the start of `buf`.
///
/// # Panics
///
/// Panics if `buf` is shorter than 2 bytes.
pub fn read_u16_le(buf: &[u8]) -> u16 {
    u16::from_le_bytes(buf[..2].try_into().expect("buf too short for u16"))
}

/// Read a little-endian `u32` from the start of `buf`.
///
/// # Panics
///
/// Panics if `buf` is shorter than 4 bytes.
pub fn read_u32_le(buf: &[u8]) -> u32 {
    u32::from_le_bytes(buf[..4].try_into().expect("buf too short for u32"))
}

/// Read a little-endian `u64` from the start of `buf`.
///
/// # Panics
///
/// Panics if `buf` is shorter than 8 bytes.
pub fn read_u64_le(buf: &[u8]) -> u64 {
    u64::from_le_bytes(buf[..8].try_into().expect("buf too short for u64"))
}

/// Write a little-endian `u16` into the start of `buf`.
///
/// # Panics
///
/// Panics if `buf` is shorter than 2 bytes.
pub fn write_u16_le(buf: &mut [u8], value: u16) {
    buf[..2].copy_from_slice(&value.to_le_bytes());
}

/// Write a little-endian `u32` into the start of `buf`.
///
/// # Panics
///
/// Panics if `buf` is shorter than 4 bytes.
pub fn write_u32_le(buf: &mut [u8], value: u32) {
    buf[..4].copy_from_slice(&value.to_le_bytes());
}

/// Write a little-endian `u64` into the start of `buf`.
///
/// # Panics
///
/// Panics if `buf` is shorter than 8 bytes.
pub fn write_u64_le(buf: &mut [u8], value: u64) {
    buf[..8].copy_from_slice(&value.to_le_bytes());
}

/// Consume 2 bytes from `buf` and return a little-endian `u16`.
///
/// # Panics
///
/// Panics if `buf` does not have 2 remaining bytes.
pub fn get_u16_le<B: Buf>(buf: &mut B) -> u16 {
    buf.get_u16_le()
}

/// Consume 4 bytes from `buf` and return a little-endian `u32`.
///
/// # Panics
///
/// Panics if `buf` does not have 4 remaining bytes.
pub fn get_u32_le<B: Buf>(buf: &mut B) -> u32 {
    buf.get_u32_le()
}

/// Consume 8 bytes from `buf` and return a little-endian `u64`.
///
/// # Panics
///
/// Panics if `buf` does not have 8 remaining bytes.
pub fn get_u64_le<B: Buf>(buf: &mut B) -> u64 {
    buf.get_u64_le()
}
