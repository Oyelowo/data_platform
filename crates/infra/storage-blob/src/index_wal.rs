//! Index-operation records stored in `storage_wal`.

use bytes::Buf;

/// Type tag for an index WAL record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum RecordTag {
    Put = 1,
    Delete = 2,
    GcMove = 3,
}

impl RecordTag {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(RecordTag::Put),
            2 => Some(RecordTag::Delete),
            3 => Some(RecordTag::GcMove),
            _ => None,
        }
    }
}

/// A durable index operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexRecord {
    /// A new object was written to a volume.
    Put {
        id: Vec<u8>,
        volume_number: u64,
        offset: u64,
        payload_len: u64,
        payload_crc: u32,
    },
    /// An object was deleted.
    Delete { id: Vec<u8> },
    /// GC moved a live object to a new volume.
    GcMove {
        id: Vec<u8>,
        old_volume_number: u64,
        new_volume_number: u64,
        new_offset: u64,
        new_payload_len: u64,
        new_payload_crc: u32,
    },
}

impl IndexRecord {
    /// Encode the record into a byte vector.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            IndexRecord::Put {
                id,
                volume_number,
                offset,
                payload_len,
                payload_crc,
            } => {
                let mut buf = Vec::with_capacity(1 + 4 + id.len() + 24);
                buf.push(RecordTag::Put as u8);
                put_id(&mut buf, id);
                buf.extend_from_slice(&volume_number.to_le_bytes());
                buf.extend_from_slice(&offset.to_le_bytes());
                buf.extend_from_slice(&payload_len.to_le_bytes());
                buf.extend_from_slice(&payload_crc.to_le_bytes());
                buf
            }
            IndexRecord::Delete { id } => {
                let mut buf = Vec::with_capacity(1 + 4 + id.len());
                buf.push(RecordTag::Delete as u8);
                put_id(&mut buf, id);
                buf
            }
            IndexRecord::GcMove {
                id,
                old_volume_number,
                new_volume_number,
                new_offset,
                new_payload_len,
                new_payload_crc,
            } => {
                let mut buf = Vec::with_capacity(1 + 4 + id.len() + 36);
                buf.push(RecordTag::GcMove as u8);
                put_id(&mut buf, id);
                buf.extend_from_slice(&old_volume_number.to_le_bytes());
                buf.extend_from_slice(&new_volume_number.to_le_bytes());
                buf.extend_from_slice(&new_offset.to_le_bytes());
                buf.extend_from_slice(&new_payload_len.to_le_bytes());
                buf.extend_from_slice(&new_payload_crc.to_le_bytes());
                buf
            }
        }
    }

    /// Decode a record from bytes.
    pub fn decode(bytes: &[u8]) -> crate::Result<Self> {
        if bytes.is_empty() {
            return Err(crate::Error::IndexWal("empty record".into()));
        }
        let tag = RecordTag::from_u8(bytes[0])
            .ok_or_else(|| crate::Error::IndexWal(format!("unknown record tag: {}", bytes[0])))?;
        let mut cur = std::io::Cursor::new(&bytes[1..]);
        match tag {
            RecordTag::Put => {
                let id = take_id(&mut cur)?;
                let volume_number = cur.get_u64_le();
                let offset = cur.get_u64_le();
                let payload_len = cur.get_u64_le();
                let payload_crc = cur.get_u32_le();
                Ok(IndexRecord::Put {
                    id,
                    volume_number,
                    offset,
                    payload_len,
                    payload_crc,
                })
            }
            RecordTag::Delete => {
                let id = take_id(&mut cur)?;
                Ok(IndexRecord::Delete { id })
            }
            RecordTag::GcMove => {
                let id = take_id(&mut cur)?;
                let old_volume_number = cur.get_u64_le();
                let new_volume_number = cur.get_u64_le();
                let new_offset = cur.get_u64_le();
                let new_payload_len = cur.get_u64_le();
                let new_payload_crc = cur.get_u32_le();
                Ok(IndexRecord::GcMove {
                    id,
                    old_volume_number,
                    new_volume_number,
                    new_offset,
                    new_payload_len,
                    new_payload_crc,
                })
            }
        }
    }
}

fn put_id(buf: &mut Vec<u8>, id: &[u8]) {
    buf.extend_from_slice(&(id.len() as u32).to_le_bytes());
    buf.extend_from_slice(id);
}

fn take_id(cur: &mut std::io::Cursor<&[u8]>) -> crate::Result<Vec<u8>> {
    if cur.remaining() < 4 {
        return Err(crate::Error::IndexWal("truncated id length".into()));
    }
    let len = cur.get_u32_le() as usize;
    if cur.remaining() < len {
        return Err(crate::Error::IndexWal("truncated id bytes".into()));
    }
    let mut id = vec![0u8; len];
    cur.copy_to_slice(&mut id);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_roundtrip() {
        let r = IndexRecord::Put {
            id: b"obj-1".to_vec(),
            volume_number: 7,
            offset: 1024,
            payload_len: 4096,
            payload_crc: 0xCAFE_BABE,
        };
        let bytes = r.encode();
        let d = IndexRecord::decode(&bytes).unwrap();
        assert_eq!(r, d);
    }

    #[test]
    fn delete_roundtrip() {
        let r = IndexRecord::Delete {
            id: b"dead".to_vec(),
        };
        let bytes = r.encode();
        let d = IndexRecord::decode(&bytes).unwrap();
        assert_eq!(r, d);
    }

    #[test]
    fn gc_move_roundtrip() {
        let r = IndexRecord::GcMove {
            id: b"live".to_vec(),
            old_volume_number: 1,
            new_volume_number: 2,
            new_offset: 2048,
            new_payload_len: 100,
            new_payload_crc: 0xDEAD_BEEF,
        };
        let bytes = r.encode();
        let d = IndexRecord::decode(&bytes).unwrap();
        assert_eq!(r, d);
    }
}
