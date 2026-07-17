//! WAL record encoding for the B+ tree engine.

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
                buf.put_u16_le(key.len() as u16);
                buf.extend_from_slice(key);
                buf.put_u32_le(value.len() as u32);
                buf.extend_from_slice(value);
            }
            WalRecord::Delete { key } => {
                buf.put_u8(1);
                buf.put_u16_le(key.len() as u16);
                buf.extend_from_slice(key);
            }
        }
        buf.freeze()
    }

    /// Decode a record from a byte payload.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut view = data;
        let ty = view.get_u8();
        let key_len = view.get_u16_le() as usize;
        if view.len() < key_len {
            return Err(Error::Corruption("wal record key truncated".into()));
        }
        let key = Bytes::copy_from_slice(&view[..key_len]);
        view.advance(key_len);
        match ty {
            0 => {
                let value_len = view.get_u32_le() as usize;
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
