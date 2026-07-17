//! On-disk record format for volume files.

/// Magic number at the start of every record ("BLOB").
pub const RECORD_MAGIC: u32 = 0x424C_4F42;

/// Current record format version.
pub const RECORD_VERSION: u8 = 1;

/// Size of the fixed record header in bytes.
pub const HEADER_SIZE: usize = 24;

/// Bit flag: record has been deleted logically.  The data may still be on disk
/// until garbage collection rewrites the volume.
pub const FLAG_DELETED: u8 = 0x01;

/// Fixed-size record header written before every object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
    /// Format version; must equal `RECORD_VERSION`.
    pub version: u8,
    /// Bit flags, e.g. `FLAG_DELETED`.
    pub flags: u8,
    /// Length of the object ID in bytes.
    pub id_len: u32,
    /// Length of the object payload in bytes.
    pub payload_len: u64,
    /// CRC32C of the payload bytes.
    pub payload_crc: u32,
}

impl RecordHeader {
    /// Create a header for a live (non-deleted) record.
    pub fn new(id_len: u32, payload_len: u64, payload_crc: u32) -> Self {
        Self {
            version: RECORD_VERSION,
            flags: 0,
            id_len,
            payload_len,
            payload_crc,
        }
    }

    /// Total size of the record on disk, including header, id, payload and padding.
    pub fn record_size(&self) -> u64 {
        padded_record_size(self.id_len, self.payload_len)
    }

    /// Byte offset of the payload relative to the start of the record header.
    pub fn payload_offset(&self) -> u64 {
        HEADER_SIZE as u64 + self.id_len as u64
    }

    /// Encode the header into a 24-byte buffer.
    pub fn encode(&self, buf: &mut [u8; HEADER_SIZE]) {
        buf[0..4].copy_from_slice(&RECORD_MAGIC.to_le_bytes());
        buf[4] = self.version;
        buf[5] = self.flags;
        buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // reserved
        buf[8..12].copy_from_slice(&self.id_len.to_le_bytes());
        buf[12..20].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[20..24].copy_from_slice(&self.payload_crc.to_le_bytes());
    }

    /// Decode a 24-byte header.
    pub fn decode(buf: &[u8; HEADER_SIZE]) -> crate::Result<Self> {
        let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if magic != RECORD_MAGIC {
            return Err(crate::Error::CorruptRecord {
                volume: 0,
                offset: 0,
                message: format!("bad record magic: {:08x}", magic),
            });
        }
        let version = buf[4];
        if version != RECORD_VERSION {
            return Err(crate::Error::CorruptRecord {
                volume: 0,
                offset: 0,
                message: format!("unsupported record version: {}", version),
            });
        }
        let flags = buf[5];
        let _reserved = u16::from_le_bytes(buf[6..8].try_into().unwrap());
        let id_len = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        let payload_len = u64::from_le_bytes(buf[12..20].try_into().unwrap());
        let payload_crc = u32::from_le_bytes(buf[20..24].try_into().unwrap());
        Ok(Self {
            version,
            flags,
            id_len,
            payload_len,
            payload_crc,
        })
    }
}

/// Compute the total on-disk size of a record, including 8-byte padding.
pub fn padded_record_size(id_len: u32, payload_len: u64) -> u64 {
    let raw = HEADER_SIZE as u64 + id_len as u64 + payload_len;
    let pad = (8 - (raw % 8)) % 8;
    raw + pad
}

/// Compute padding bytes needed after `id_len + payload_len`.
pub fn padding_len(id_len: u32, payload_len: u64) -> u64 {
    let raw = HEADER_SIZE as u64 + id_len as u64 + payload_len;
    (8 - (raw % 8)) % 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = RecordHeader::new(5, 13, 0xDEAD_BEEF);
        let mut buf = [0u8; HEADER_SIZE];
        h.encode(&mut buf);
        let d = RecordHeader::decode(&buf).unwrap();
        assert_eq!(h, d);
    }

    #[test]
    fn padding_is_aligned() {
        for id_len in [0, 1, 7, 8, 9] {
            for payload_len in [0u64, 1, 7, 8, 9, 100] {
                let size = padded_record_size(id_len, payload_len);
                assert_eq!(size % 8, 0, "id={}, payload={}", id_len, payload_len);
                let pad = padding_len(id_len, payload_len);
                assert_eq!(HEADER_SIZE as u64 + id_len as u64 + payload_len + pad, size);
            }
        }
    }
}
