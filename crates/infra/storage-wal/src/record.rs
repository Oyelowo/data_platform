//! Record framing, checksums, and durability contracts.

use bytes::{Buf, BufMut, Bytes};

use crate::{Error, Lsn, Result};

/// Magic number at the start of every record.
const RECORD_MAGIC: u32 = 0x57_A1_00_01;

/// On-disk record header size in bytes.
pub const RECORD_HEADER_SIZE: usize = 4 /* magic */ + 1 /* type */ + 8 /* lsn */ + 4 /* payload len */ + 4 /* crc */;

/// Maximum payload size per record (16 MiB). Keeps memory usage bounded and
/// allows length to fit in a `u32`.
pub const MAX_PAYLOAD_SIZE: usize = 16 * 1024 * 1024;

/// Record type stored in the WAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordType {
    /// Application data insert/update.
    Put = 1,
    /// Tombstone / deletion marker.
    Delete = 2,
    /// Checkpoint / truncation marker.
    Checkpoint = 3,
    /// Padding record used to skip to the next segment boundary.
    Blank = 4,
}

impl RecordType {
    fn from_u8(value: u8) -> Result<Self> {
        match value {
            1 => Ok(RecordType::Put),
            2 => Ok(RecordType::Delete),
            3 => Ok(RecordType::Checkpoint),
            4 => Ok(RecordType::Blank),
            other => Err(Error::CorruptRecord {
                lsn: 0,
                reason: format!("unknown record type {other}"),
            }),
        }
    }
}

/// Durability promise requested by the caller.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// Do not wait for fsync; append to the OS page cache and return.
    /// Records may be lost on power failure.
    Buffered,
    /// Block until the record is durably persisted (default).
    #[default]
    Immediate,
}

/// A logical WAL record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub lsn: Lsn,
    pub ty: RecordType,
    pub payload: Bytes,
}

impl Record {
    /// Create a new record. The caller must assign a unique, monotonic LSN.
    pub fn new(ty: RecordType, payload: impl Into<Bytes>) -> Self {
        Self {
            lsn: 0,
            ty,
            payload: payload.into(),
        }
    }

    /// Serialize the record into a byte buffer including header, payload, and
    /// CRC32C checksum.
    ///
    /// On-disk layout (little-endian):
    /// ```text
    /// magic(4) | type(1) | lsn(8) | payload_len(4) | payload(N) | crc(4)
    /// ```
    pub fn encode(&self, buf: &mut Vec<u8>) -> Result<usize> {
        if self.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(Error::InvalidArgument(format!(
                "payload exceeds {} bytes",
                MAX_PAYLOAD_SIZE
            )));
        }

        let start = buf.len();
        buf.put_u32_le(RECORD_MAGIC);
        buf.put_u8(self.ty as u8);
        buf.put_u64_le(self.lsn);
        buf.put_u32_le(self.payload.len() as u32);
        buf.extend_from_slice(&self.payload);

        // CRC covers everything except the magic and the CRC field itself.
        let crc = crc32c::crc32c(&buf[start + 4..]);
        buf.put_u32_le(crc);

        Ok(buf.len() - start)
    }

    /// Decode a single record from the front of the buffer. Returns the record
    /// and the number of bytes consumed, or `None` if there are not enough
    /// bytes yet.
    pub fn decode(buf: &[u8]) -> Result<Option<(Record, usize)>> {
        if buf.len() < RECORD_HEADER_SIZE {
            return Ok(None);
        }

        let mut cursor = buf;
        let magic = cursor.get_u32_le();
        if magic != RECORD_MAGIC {
            return Err(Error::CorruptRecord {
                lsn: 0,
                reason: format!("bad magic {magic:#x}"),
            });
        }

        let ty = RecordType::from_u8(cursor.get_u8())?;
        let lsn = cursor.get_u64_le();
        let payload_len = cursor.get_u32_le() as usize;

        let total_len = RECORD_HEADER_SIZE + payload_len;
        if buf.len() < total_len {
            return Ok(None);
        }

        let payload_start = RECORD_HEADER_SIZE - 4; // before the trailing CRC
        let payload = Bytes::copy_from_slice(&buf[payload_start..payload_start + payload_len]);
        let stored_crc = &buf[payload_start + payload_len..payload_start + payload_len + 4];
        let stored_crc = u32::from_le_bytes([stored_crc[0], stored_crc[1], stored_crc[2], stored_crc[3]]);

        let computed_crc = crc32c::crc32c(&buf[4..payload_start + payload_len]);
        if computed_crc != stored_crc {
            return Err(Error::ChecksumMismatch {
                lsn,
                expected: stored_crc,
                got: computed_crc,
            });
        }

        Ok(Some((Record { lsn, ty, payload }, total_len)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut rec = Record::new(RecordType::Put, &b"hello world"[..]);
        rec.lsn = 42;
        let mut buf = Vec::new();
        rec.encode(&mut buf).unwrap();
        let (decoded, consumed) = Record::decode(&buf).unwrap().unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, rec);
    }

    #[test]
    fn truncated_buffer_returns_none() {
        let mut rec = Record::new(RecordType::Put, &b"data"[..]);
        rec.lsn = 1;
        let mut buf = Vec::new();
        rec.encode(&mut buf).unwrap();
        for n in 1..buf.len() {
            assert!(Record::decode(&buf[..n]).unwrap().is_none());
        }
    }

    #[test]
    fn checksum_mismatch() {
        let mut rec = Record::new(RecordType::Put, &b"data"[..]);
        rec.lsn = 1;
        let mut buf = Vec::new();
        rec.encode(&mut buf).unwrap();
        buf[RECORD_HEADER_SIZE - 1] = buf[RECORD_HEADER_SIZE - 1].wrapping_add(1);
        assert!(matches!(
            Record::decode(&buf),
            Err(Error::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn max_payload_size() {
        let big = vec![0u8; MAX_PAYLOAD_SIZE];
        let rec = Record::new(RecordType::Put, big);
        let mut buf = Vec::new();
        assert!(rec.encode(&mut buf).is_ok());
    }

    #[test]
    fn oversize_payload_rejected() {
        let huge = vec![0u8; MAX_PAYLOAD_SIZE + 1];
        let rec = Record::new(RecordType::Put, huge);
        let mut buf = Vec::new();
        assert!(rec.encode(&mut buf).is_err());
    }
}
