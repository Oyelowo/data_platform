//! Checksum helpers.

/// Compute the CRC32C of `data`.
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c::crc32c(data)
}

/// A small helper that accumulates CRC32C over multiple buffers.
#[derive(Debug, Clone, Copy, Default)]
pub struct Crc32c {
    value: u32,
}

impl Crc32c {
    /// Create a new CRC32C accumulator initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the accumulator with `data`.
    pub fn update(&mut self, data: &[u8]) {
        self.value = crc32c_append(self.value, data);
    }

    /// Return the current checksum value.
    pub fn finalize(self) -> u32 {
        self.value
    }
}

use crc32c::crc32c_append;
