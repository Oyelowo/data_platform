//! WAL record encoding for the B+ tree engine.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::{Error, Result};

/// Maximum number of operations allowed in a single WAL batch record.
const MAX_BATCH_OPS: usize = 10_000;

/// Logical operation recorded in the WAL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WalRecord {
    /// Insert or overwrite a key.
    Put { key: Bytes, value: Bytes },
    /// Delete a key.
    Delete { key: Bytes },
    /// Atomic batch of multiple operations committed as a single transaction.
    ///
    /// A batch record is appended to the WAL once and fsynced once, so either
    /// all operations in the batch survive a crash or none do.
    Batch(Vec<BatchOp>),
}

/// A single operation inside a [`WalRecord::Batch`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BatchOp {
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
            WalRecord::Batch(ops) => {
                buf.put_u8(2);
                buf.put_u32_le(ops.len() as u32);
                for op in ops {
                    match op {
                        BatchOp::Put { key, value } => {
                            buf.put_u8(0);
                            buf.put_u16_le(key.len() as u16);
                            buf.extend_from_slice(key);
                            buf.put_u32_le(value.len() as u32);
                            buf.extend_from_slice(value);
                        }
                        BatchOp::Delete { key } => {
                            buf.put_u8(1);
                            buf.put_u16_le(key.len() as u16);
                            buf.extend_from_slice(key);
                        }
                    }
                }
            }
        }
        buf.freeze()
    }

    /// Decode a record from a byte payload.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut view = data;
        if view.is_empty() {
            return Err(Error::Corruption("wal record type truncated".into()));
        }
        let ty = view.get_u8();
        match ty {
            0 => {
                if view.len() < 2 {
                    return Err(Error::Corruption("wal record key length truncated".into()));
                }
                let key_len = view.get_u16_le() as usize;
                if view.len() < key_len {
                    return Err(Error::Corruption("wal record key truncated".into()));
                }
                let key = Bytes::copy_from_slice(&view[..key_len]);
                view.advance(key_len);
                if view.len() < 4 {
                    return Err(Error::Corruption(
                        "wal record value length truncated".into(),
                    ));
                }
                let value_len = view.get_u32_le() as usize;
                if view.len() < value_len {
                    return Err(Error::Corruption("wal record value truncated".into()));
                }
                let value = Bytes::copy_from_slice(&view[..value_len]);
                Ok(Self::Put { key, value })
            }
            1 => {
                if view.len() < 2 {
                    return Err(Error::Corruption("wal record key length truncated".into()));
                }
                let key_len = view.get_u16_le() as usize;
                if view.len() < key_len {
                    return Err(Error::Corruption("wal record key truncated".into()));
                }
                let key = Bytes::copy_from_slice(&view[..key_len]);
                Ok(Self::Delete { key })
            }
            2 => {
                if view.len() < 4 {
                    return Err(Error::Corruption("wal batch count truncated".into()));
                }
                let count = view.get_u32_le() as usize;
                if count > MAX_BATCH_OPS {
                    return Err(Error::Corruption(format!(
                        "wal batch count {count} exceeds maximum {MAX_BATCH_OPS}"
                    )));
                }
                let mut ops = Vec::with_capacity(count);
                for _ in 0..count {
                    if view.len() < 3 {
                        return Err(Error::Corruption("wal batch op truncated".into()));
                    }
                    let op_ty = view.get_u8();
                    let key_len = view.get_u16_le() as usize;
                    if view.len() < key_len {
                        return Err(Error::Corruption("wal batch key truncated".into()));
                    }
                    let key = Bytes::copy_from_slice(&view[..key_len]);
                    view.advance(key_len);
                    match op_ty {
                        0 => {
                            if view.len() < 4 {
                                return Err(Error::Corruption(
                                    "wal batch value length truncated".into(),
                                ));
                            }
                            let value_len = view.get_u32_le() as usize;
                            if view.len() < value_len {
                                return Err(Error::Corruption("wal batch value truncated".into()));
                            }
                            let value = Bytes::copy_from_slice(&view[..value_len]);
                            view.advance(value_len);
                            ops.push(BatchOp::Put { key, value });
                        }
                        1 => ops.push(BatchOp::Delete { key }),
                        _ => {
                            return Err(Error::Corruption(format!(
                                "unknown wal batch op type {op_ty}"
                            )));
                        }
                    }
                }
                Ok(Self::Batch(ops))
            }
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
            key: Bytes::from_static(b"k"),
            value: Bytes::from_static(b"v"),
        };
        assert_eq!(WalRecord::decode(&rec.encode()).unwrap(), rec);
    }

    #[test]
    fn delete_roundtrip() {
        let rec = WalRecord::Delete {
            key: Bytes::from_static(b"k"),
        };
        assert_eq!(WalRecord::decode(&rec.encode()).unwrap(), rec);
    }

    #[test]
    fn batch_roundtrip() {
        let rec = WalRecord::Batch(vec![
            BatchOp::Put {
                key: Bytes::from_static(b"a"),
                value: Bytes::from_static(b"1"),
            },
            BatchOp::Delete {
                key: Bytes::from_static(b"b"),
            },
        ]);
        assert_eq!(WalRecord::decode(&rec.encode()).unwrap(), rec);
    }
}
