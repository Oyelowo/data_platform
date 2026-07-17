//! WAL record encoding for the Bw-Tree engine.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::{Error, Result};

/// Logical operation recorded in the WAL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WalRecord {
    /// Insert or overwrite a key.
    Put { key: Bytes, value: Bytes },
    /// Delete a key.
    Delete { key: Bytes },
}

impl WalRecord {
    /// Encode the record into a byte payload for `storage-wal`.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        match self {
            WalRecord::Put { key, value } => {
                buf.put_u8(0);
                buf.put_u64_le(key.len() as u64);
                buf.extend_from_slice(key);
                buf.put_u64_le(value.len() as u64);
                buf.extend_from_slice(value);
            }
            WalRecord::Delete { key } => {
                buf.put_u8(1);
                buf.put_u64_le(key.len() as u64);
                buf.extend_from_slice(key);
            }
        }
        buf.freeze()
    }

    /// Decode a record from a byte payload.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut view = data;
        let ty = view.get_u8();
        let key_len = view.get_u64_le() as usize;
        if view.len() < key_len {
            return Err(Error::Corruption("wal record key truncated".into()));
        }
        let key = Bytes::copy_from_slice(&view[..key_len]);
        view.advance(key_len);
        match ty {
            0 => {
                let value_len = view.get_u64_le() as usize;
                if view.len() < value_len {
                    return Err(Error::Corruption("wal record value truncated".into()));
                }
                let value = Bytes::copy_from_slice(&view[..value_len]);
                Ok(Self::Put { key, value })
            }
            1 => Ok(Self::Delete { key }),
            _ => Err(Error::Corruption(format!("unknown wal record type {ty}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_roundtrip() {
        let rec = WalRecord::Put {
            key: Bytes::from_static(b"hello"),
            value: Bytes::from_static(b"world"),
        };
        let decoded = WalRecord::decode(&rec.encode()).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn delete_roundtrip() {
        let rec = WalRecord::Delete {
            key: Bytes::from_static(b"hello"),
        };
        let decoded = WalRecord::decode(&rec.encode()).unwrap();
        assert_eq!(rec, decoded);
    }

    #[test]
    fn empty_key_value_roundtrip() {
        let rec = WalRecord::Put {
            key: Bytes::new(),
            value: Bytes::new(),
        };
        let decoded = WalRecord::decode(&rec.encode()).unwrap();
        assert_eq!(rec, decoded);
    }
}
